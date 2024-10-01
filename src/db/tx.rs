use crate::{
    data::fb::TransactionInfoExtendedT,
    network::Network,
    txdetails::TransactionDetails,
    warp::sync::{ExtendedReceivedTx, ReceivedTx, TxValueUpdate},
    Hash,
};
use anyhow::Result;
use rusqlite::{params, Connection, Transaction};

use crate::{
    coin::COINS,
    ffi::{map_result_bytes, CParam, CResult},
};
use flatbuffers::FlatBufferBuilder;
use warp_macros::c_export;

use super::contacts::address_to_bytes;

pub fn list_new_txids(connection: &Connection) -> Result<Vec<(u32, u32, u32, Hash)>> {
    let mut s = connection.prepare(
        "SELECT t.id_tx, t.account, t.timestamp, t.txid FROM txs t
        LEFT JOIN txdetails d ON t.id_tx = d.id_tx WHERE d.id_tx IS NULL",
    )?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    let mut res = vec![];
    for r in rows {
        let (id_tx, account, timestamp, txid) = r?;
        let txid: Hash = txid.try_into().unwrap();
        res.push((id_tx, account, timestamp, txid));
    }
    Ok(res)
}

pub fn list_txs(connection: &Connection, account: u32) -> Result<Vec<ExtendedReceivedTx>> {
    let mut s = connection.prepare(
        "SELECT t.id_tx, t.txid, t.height, t.timestamp, t.value, t.address, c.name, t.memo FROM txs t
        LEFT JOIN contact_receivers r ON r.address = t.receiver AND r.account = t.account
        LEFT JOIN contacts c ON c.id_contact = r.contact
        WHERE t.account = ?1 ORDER BY t.height DESC",
    )?;
    let rows = s.query_map([account], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, u32>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut txs = vec![];
    for r in rows {
        let (id_tx, txid, height, timestamp, value, address, contact, memo) = r?;
        let rtx = ReceivedTx {
            id: id_tx,
            account,
            height,
            txid: txid.try_into().unwrap(),
            timestamp,
            value,
            ivtx: 0,
        };
        let ertx = ExtendedReceivedTx {
            rtx,
            address,
            contact,
            memo,
        };
        txs.push(ertx);
    }
    Ok(txs)
}

pub fn get_tx(connection: &Connection, id_tx: u32) -> Result<ReceivedTx> {
    let (account, txid, height, timestamp, value) = connection.query_row(
        "SELECT account, txid, height, timestamp, value
        FROM txs WHERE id_tx = ?1",
        [id_tx],
        |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, i64>(4)?,
            ))
        },
    )?;
    let tx = ReceivedTx {
        id: id_tx,
        account,
        height,
        txid: txid.try_into().unwrap(),
        timestamp,
        value,
        ivtx: 0,
    };
    Ok(tx)
}

pub fn get_tx_details_account(
    connection: &Connection,
    id_tx: u32,
) -> Result<(u32, TransactionDetails)> {
    let (account, tx_bin) = connection.query_row(
        "SELECT t.account, d.data FROM txs t
        JOIN txdetails d ON t.id_tx = d.id_tx 
        WHERE t.id_tx = ?1",
        [id_tx],
        |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?)),
    )?;
    let tx: TransactionDetails = bincode::deserialize_from(&*tx_bin)?;
    Ok((account, tx))
}

#[c_export]
pub fn get_tx_details(
    network: &Network,
    connection: &Connection,
    txid: &[u8],
) -> Result<TransactionInfoExtendedT> {
    let tx_bin =
        connection.query_row("SELECT data FROM txdetails WHERE txid = ?1", [txid], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
    let tx: TransactionDetails = bincode::deserialize_from(&*tx_bin)?;
    let etx = tx.to_transaction_info_ext(network);
    Ok(etx)
}

pub fn store_tx(connection: &Transaction, tx: &ReceivedTx) -> Result<()> {
    let mut s_tx = connection.prepare_cached(
        "INSERT INTO txs
        (account, txid, height, timestamp, value)
        VAlUES (?1, ?2, ?3, ?4, 0)
        ON CONFLICT DO NOTHING",
    )?;
    s_tx.execute(params![tx.account, tx.txid, tx.height, tx.timestamp,])?;
    Ok(())
}

pub fn add_tx_value<IDSpent: std::fmt::Debug>(
    connection: &Transaction,
    tx_value: &TxValueUpdate<IDSpent>,
) -> Result<()> {
    let mut s_tx = connection.prepare_cached(
        "INSERT INTO txs(account, txid, height, timestamp, value)
        VALUES (?1, ?2, ?3, ?4, 0) ON CONFLICT DO NOTHING",
    )?;
    s_tx.execute(params![
        tx_value.account,
        tx_value.txid,
        tx_value.height,
        tx_value.timestamp
    ])?;
    let mut s_tx = connection
        .prepare_cached("UPDATE txs SET value = value + ?3 WHERE txid = ?1 AND account = ?2")?;
    s_tx.execute(params![tx_value.txid, tx_value.account, tx_value.value])?;
    Ok(())
}

pub fn update_tx_primary_address_memo(
    network: &Network,
    connection: &Connection,
    id_tx: u32,
    address: Option<String>,
    memo: Option<String>,
) -> Result<()> {
    let receiver = address
        .as_ref()
        .map(|a| address_to_bytes(network, a).unwrap());
    connection.execute(
        "UPDATE txs SET address = ?2, receiver = ?3, memo = ?4 WHERE id_tx = ?1",
        params![id_tx, address, receiver, memo],
    )?;
    Ok(())
}

pub fn store_tx_details(
    connection: &Connection,
    id: u32,
    account: u32,
    height: u32,
    txid: &Hash,
    data: &[u8],
) -> Result<()> {
    connection.execute(
        "INSERT INTO txdetails(id_tx, account, height, txid, data)
        VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT DO NOTHING",
        params![id, account, height, txid, data],
    )?;
    Ok(())
}

pub fn get_txid(connection: &Connection, id: u32) -> Result<(Vec<u8>, u32)> {
    let (txid, timestamp) = connection.query_row(
        "SELECT txid, timestamp FROM txs WHERE id_tx = ?1",
        [id],
        |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, u32>(1)?)),
    )?;
    Ok((txid, timestamp))
}

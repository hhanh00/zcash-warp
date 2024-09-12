use crate::{
    txdetails::TransactionDetails,
    warp::sync::{ExtendedReceivedTx, ReceivedTx, TxValueUpdate},
    Hash,
};
use anyhow::Result;
use rusqlite::{params, Connection, Transaction};

pub fn list_new_txids(connection: &Connection) -> Result<Vec<(u32, u32, u32, Hash)>> {
    let mut s = connection.prepare(
        "SELECT t.id_tx, t.account, t.timestamp, t.txid FROM txs t
        LEFT JOIN txdetails d ON t.txid = d.txid WHERE d.txid IS NULL",
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
        "SELECT id_tx, txid, height, timestamp, value, address, memo FROM txs
        WHERE account = ?1",
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
        ))
    })?;
    let mut txs = vec![];
    for r in rows {
        let (id_tx, txid, height, timestamp, value, address, memo) = r?;
        let rtx = ReceivedTx {
            id: id_tx,
            account,
            height,
            txid: txid.try_into().unwrap(),
            timestamp,
            value,
            ivtx: 0,
        };
        let ertx = ExtendedReceivedTx { rtx, address, memo };
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

pub fn get_tx_details(connection: &Connection, id_tx: u32) -> Result<(u32, TransactionDetails)> {
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
    let mut s_tx =
        connection.prepare_cached("INSERT INTO txs(account, txid, height, timestamp, value)
        VALUES (?1, ?2, ?3, ?4, 0) ON CONFLICT DO NOTHING")?;
    s_tx.execute(params![tx_value.account, tx_value.txid, tx_value.height, tx_value.timestamp])?;
    let mut s_tx =
        connection.prepare_cached("UPDATE txs SET value = value + ?2 WHERE txid = ?1")?;
    s_tx.execute(params![tx_value.txid, tx_value.value])?;
    Ok(())
}

pub fn update_tx_primary_address_memo(
    connection: &Connection,
    id_tx: u32,
    address: Option<String>,
    memo: Option<String>,
) -> Result<()> {
    connection.execute(
        "UPDATE txs SET address = ?2, memo = ?3 WHERE id_tx = ?1",
        params![id_tx, address, memo],
    )?;
    Ok(())
}

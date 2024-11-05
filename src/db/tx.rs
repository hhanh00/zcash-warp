use crate::{
    data::fb::TransactionInfoExtendedT,
    network::Network,
    txdetails::TransactionDetails,
    utils::ContextExt,
    warp::sync::{ExtendedReceivedTx, ReceivedTx, TxValueUpdate},
    Hash,
};
use anyhow::Result;
use rusqlite::{params, Connection, Transaction};

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
    let (account, txid, height, timestamp, value) = connection
        .query_row(
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
        )
        .with_file_line(|| format!("No tx {id_tx}"))?;
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
    let (account, tx_bin) = connection
        .query_row(
            "SELECT t.account, d.data FROM txs t
        JOIN txdetails d ON t.id_tx = d.id_tx
        WHERE t.id_tx = ?1",
            [id_tx],
            |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )
        .with_file_line(|| format!("No txdetails {id_tx}"))?;
    let tx: TransactionDetails = bincode::deserialize_from(&*tx_bin)?;
    Ok((account, tx))
}

#[c_export]
pub fn get_tx_details(
    network: &Network,
    connection: &Connection,
    txid: &[u8],
) -> Result<TransactionInfoExtendedT> {
    let tx_bin = connection
        .query_row("SELECT data FROM txdetails WHERE txid = ?1", [txid], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .with_file_line(|| format!("No txdetails {}", hex::encode(txid)))?;
    let tx: TransactionDetails = bincode::deserialize_from(&*tx_bin)?;
    let etx = tx.to_transaction_info_ext(network);
    Ok(etx)
}

pub fn store_tx(connection: &Transaction, tx: &ReceivedTx) -> Result<u32> {
    // Reset value if tx is confirmed
    let mut s_tx = connection.prepare_cached(
        "INSERT INTO txs
        (account, txid, height, timestamp, value)
        VAlUES (?1, ?2, ?3, ?4, 0)
        ON CONFLICT DO UPDATE SET
        height = excluded.height, timestamp = excluded.timestamp,
        value = IIF(height IS NULL, 0, value)
        RETURNING id_tx",
    )?;
    let id = s_tx.query_row(
        params![tx.account, tx.txid, tx.height, tx.timestamp,],
        |r| r.get::<_, u32>(0),
    )?;
    Ok(id)
}

pub fn add_tx_value(connection: &Transaction, tx_value: &TxValueUpdate) -> Result<()> {
    let tx = ReceivedTx {
        id: 0,
        account: tx_value.account,
        height: tx_value.height,
        txid: tx_value.txid,
        timestamp: tx_value.timestamp,
        ivtx: 0,
        value: 0,
    };
    store_tx(connection, &tx)?;
    let mut s_tx = connection.prepare_cached(
        "UPDATE txs SET value = value + ?3,
            timestamp = ?4 WHERE txid = ?1 AND account = ?2",
    )?;
    s_tx.execute(params![
        tx_value.txid,
        tx_value.account,
        tx_value.value,
        tx_value.timestamp
    ])?;
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

pub fn drop_transparent_data(connection: &Connection, account: u32) -> Result<()> {
    connection.execute("DELETE FROM utxos WHERE account = ?1", [account])?;
    connection.execute("DELETE FROM utxo_spends WHERE account = ?1", [account])?;
    Ok(())
}

pub fn update_tx_values(connection: &Connection) -> Result<()> {
    // Update tx values based on notes and spends
    connection.execute("
    WITH v AS (SELECT t.id_tx, t.txid, n.value FROM notes n JOIN txs t ON n.tx = t.id_tx UNION
        SELECT s.id_tx, t.txid, -n.value from note_spends s JOIN notes n ON s.id_note = n.id_note JOIN txs t ON s.id_tx = t.id_tx UNION
        SELECT t.id_tx, u.txid, u.value from utxos u JOIN txs t ON u.txid = t.txid UNION
        SELECT s.id_tx, t.txid, -u.value FROM utxo_spends s JOIN txs t ON s.id_tx = t.id_tx JOIN utxos u ON s.id_utxo = u.id_utxo
        )
    INSERT INTO txs(id_tx, txid, value, account, height, timestamp)
    SELECT v.id_tx, v.txid, SUM(v.value) AS value,
    0 AS account, 0 AS height, 0 AS timestamp
    FROM v GROUP BY id_tx
    ON CONFLICT (id_tx) DO UPDATE SET
    value = excluded.value", [])?;
    Ok(())
}

pub fn get_txid(connection: &Connection, id: u32) -> Result<(Vec<u8>, u32)> {
    let (txid, timestamp) = connection
        .query_row(
            "SELECT txid, timestamp FROM txs WHERE id_tx = ?1",
            [id],
            |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, u32>(1)?)),
        )
        .with_file_line(|| format!("No tx {id}"))?;
    Ok((txid, timestamp))
}

pub fn store_block_time(connection: &Connection, height: u32, timestamp: u32) -> Result<()> {
    connection.execute(
        "INSERT INTO blck_times(height, timestamp)
    VALUES (?1, ?2) ON CONFLICT DO NOTHING",
        params![height, timestamp],
    )?;
    Ok(())
}

pub fn copy_block_times_from_tx(connection: &Connection) -> Result<()> {
    connection.execute(
        "INSERT INTO blck_times(height, timestamp)
    SELECT height, timestamp FROM txs
    WHERE TRUE ON CONFLICT DO NOTHING",
        [],
    )?;
    Ok(())
}

pub fn update_tx_time(connection: &Connection) -> Result<()> {
    connection.execute(
        "INSERT INTO txs(id_tx, timestamp, account, txid, height, value)
        SELECT t.id_tx, b.timestamp, 0, x'', 0, 0 FROM txs t
        JOIN blck_times b ON t.height = b.height WHERE t.timestamp = 0
        ON CONFLICT (id_tx) DO UPDATE SET
        timestamp = excluded.timestamp",
        [],
    )?;
    Ok(())
}

pub fn list_unknown_height_timestamps(connection: &Connection) -> Result<Vec<u32>> {
    let mut s = connection.prepare("SELECT height FROM txs WHERE timestamp = 0")?;
    let rows = s.query_map([], |r| r.get::<_, u32>(0))?;
    let heights = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(heights)
}

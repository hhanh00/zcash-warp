use crate::{
    messages::ZMessage,
    txdetails::TransactionDetails,
    warp::sync::{ReceivedTx, TxValueUpdate},
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
        connection.prepare_cached("UPDATE txs SET value = value + ?2 WHERE txid = ?1")?;
    s_tx.execute(params![tx_value.txid, tx_value.value])?;
    Ok(())
}

pub fn store_message(
    connection: &Connection,
    account: u32,
    tx: &TransactionDetails,
    nout: u32,
    message: &ZMessage,
) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO msgs
        (account, height, timestamp, txid, nout, 
        sender, recipient, subject, body, read)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, false)
        ON CONFLICT DO NOTHING",
    )?;
    s.execute(params![
        account,
        tx.height,
        tx.timestamp,
        tx.txid,
        nout,
        message.sender,
        message.recipient,
        message.subject,
        message.body
    ])?;
    Ok(())
}

pub fn update_tx_primary_address_memo(connection: &Connection, id_tx: u32,
    address: Option<String>, memo: Option<String>) -> Result<()> {
    connection.execute(
        "UPDATE txs SET address = ?2, memo = ?3 WHERE id_tx = ?1",
    params![id_tx, address, memo])?;
    Ok(())
}

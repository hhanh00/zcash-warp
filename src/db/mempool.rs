use anyhow::Result;
use rusqlite::{params, Connection};

use warp_macros::c_export;

use crate::{data::fb::UnconfirmedTxT, warp::sync::ReceivedTx};

#[c_export]
pub fn list_unconfirmed_txs(connection: &Connection, account: u32) -> Result<Vec<UnconfirmedTxT>> {
    let mut s = connection.prepare("SELECT txid, value FROM mempool_txs WHERE account = ?1")?;
    let rows = s.query_map([account], |r| {
        let txid = r.get::<_, Vec<u8>>(0)?;
        let value = r.get::<_, i64>(1)?;
        Ok(UnconfirmedTxT {
            account,
            txid: Some(txid),
            value,
        })
    })?;
    let txs = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(txs)
}

#[c_export]
pub fn get_unconfirmed_balance(connection: &Connection, account: u32) -> Result<i64> {
    let balance = connection.query_row(
        "SELECT SUM(value) FROM mempool_txs
        WHERE account = ?1",
        [account],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    Ok(balance.unwrap_or_default())
}

pub fn store_unconfirmed_tx(connection: &Connection, tx: &ReceivedTx) -> Result<()> {
    let mut s_tx = connection.prepare_cached(
        "INSERT INTO mempool_txs
        (account, txid, value)
        VAlUES (?1, ?2, ?3)
        ON CONFLICT DO NOTHING",
    )?;
    s_tx.execute(params![tx.account, tx.txid, tx.value])?;
    Ok(())
}

pub fn clear_unconfirmed_tx(connection: &Connection) -> Result<()> {
    connection.execute("DELETE FROM mempool_txs", [])?;
    Ok(())
}

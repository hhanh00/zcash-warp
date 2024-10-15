use crate::Hash;
use anyhow::Result;
use rusqlite::{params, Connection};

use crate::{
    db::COINS,
    ffi::{map_result, CResult},
};
use warp_macros::c_export;

#[c_export]
pub fn get_unconfirmed_balance(connection: &Connection, account: u32) -> Result<i64> {
    let balance = connection.query_row(
        "SELECT SUM(value) FROM unconfirmed_txs
        WHERE account = ?1",
        [account],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    Ok(balance.unwrap_or_default())
}

pub fn clear_unconfirmed_tx(connection: &Connection) -> Result<()> {
    connection.execute("DELETE FROM unconfirmed_txs", [])?;
    Ok(())
}

pub fn store_unconfirmed_tx(
    connection: &Connection,
    account: u32,
    txid: &Hash,
    amount: i64,
) -> Result<()> {
    connection.execute(
        "INSERT INTO unconfirmed_txs
        (account, txid, value)
        VALUES (?1, ?2, ?3)
        ON CONFLICT DO NOTHING",
        params![account, txid, amount],
    )?;
    Ok(())
}

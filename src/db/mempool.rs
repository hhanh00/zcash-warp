use anyhow::Result;
use rusqlite::Connection;

use warp_macros::c_export;

#[c_export]
pub fn get_unconfirmed_balance(connection: &Connection, account: u32) -> Result<i64> {
    let balance = connection.query_row(
        "SELECT SUM(value) FROM txs
        WHERE account = ?1 AND height IS NULL",
        [account],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    Ok(balance.unwrap_or_default())
}

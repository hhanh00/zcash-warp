use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _};
use zcash_protocol::consensus::Network;

use crate::account::address::get_diversified_address;
use crate::{data::fb::BackupT, db::account::get_account_info, types::PoolMask};

use crate::{
    coin::COINS,
    ffi::{map_result, map_result_bytes, map_result_string, CResult},
};
use flatbuffers::FlatBufferBuilder;
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

#[no_mangle]
pub extern "C" fn c_check_db_password(coin: u8, password: *mut c_char) -> CResult<u8> {
    let password = unsafe { CStr::from_ptr(password).to_string_lossy() };
    let coin_def = COINS[coin as usize].lock();
    let pool = coin_def.pool.as_ref().expect("No db path set");
    let connection = pool.get().unwrap();
    let _ = connection
        .query_row(&format!("PRAGMA key = '{}'", password), [], |_| Ok(()))
        .optional();
    let c = connection.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| {
        r.get::<_, u32>(0)
    });
    let r = if c.is_ok() { 1 } else { 0 };
    CResult::new(r)
}

#[c_export]
pub fn encrypt_db(connection: &Connection, password: &str, new_db_path: &str) -> Result<()> {
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS encrypted_db KEY ?2"),
        [new_db_path, password],
    )?;
    connection.query_row("SELECT sqlcipher_export('encrypted_db')", [], |_row| Ok(()))?;
    connection.execute("DETACH DATABASE encrypted_db", [])?;
    Ok(())
}

#[c_export]
pub fn create_backup(network: &Network, connection: &Connection, account: u32) -> Result<BackupT> {
    let ai = get_account_info(network, &connection, account)?;
    let backup = ai.to_backup(network);
    Ok(backup)
}

#[c_export]
pub fn get_address(
    network: &Network,
    connection: &Connection,
    account: u32,
    time: u32,
    mask: u8,
) -> Result<String> {
    let address = if mask & 8 != 0 {
        get_diversified_address(network, connection, account, time, PoolMask(mask))
    } else {
        let ai = get_account_info(network, &connection, account)?;
        Ok(ai.to_address(network, PoolMask(mask)))
    }?;
    Ok(address.unwrap_or_default())
}

#[no_mangle]
pub extern "C" fn c_set_db_password(coin: u8, password: *mut c_char) -> CResult<u8> {
    let password = unsafe { CStr::from_ptr(password).to_string_lossy() };
    let mut coin = COINS[coin as usize].lock();
    coin.db_password = Some(password.to_string());
    CResult::new(0)
}

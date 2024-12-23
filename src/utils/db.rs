use crate::db::create_schema;
use crate::network::Network;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _};

use crate::account::address::get_diversified_address;
use crate::{data::fb::BackupT, db::account::get_account_info, types::PoolMask};

use crate::{
    coin::COINS,
    ffi::{map_result, CResult},
};
use std::ffi::{c_char, CStr};

pub fn check_db_password(path: &str, password: &str) -> Result<u8> {
    let connection = Connection::open(path)?;
    let _ = connection
        .query_row(&format!("PRAGMA key = '{}'", password), [], |_| Ok(()))
        .optional();
    let c = connection.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| {
        r.get::<_, u32>(0)
    });
    let r = if c.is_ok() { 1 } else { 0 };
    Ok(r)
}

pub fn encrypt_db(connection: &Connection, password: &str, new_db_path: &str) -> Result<()> {
    connection.execute(
        "ATTACH DATABASE ?1 AS encrypted_db KEY ?2",
        [new_db_path, password],
    )?;
    connection.query_row("SELECT sqlcipher_export('encrypted_db')", [], |_row| Ok(()))?;
    connection.execute("DETACH DATABASE encrypted_db", [])?;
    Ok(())
}

pub fn create_backup(network: &Network, connection: &Connection, account: u32) -> Result<BackupT> {
    let ai = get_account_info(network, &connection, account)?;
    let backup = ai.to_backup(network);
    Ok(backup)
}

pub fn get_address(
    network: &Network,
    connection: &Connection,
    account: u32,
    time: u32,
    mask: u8,
) -> Result<String> {
    let address = if mask & 8 != 0 {
        get_diversified_address(network, connection, account, time, PoolMask(mask))?
    } else {
        let ai = get_account_info(network, &connection, account)?;
        ai.to_address(network, PoolMask(mask))
    };
    Ok(address.unwrap_or_default())
}

#[no_mangle]
pub extern "C" fn c_set_db_path_password(
    coin: u8,
    path: *mut c_char,
    password: *mut c_char,
) -> CResult<u8> {
    let res = || {
        let path = unsafe { CStr::from_ptr(path).to_string_lossy() };
        let password = unsafe { CStr::from_ptr(password).to_string_lossy() };
        let mut coin = COINS[coin as usize].lock();
        coin.set_path_password(&path, &password)?;
        Ok::<_, anyhow::Error>(0)
    };
    map_result(res())
}

#[no_mangle]
pub extern "C" fn c_schema_version() -> u32 {
    2
}

pub fn create_db(path: &str, password: &str, version: &str) -> Result<()> {
    let mut connection = open_with_password(path, password)?;
    create_schema(&mut connection, version)?;
    Ok(())
}

pub fn open_with_password(path: &str, password: &str) -> Result<Connection> {
    let connection = Connection::open(path)?;
    let _ = connection
        .query_row(&format!("PRAGMA key = '{}'", password), [], |_| Ok(()))
        .optional();
    Ok(connection)
}

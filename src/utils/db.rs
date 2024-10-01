use crate::db::reset_tables;
use crate::network::Network;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension as _};

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
pub extern "C" fn c_check_db_password(path: *mut c_char, password: *mut c_char) -> CResult<u8> {
    let res = || {
        let path = unsafe { CStr::from_ptr(path).to_string_lossy() };
        let password = unsafe { CStr::from_ptr(password).to_string_lossy() };
        let connection = Connection::open(&*path)?;
        let _ = connection
            .query_row(&format!("PRAGMA key = '{}'", password), [], |_| Ok(()))
            .optional();
        let c = connection.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| {
            r.get::<_, u32>(0)
        });
        let r = if c.is_ok() { 1 } else { 0 };
        Ok::<_, anyhow::Error>(r)
    };
    map_result(res())
}

#[c_export]
pub fn encrypt_db(connection: &Connection, password: &str, new_db_path: &str) -> Result<()> {
    connection.execute(
        "ATTACH DATABASE ?1 AS encrypted_db KEY ?2",
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
        Some(get_diversified_address(
            network,
            connection,
            account,
            time,
            PoolMask(mask),
        )?)
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

#[c_export]
pub fn create_db(network: &Network, path: &str, password: &str) -> Result<()> {
    let connection = open_with_password(path, password)?;
    reset_tables(network, &connection, false)?;
    Ok(())
}

#[c_export]
pub fn migrate_db(
    network: &Network,
    major: u8,
    src: &str,
    dest: &str,
    password: &str,
) -> Result<()> {
    let dest = open_with_password(dest, password)?;

    dest.execute(
        "ATTACH DATABASE ?1 AS src_db KEY ?2",
        params![src, password],
    )?;

    match major {
        1 => migrate_v1(network, &dest)?,
        _ => anyhow::bail!("Unsupported upgrade"),
    }
    Ok(())
}

pub fn migrate_v1(network: &Network, db: &Connection) -> Result<()> {
    reset_tables(network, db, true)?;
    Ok(())
}

pub fn open_with_password(path: &str, password: &str) -> Result<Connection> {
    let connection = Connection::open(path)?;
    let _ = connection
        .query_row(&format!("PRAGMA key = '{}'", password), [], |_| Ok(()))
        .optional();
    Ok(connection)
}

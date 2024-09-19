use anyhow::Result;
use rusqlite::Connection;
use warp_macros::c_export;
use crate::{coin::COINS, ffi::{map_result, CResult}};

pub mod account;
pub mod account_manager;
pub mod contacts;
pub mod chain;
pub mod notes;
pub mod tx;
pub mod witnesses;
pub mod messages;

#[c_export]
pub fn reset_tables(connection: &Connection) -> Result<()> {
    tracing::info!("Reset Tables");
    // TODO Schema versioning
    // connection.execute("DROP TABLE IF EXISTS props", [])?;
    // connection.execute("DROP TABLE IF EXISTS txs", [])?;
    // connection.execute("DROP TABLE IF EXISTS notes", [])?;
    // connection.execute("DROP TABLE IF EXISTS witnesses", [])?;
    // connection.execute("DROP TABLE IF EXISTS utxos", [])?;
    // connection.execute("DROP TABLE IF EXISTS blcks", [])?;
    // connection.execute("DROP TABLE IF EXISTS txdetails", [])?;
    // connection.execute("DROP TABLE IF EXISTS msgs", [])?;
    // connection.execute("DROP TABLE IF EXISTS contacts", [])?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS props(
        id_prop INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        name TEXT NOT NULL,
        value BLOB NOT NULL,
        UNIQUE (account, name))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS accounts(
        id_account INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        key_type INTEGER NOT NULL,
        fingerprint BLOB NOT NULL UNIQUE,
        seed TEXT,
        aindex INTEGER NOT NULL,
        birth INTEGER NOT NULL,
        balance INTEGER NOT NULL,
        saved BOOL NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS t_accounts(
        account INTEGER PRIMARY KEY,
        addr_index INTEGER NOT NULL,
        sk TEXT,
        address TEXT NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS t_subaccounts(
        id_subaccount INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        addr_index INTEGER NOT NULL,
        sk TEXT,
        address TEXT NOT NULL,
        UNIQUE (account, addr_index))",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS s_accounts(
        account INTEGER PRIMARY KEY,
        sk TEXT,
        vk TEXT NOT NULL,
        address TEXT NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS o_accounts(
        account INTEGER PRIMARY KEY,
        sk BLOB,
        vk BLOB NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS blcks(
        height INTEGER PRIMARY KEY,
        hash BLOB NOT NULL,
        prev_hash BLOB NOT NULL,
        timestamp INTEGER NOT NULL)",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS txs(
        id_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        txid BLOB NOT NULL,
        height INTEGER NOT NULL,
        timestamp INTEGER NOT NULL,
        value INTEGER NOT NULL,
        address TEXT,
        memo TEXT,
        UNIQUE (account, txid))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS notes(
        id_note INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        position INTEGER NOT NULL,
        height INTEGER NOT NULL,
        tx INTEGER NULL,
        output_index INTEGER NOT NULL,
        address BLOB NOT NULL,
        value INTEGER NOT NULL,
        rcm BLOB NOT NULL,
        nf BLOB NOT NULL UNIQUE,
        rho BLOB,
        spent INTEGER,
        orchard BOOL NOT NULL,
        excluded BOOL NOT NULL,
        UNIQUE (position, orchard))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS witnesses(
        id_witness INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        note INTEGER NOT NULL,
        height INTEGER NOT NULL,
        witness BLOB NOT NULL)",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS utxos(
        id_utxo INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        addr_index INTEGER NOT NULL,
        height INTEGER NOT NULL,
        timestamp INTEGER NOT NULL,
        txid BLOB NOT NULL,
        vout INTEGER NULL,
        value INTEGER NOT NULL,
        spent INTEGER,
        UNIQUE (txid, vout))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS txdetails(
        id_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        txid BLOB NOT NULL UNIQUE,
        data BLOB NOT NULL)",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS msgs(
        id_msg INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        timestamp INTEGER NOT NULL,
        txid BLOB NOT NULL,
        nout INTEGER NOT NULL,
        incoming BOOL NOT NULL,
        sender TEXT,
        recipient TEXT NOT NULL,
        subject TEXT NOT NULL,
        body TEXT NOT NULL,
        read BOOL NOT NULL,
        UNIQUE (txid, nout))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS contacts(
        id_contact INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        name TEXT NOT NULL,
        address TEXT NOT NULL,
        saved BOOL NOT NULL,
        UNIQUE (account, name))",
        [],
    )?;

    Ok(())
}

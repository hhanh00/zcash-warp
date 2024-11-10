use anyhow::Result;
use rusqlite::Connection;
use warp_macros::c_export;

use crate::utils::ContextExt;

pub mod account;
pub mod account_manager;
pub mod chain;
pub mod contacts;
pub mod mempool;
pub mod messages;
pub mod notes;
pub mod swap;
pub mod tx;
pub mod witnesses;

#[c_export]
pub fn create_schema(connection: &mut Connection, _version: &str) -> Result<()> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS props(
        id_prop INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        name TEXT NOT NULL,
        value BLOB NOT NULL,
        UNIQUE (account, name))",
            [],
        )
        .with_file_line(|| "props")?;
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS accounts(
        id_account INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        position INTEGER NOT NULL,
        seed TEXT,
        aindex INTEGER NOT NULL,
        dindex INTEGER NOT NULL,
        birth INTEGER NOT NULL,
        icon BLOB,
        balance INTEGER NOT NULL,
        saved BOOL NOT NULL,
        hidden BOOL NOT NULL)",
            [],
        )
        .with_file_line(|| "accounts")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS t_accounts(
        account INTEGER PRIMARY KEY,
        xsk BLOB,
        sk TEXT,
        vk BLOB,
        address TEXT NOT NULL)",
            [],
        )
        .with_file_line(|| "t_accounts")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS t_addresses(
        id_address INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        sk TEXT,
        external INTEGER NOT NULL,
        addr_index INTEGER NOT NULL,
        address TEXT NOT NULL,
        UNIQUE (account, external, addr_index))",
            [],
        )
        .with_file_line(|| "t_addresses")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS s_accounts(
        account INTEGER PRIMARY KEY,
        sk TEXT,
        vk TEXT NOT NULL,
        address TEXT NOT NULL)",
            [],
        )
        .with_file_line(|| "s_accounts")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS o_accounts(
        account INTEGER PRIMARY KEY,
        sk BLOB,
        vk BLOB NOT NULL)",
            [],
        )
        .with_file_line(|| "o_accounts")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS blcks(
        height INTEGER PRIMARY KEY,
        hash BLOB NOT NULL,
        prev_hash BLOB NOT NULL,
        timestamp INTEGER NOT NULL)",
            [],
        )
        .with_file_line(|| "blcks")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS txs(
        id_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        txid BLOB NOT NULL,
        height INTEGER NOT NULL,
        timestamp INTEGER NOT NULL,
        value INTEGER NOT NULL,
        address TEXT,
        receiver BLOB,
        memo TEXT,
        expiration INTEGER,
        UNIQUE (account, txid))",
            [],
        )
        .with_file_line(|| "txs")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS mempool_txs(
        id_mempool_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        txid BLOB NOT NULL,
        value INTEGER NOT NULL,
        UNIQUE (account, txid))",
            [],
        )
        .with_file_line(|| "mempool_txs")?;

    connection
        .execute(
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
        nf BLOB NOT NULL,
        rho BLOB,
        spent INTEGER,
        expiration INTEGER,
        orchard BOOL NOT NULL,
        excluded BOOL NOT NULL,
        UNIQUE (account, position, orchard),
        UNIQUE (account, nf))",
            [],
        )
        .with_file_line(|| "notes")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS note_spends(
        id_note INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        id_tx INTEGER NOT NULL)",
            [],
        )
        .with_file_line(|| "note_spends")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS witnesses(
        id_witness INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        note INTEGER NOT NULL,
        height INTEGER NOT NULL,
        witness BLOB NOT NULL,
        UNIQUE (account, note, height))",
            [],
        )
        .with_file_line(|| "witnesses")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS utxos(
        id_utxo INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        external INTEGER NOT NULL,
        addr_index INTEGER NOT NULL,
        height INTEGER NOT NULL,
        timestamp INTEGER NOT NULL,
        txid BLOB NOT NULL,
        vout INTEGER NULL,
        value INTEGER NOT NULL,
        spent INTEGER,
        expiration INTEGER,
        UNIQUE (account, txid, vout))",
            [],
        )
        .with_file_line(|| "utxos")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS utxo_spends(
        id_utxo INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        id_tx INTEGER NOT NULL)",
            [],
        )
        .with_file_line(|| "utxo_spends")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS txdetails(
        id_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        txid BLOB NOT NULL,
        data BLOB NOT NULL,
        UNIQUE (account, txid))",
            [],
        )
        .with_file_line(|| "txdetails")?;

    connection
        .execute(
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
        receiver BLOB,
        subject TEXT NOT NULL,
        body TEXT NOT NULL,
        read BOOL NOT NULL,
        UNIQUE (account, txid, nout))",
            [],
        )
        .with_file_line(|| "msgs")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS contacts(
        id_contact INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        name TEXT NOT NULL,
        address TEXT NOT NULL,
        saved BOOL NOT NULL,
        UNIQUE (account, name))",
            [],
        )
        .with_file_line(|| "contacts")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS contact_receivers(
        id_contact_receiver INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        contact INTEGER NOT NULL,
        pool INTEGER NOT NULL,
        address BLOB NOT NULL,
        UNIQUE (account, contact, pool))",
            [],
        )
        .with_file_line(|| "contact_receivers")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS blck_times(
        height INTEGER PRIMARY KEY,
        timestamp INTEGER NOT NULL)",
            [],
        )
        .with_file_line(|| "blck_times")?;

    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS swaps(
            id_swap INTEGER NOT NULL PRIMARY KEY,
            account INTEGER NOT NULL,
            provider TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            timestamp INTEGER,
            from_currency TEXT NOT NULL,
            from_amount TEXT NOT NULL,
            from_address TEXT NOT NULL,
            from_image TEXT NOT NULL,
            to_currency TEXT NOT NULL,
            to_amount TEXT NOT NULL,
            to_address TEXT NOT NULL,
            to_image TEXT NOT NULL
        )",
            [],
        )
        .with_file_line(|| "swaps")?;

    Ok(())
}

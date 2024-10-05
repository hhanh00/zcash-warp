use crate::{
    coin::COINS,
    ffi::{map_result, CResult},
    network::Network,
};
use account_manager::create_new_account;
use anyhow::Result;
use rusqlite::Connection;
use warp_macros::c_export;
use zcash_protocol::consensus::{NetworkUpgrade, Parameters};

pub mod account;
pub mod account_manager;
pub mod chain;
pub mod contacts;
pub mod messages;
pub mod notes;
pub mod tx;
pub mod witnesses;

#[c_export]
pub fn reset_tables(network: &Network, connection: &Connection, upgrade: bool) -> Result<bool> {
    tracing::info!("Reset Tables");

    connection.execute(
        "CREATE TABLE IF NOT EXISTS schema_version(
        id INTEGER NOT NULL PRIMARY KEY,
        version INTEGER NOT NULL)",
        [],
    )?;
    connection.execute(
        "INSERT INTO schema_version(id, version)
    VALUES (0, 0) ON CONFLICT DO NOTHING",
        [],
    )?;

    let minor =
        connection.query_row("SELECT version FROM schema_version WHERE id = 0", [], |r| {
            r.get::<_, u8>(0)
        })? as usize;

    let migrations = vec![migrate_v1];
    let res = if minor < migrations.len() {
        migrations[minor](network, connection, upgrade)?;
        true
    } else {
        false
    };
    Ok(res)
}

fn migrate_v1(network: &Network, connection: &Connection, upgrade: bool) -> Result<()> {
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
        seed TEXT,
        aindex INTEGER NOT NULL,
        dindex INTEGER NOT NULL,
        birth INTEGER NOT NULL,
        balance INTEGER NOT NULL,
        saved BOOL NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS t_accounts(
        account INTEGER PRIMARY KEY,
        xsk BLOB,
        sk TEXT,
        vk BLOB,
        addr_index INTEGER NOT NULL,
        address TEXT NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS t_addresses(
        id_address INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        sk TEXT,
        addr_index INTEGER NOT NULL,
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
        receiver BLOB,
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
        nf BLOB NOT NULL,
        rho BLOB,
        spent INTEGER,
        orchard BOOL NOT NULL,
        excluded BOOL NOT NULL,
        UNIQUE (account, position, orchard),
        UNIQUE (account, nf))",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS note_spends(
        id_note INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        id_tx INTEGER NOT NULL)",
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
        UNIQUE (account, txid, vout))",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS utxo_spends(
        id_utxo INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        id_tx INTEGER NOT NULL)",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS txdetails(
        id_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        txid BLOB NOT NULL,
        data BLOB NOT NULL,
        UNIQUE (account, txid))",
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
        receiver BLOB,
        subject TEXT NOT NULL,
        body TEXT NOT NULL,
        read BOOL NOT NULL,
        UNIQUE (account, txid, nout))",
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

    connection.execute(
        "CREATE TABLE IF NOT EXISTS contact_receivers(
        id_contact_receiver INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        contact INTEGER NOT NULL,
        pool INTEGER NOT NULL,
        address BLOB NOT NULL,
        UNIQUE (account, contact, pool))",
        [],
    )?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS blck_times(
        height INTEGER PRIMARY KEY,
        timestamp INTEGER NOT NULL)",
        [],
    )?;

    if upgrade {
        let mut s = connection
            .prepare("SELECT a.name, a.seed, a.aindex, a.sk, a.ivk FROM src_db.accounts a")?;
        let rows = s.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let birth: u32 = network
            .activation_height(NetworkUpgrade::Sapling)
            .unwrap()
            .into();
        for r in rows {
            let (name, seed, aindex, sk, ivk) = r?;
            let key = seed.clone().or(sk.clone()).unwrap_or(ivk.clone());
            create_new_account(network, connection, &name, &key, aindex, birth)?;
        }
    }

    Ok(())
}

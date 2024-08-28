use anyhow::Result;
use rusqlite::Connection;

pub(crate) mod account;
pub(crate) mod account_manager;
pub(crate) mod contacts;
pub(crate) mod migration;
pub(crate) mod notes;
pub(crate) mod tx;
pub(crate) mod witnesses;

// pub use account::{get_account_info, get_balance, list_accounts};
// pub use account_manager::{create_new_account, delete_account, detect_key, parse_seed_phrase};
// pub use migration::init_db;
// pub use notes::{
//     add_tx_value, get_block_header, get_note_by_nf, get_sync_height, get_txid, list_received_notes,
//     list_utxos, mark_shielded_spent, mark_transparent_spent, reset_scan, rewind_checkpoint,
//     store_block, store_received_note, store_tx, store_tx_details, store_utxo, truncate_scan,
//     update_tx_timestamp,
// };
// pub use witnesses::get_witnesses_v1;
// pub use tx::{list_new_txids, get_tx_details};

pub fn reset_tables(connection: &Connection) -> Result<()> {
    connection.execute("DROP TABLE IF EXISTS txs", [])?;
    connection.execute("DROP TABLE IF EXISTS notes", [])?;
    connection.execute("DROP TABLE IF EXISTS witnesses", [])?;
    connection.execute("DROP TABLE IF EXISTS utxos", [])?;
    connection.execute("DROP TABLE IF EXISTS blcks", [])?;
    connection.execute("DROP TABLE IF EXISTS txdetails", [])?;
    connection.execute("DROP TABLE IF EXISTS msgs", [])?;
    connection.execute("DROP TABLE IF EXISTS contacts", [])?;

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
        diversifier BLOB NOT NULL,
        value INTEGER NOT NULL,
        rcm BLOB NOT NULL,
        nf BLOB NOT NULL UNIQUE,
        rho BLOB,
        spent INTEGER,
        orchard BOOL NOT NULL,
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
        height INTEGER NOT NULL,
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
        dirty BOOL NOT NULL,
        UNIQUE (account, name))",
        [],
    )?;

    Ok(())
}

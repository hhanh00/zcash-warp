use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension as _, Transaction};
use zcash_protocol::consensus::{NetworkUpgrade, Parameters as _};

use crate::db::notes::update_account_balances;
use crate::network::Network;
use crate::types::CheckpointHeight;
use crate::utils::chain::reset_chain;
use crate::{data::fb::CheckpointT, warp::BlockHeader};
use crate::{Client, Hash};

use crate::{
    coin::COINS,
    ffi::{map_result, map_result_bytes, CResult},
};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use warp_macros::c_export;

pub fn snap_to_checkpoint(connection: &Connection, height: u32) -> Result<CheckpointHeight> {
    let height = connection.query_row(
        "SELECT MAX(height) FROM blcks WHERE height <= ?1",
        [height],
        |r| r.get::<_, Option<u32>>(0),
    )?;
    let height = height.ok_or(anyhow::anyhow!("No suitable checkpoint"))?;
    Ok(CheckpointHeight(height))
}

pub fn get_block_header(connection: &Connection, height: u32) -> Result<BlockHeader> {
    let (hash, prev_hash, timestamp) = connection.query_row(
        "SELECT hash, prev_hash, timestamp FROM blcks WHERE height = ?1",
        [height],
        |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, u32>(2)?,
            ))
        },
    )?;
    Ok(BlockHeader {
        height,
        hash: hash.try_into().unwrap(),
        prev_hash: prev_hash.try_into().unwrap(),
        timestamp,
    })
}

pub fn store_block(connection: &Transaction, bh: &BlockHeader) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO blcks
        (height, hash, prev_hash, timestamp) VALUES (?1, ?2, ?3, ?4)",
    )?;
    s.execute(params![bh.height, bh.hash, bh.prev_hash, bh.timestamp,])?;
    Ok(())
}

#[c_export]
pub fn get_sync_height(connection: &Connection) -> Result<u32> {
    let height = connection.query_row("SELECT MAX(height) FROM blcks", [], |r| {
        r.get::<_, Option<u32>>(0)
    })?;
    Ok(height.unwrap_or_default())
}

pub fn truncate_scan(connection: &Connection) -> Result<()> {
    connection.execute("DELETE FROM blcks", [])?;
    connection.execute("DELETE FROM blck_times", [])?;
    connection.execute("DELETE FROM txs", [])?;
    connection.execute("DELETE FROM txdetails", [])?;
    connection.execute("DELETE FROM notes", [])?;
    connection.execute("DELETE FROM note_spends", [])?;
    connection.execute("DELETE FROM witnesses", [])?;
    connection.execute("DELETE FROM utxos", [])?;
    connection.execute("DELETE FROM utxo_spends", [])?;
    connection.execute("DELETE FROM contacts", [])?;
    connection.execute("DELETE FROM msgs", [])?;

    Ok(())
}

pub fn reset_scan(
    network: &Network,
    connection: &mut Connection,
    height: Option<u32>,
) -> Result<u32> {
    let activation: u32 = network
        .activation_height(NetworkUpgrade::Sapling)
        .unwrap()
        .into();
    let height = height.unwrap_or(activation + 1) - 1;

    let db_tx = connection.transaction()?;
    db_tx.execute("DELETE FROM blcks WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM blck_times WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM txs WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM txdetails", [])?;
    db_tx.execute("DELETE FROM notes WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM note_spends WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM witnesses WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM utxos WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM utxo_spends WHERE height >= ?1", [height])?;
    db_tx.execute("DELETE FROM msgs", [])?;
    db_tx.execute("UPDATE notes SET spent = NULL WHERE spent >= ?1", [height])?;
    db_tx.execute("UPDATE notes SET expiration = NULL", [])?;
    db_tx.execute("UPDATE utxos SET spent = NULL WHERE spent >= ?1", [height])?;
    db_tx.execute("UPDATE utxos SET expiration = NULL", [])?;
    update_account_balances(&db_tx, height)?;
    db_tx.commit()?;

    Ok(height)
}

pub async fn rewind_checkpoint(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
) -> Result<()> {
    let checkpoint = get_sync_height(connection)?;
    if checkpoint > 0 {
        rewind(network, connection, client, checkpoint - 1).await?;
    }
    Ok(())
}

#[c_export]
pub async fn rewind(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
    height: u32,
) -> Result<()> {
    let height = connection
        .query_row(
            "SELECT height FROM blcks WHERE height <= ?1 ORDER BY height DESC LIMIT 1",
            [height],
            |r| r.get::<_, u32>(0),
        )
        .optional()?;
    if let Some(height) = height {
        let db_tx = connection.transaction()?;
        tracing::info!("Dropping sync data after @{height}");
        db_tx.execute("DELETE FROM blcks WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM blck_times WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM txs WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM notes WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM note_spends WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM witnesses WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM utxos WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM utxo_spends WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM txdetails WHERE height > ?1", [height])?;
        db_tx.execute("DELETE FROM msgs WHERE height > ?1", [height])?;
        db_tx.execute("UPDATE notes SET spent = NULL WHERE spent > ?1", [height])?;
        db_tx.execute("UPDATE notes SET expiration = NULL", [])?;
        db_tx.execute("UPDATE utxos SET spent = NULL WHERE spent > ?1", [height])?;
        db_tx.execute("UPDATE utxos SET expiration = NULL", [])?;
        update_account_balances(&db_tx, height)?;
        db_tx.commit()?;
    } else {
        reset_chain(network, connection, client, 0).await?;
    }

    Ok(())
}

#[c_export]
pub fn list_checkpoints(connection: &Connection) -> Result<Vec<CheckpointT>> {
    let mut s = connection.prepare("SELECT height, hash, timestamp FROM blcks ORDER BY height")?;
    let rows = s.query_map([], |r| -> Result<(u32, Hash, u32), _> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    let mut checkpoints = vec![];
    for r in rows {
        let (height, hash, timestamp) = r?;
        checkpoints.push(CheckpointT {
            height,
            hash: Some(hash.to_vec()),
            timestamp,
        })
    }
    Ok(checkpoints)
}

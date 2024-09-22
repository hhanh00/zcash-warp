use anyhow::Result;
use rusqlite::{Connection, DropBehavior};
use zcash_protocol::consensus::{NetworkUpgrade, Parameters};

use crate::{
    db::{
        account_manager::get_min_birth,
        chain::{store_block, truncate_scan},
    }, lwd::{get_compact_block, get_last_height}, network::Network, warp::BlockHeader, Client
};

use warp_macros::c_export;
use crate::{coin::COINS, ffi::{map_result, CResult}};

#[c_export]
pub async fn get_activation_date(network: &Network, client: &mut Client) -> Result<u32> {
    let height = network.activation_height(NetworkUpgrade::Sapling).unwrap();
    let cb = get_compact_block(client, height.into()).await?;
    let timestamp = cb.time;
    Ok(timestamp)
}

const SEC_PER_DAY: u32 = 24 * 60 * 60;

#[c_export]
pub async fn get_height_by_time(network: &Network, client: &mut Client, time: u32) -> Result<u32> {
    let time = time / SEC_PER_DAY;
    let mut s: u32 = network
        .activation_height(NetworkUpgrade::Sapling)
        .unwrap()
        .into();
    let mut e = get_last_height(client).await?;
    while s <= e {
        let m = (s + e) / 2;
        let cp = get_compact_block(client, m).await?;
        let block_time = cp.time / SEC_PER_DAY;
        match time.cmp(&block_time) {
            std::cmp::Ordering::Less => {
                e = m - 1;
            }
            std::cmp::Ordering::Equal => {
                return Ok(m);
            }
            std::cmp::Ordering::Greater => {
                s = m + 1;
            }
        }
    }
    Ok(s)
}

#[c_export]
pub fn get_activation_height(network: &Network) -> Result<u32> {
    let h = network.activation_height(NetworkUpgrade::Sapling).unwrap();
    let h: u32 = h.into();
    Ok(h)
}

#[c_export]
pub async fn get_time_by_height(network: &Network, client: &mut Client, height: u32) -> Result<u32> {
    let ah = network.activation_height(NetworkUpgrade::Sapling).unwrap();
    let height = height.max(u32::from(ah) + 1);

    let block = get_compact_block(client, height).await?;
    Ok(block.time)
}

#[c_export]
pub async fn reset_chain(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
    height: u32,
) -> Result<()> {
    let height = if height == 0 { None } else { Some(height) };
    truncate_scan(connection)?;
    let activation: u32 = network
        .activation_height(NetworkUpgrade::Sapling)
        .unwrap()
        .into();
    let min_birth_height = get_min_birth(&connection)?.unwrap_or(activation);
    let height = height.unwrap_or(min_birth_height).max(activation + 1);
    let block = get_compact_block(client, height).await?;
    let mut transaction = connection.transaction()?;
    transaction.set_drop_behavior(DropBehavior::Commit);
    store_block(&transaction, &BlockHeader::from(&block))?;
    Ok(())
}

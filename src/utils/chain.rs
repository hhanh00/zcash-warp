use anyhow::Result;
use zcash_protocol::consensus::{Network, NetworkUpgrade, Parameters};

use crate::{
    lwd::{get_compact_block, get_last_height},
    Client,
};

pub async fn get_activation_date(network: &Network, client: &mut Client) -> Result<u32> {
    let height = network.activation_height(NetworkUpgrade::Sapling).unwrap();
    let cb = get_compact_block(client, height.into()).await?;
    let timestamp = cb.time;
    Ok(timestamp)
}

const SEC_PER_DAY: u32 = 24 * 60 * 60;

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
            std::cmp::Ordering::Equal => { return Ok(m); }
            std::cmp::Ordering::Greater => {
                s = m + 1;
            }
        }
    }
    unreachable!()
}

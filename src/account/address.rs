use anyhow::Result;
use orchard::keys::Scope;
use rusqlite::Connection;
use zcash_primitives::consensus::Network;

use crate::{db::get_account_info, types::PoolMask};

pub fn get_diversified_address(
    network: &Network,
    connection: &Connection,
    account: u32,
    time: u32,
    pools: PoolMask,
) -> Result<String> {
    let ai = get_account_info(network, connection, account)?;
    let ai = ai.select_pools(pools);
    let saddr = ai
        .sapling
        .as_ref()
        .map(|si| {
            let mut di = [0u8; 11];
            di[4..8].copy_from_slice(&time.to_le_bytes());
            let di = zcash_primitives::zip32::DiversifierIndex(di);
            let (_, saddr) = si
                .vk
                .find_address(di)
                .ok_or(anyhow::anyhow!("No diversifier address found"))?;
            Ok::<_, anyhow::Error>(saddr)
        })
        .transpose()?;
    let oaddr = ai.orchard.as_ref().map(|oi| {
        let di = orchard::keys::DiversifierIndex::from(time);
        oi.vk.address_at(di, Scope::External)
    });
    let ua = zcash_client_backend::address::UnifiedAddress::from_receivers(oaddr, saddr, None)
        .ok_or(anyhow::anyhow!("Cannot build UA"))?;
    let address = ua.encode(network);
    Ok(address)
}

pub fn convert_tex_address(network: &Network, address: &str, to_tex: bool) -> Result<String> {
    todo!()
}

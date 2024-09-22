use anyhow::Result;
use bech32::{Bech32m, Hrp};
use orchard::keys::Scope;
use rusqlite::Connection;
use zcash_client_backend::encoding::AddressCodec;
use zcash_primitives::legacy::TransparentAddress;

use crate::{db::account::get_account_info, network::Network, types::PoolMask};

pub fn get_diversified_address(
    network: &Network,
    connection: &Connection,
    account: u32,
    time: u32,
    pools: PoolMask,
) -> Result<Option<String>> {
    let ai = get_account_info(network, connection, account)?;
    let ai = ai.select_pools(pools);
    let saddr = ai
        .sapling
        .as_ref()
        .map(|si| {
            let mut di = [0u8; 11];
            di[4..8].copy_from_slice(&time.to_le_bytes());
            let di = zcash_primitives::zip32::DiversifierIndex::from(di);
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
    let ua = zcash_client_backend::address::UnifiedAddress::from_receivers(oaddr, saddr, None);
    let address = ua.map(|ua| ua.encode(network));
    Ok(address)
}

const TEX_HRP: Hrp = Hrp::parse_unchecked("tex");

pub fn convert_tex_address(network: &Network, address: &str, to_tex: bool) -> Result<String> {
    if to_tex {
        let taddr = TransparentAddress::decode(network, address)?;
        if let TransparentAddress::PublicKeyHash(pkh) = taddr {
            let tex = bech32::encode::<Bech32m>(TEX_HRP, &pkh)?;
            Ok(tex)
        } else {
            anyhow::bail!("Not a PKH Transparent Address");
        }
    } else {
        let (hrp, data) = bech32::decode(address)?;
        if hrp != TEX_HRP {
            anyhow::bail!("Not a TEX address")
        }
        if data.len() != 20 {
            anyhow::bail!("Not a TEX address")
        }
        let pkh: [u8; 20] = data.try_into().unwrap();
        let taddr = TransparentAddress::PublicKeyHash(pkh);
        let address = taddr.encode(network);
        Ok(address)
    }
}

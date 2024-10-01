use anyhow::Result;
use bech32::{Bech32m, Hrp};
use rusqlite::Connection;
use zcash_client_backend::encoding::AddressCodec;
use zcash_keys::keys::UnifiedAddressRequest;
use zcash_primitives::legacy::TransparentAddress;
use zip32::DiversifierIndex;

use crate::{
    db::account::get_account_info,
    network::Network,
    types::{PoolMask, TransparentAccountInfo},
};

pub fn get_diversified_address(
    network: &Network,
    connection: &Connection,
    account: u32,
    addr_index: u32,
    pools: PoolMask,
) -> Result<String> {
    let ai = get_account_info(network, connection, account)?;
    let ai = ai.select_pools(pools);
    let pool_mask = ai.to_mask();
    let address = match pool_mask {
        0 => anyhow::bail!("No Receiver"),
        1 => {
            let tvk = ai.transparent.as_ref().and_then(|ti| ti.vk.as_ref());
            let address = tvk
                .map(|tvk| TransparentAccountInfo::derive_address(tvk, addr_index).encode(network));
            address.ok_or(anyhow::anyhow!("No Transparent Address"))?
        }
        _ => {
            let uvk = ai.to_vk()?;
            let di: DiversifierIndex = addr_index.try_into().unwrap();
            let ua_request = UnifiedAddressRequest::new(
                ai.orchard.is_some(),
                ai.sapling.is_some(),
                ai.transparent.is_some(),
            )
            .ok_or(anyhow::anyhow!("Must have shielded receiver"))?;
            let (address, _) = uvk.find_address(di, ua_request)?;
            let address = address.encode(network);
            address
        }
    };
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

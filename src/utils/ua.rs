use anyhow::Result;
use orchard::Address;
use zcash_keys::{
    address::{Address as RecipientAddress, UnifiedAddress},
    encoding::AddressCodec,
};
use zcash_primitives::legacy::TransparentAddress;
use zcash_protocol::consensus::Network;

use crate::{data::fb::UAReceiversT, types::PoolMask};

use crate::{
    coin::COINS,
    ffi::{map_result_bytes, CResult},
};
use std::ffi::{c_char, CStr};
use warp_macros::c_export;
use flatbuffers::FlatBufferBuilder;

#[c_export]
pub fn decode_address(network: &Network, address: &str) -> Result<UAReceiversT> {
    let ua = RecipientAddress::decode(network, address).ok_or(anyhow::anyhow!("Invalid UA"))?;
    let ua = match ua {
        RecipientAddress::Unified(ua) => {
            let t = ua.transparent().map(|t| t.encode(network));
            let s = ua.sapling().map(|s| s.encode(network));
            let o = ua
                .orchard()
                .map(|o| ua_of_orchard(&o.to_raw_address_bytes()).encode(network));
            UAReceiversT {
                tex: false,
                transparent: t,
                sapling: s,
                orchard: o,
            }
        }
        RecipientAddress::Sapling(s) => UAReceiversT {
            tex: false,
            transparent: None,
            sapling: Some(s.encode(network)),
            orchard: None,
        },
        RecipientAddress::Transparent(t) => UAReceiversT {
            tex: false,
            transparent: Some(t.encode(network)),
            sapling: None,
            orchard: None,
        },
        RecipientAddress::Tex(pkh) => UAReceiversT {
            tex: true,
            transparent: Some(TransparentAddress::PublicKeyHash(pkh).encode(network)),
            sapling: None,
            orchard: None,
        },
    };
    Ok(ua)
}

pub fn single_receiver_address(
    network: &Network,
    address: &str,
    pools: PoolMask,
) -> Result<Option<String>> {
    if !pools.single_pool() {
        anyhow::bail!("Not a single receiver in {pools:?}");
    }
    let pool = pools.to_pool().ok_or(anyhow::anyhow!("No pool"))?;
    let r = RecipientAddress::decode(network, address)
        .ok_or(anyhow::anyhow!("Cannot parse address {address}"))?;
    let address = match r {
        RecipientAddress::Tex(_) | RecipientAddress::Transparent(_) if pool == 0 => {
            Some(address.to_string())
        }
        RecipientAddress::Sapling(_) if pool == 1 => Some(address.to_string()),
        RecipientAddress::Unified(ua) => match pool {
            0 => ua.transparent().map(|t| t.encode(network)),
            1 => ua.sapling().map(|s| s.encode(network)),
            2 => ua
                .orchard()
                .map(|o| ua_of_orchard(&o.to_raw_address_bytes()).encode(network)),
            _ => unreachable!(),
        },
        _ => None,
    };
    Ok(address)
}

pub fn ua_of_orchard(address: &[u8; 43]) -> UnifiedAddress {
    let orchard = Address::from_raw_address_bytes(address).unwrap();
    let ua =
        zcash_client_backend::address::UnifiedAddress::from_receivers(Some(orchard), None, None)
            .unwrap();
    ua
}

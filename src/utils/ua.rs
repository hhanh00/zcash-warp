use crate::network::Network;
use anyhow::Result;
use orchard::Address;
use sapling_crypto::PaymentAddress;
use zcash_keys::{
    address::{Address as RecipientAddress, UnifiedAddress},
    encoding::AddressCodec,
};
use zcash_primitives::legacy::TransparentAddress;

use crate::{data::UAReceiversT, types::PoolMask};

pub fn split_address(
    network: &Network,
    address: &str,
) -> Result<(
    Option<TransparentAddress>,
    Option<PaymentAddress>,
    Option<Address>,
    bool,
)> {
    let address: RecipientAddress =
        RecipientAddress::decode(network, address).ok_or(anyhow::anyhow!("Invalid UA"))?;
    let receivers = match address {
        RecipientAddress::Unified(ua) => {
            let t = ua.transparent().cloned();
            let s = ua.sapling().cloned();
            let o = ua.orchard().cloned();
            (t, s, o, false)
        }
        RecipientAddress::Sapling(s) => (None, Some(s), None, false),
        RecipientAddress::Transparent(t) => (Some(t), None, None, false),
        RecipientAddress::Tex(pkh) => (
            Some(TransparentAddress::PublicKeyHash(pkh)),
            None,
            None,
            true,
        ),
    };
    Ok(receivers)
}

pub fn decode_address(network: &Network, address: &str) -> Result<UAReceiversT> {
    let (t, s, o, tex) = split_address(network, address)?;
    let ua = UAReceiversT {
        tex,
        transparent: t.map(|t| t.encode(network)).unwrap_or_default(),
        sapling: s.map(|s| s.encode(network)).unwrap_or_default(),
        orchard: o.map(|o| ua_of_orchard(&o).encode(network)).unwrap_or_default(),
    };
    Ok(ua)
}

pub fn filter_address(network: &Network, address: &str, pool_mask: u8) -> Result<String> {
    let (t, s, o, _) = split_address(network, address)?;
    let t = t.filter(|_| pool_mask & 1 != 0);
    let s = s.filter(|_| pool_mask & 2 != 0);
    let o = o.filter(|_| pool_mask & 4 != 0);

    let addr = match (t, s, o) {
        (Some(t), None, None) => t.encode(network),
        (None, Some(s), None) => s.encode(network),
        _ => {
            let ua = UnifiedAddress::from_receivers(o, s, t);
            ua.map(|ua| ua.encode(network)).unwrap()
        }
    };
    Ok(addr)
}

pub fn single_receiver_address(
    network: &Network,
    address: &str,
    pools: PoolMask,
) -> Result<String> {
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
            2 => ua.orchard().map(|o| ua_of_orchard(o).encode(network)),
            _ => unreachable!(),
        },
        _ => None,
    }
    .ok_or(anyhow::anyhow!(
        "Address has no matching receiver: {address} {pools:?}"
    ))?;

    Ok(address)
}

pub fn ua_of_orchard(orchard: &Address) -> UnifiedAddress {
    let ua = zcash_client_backend::address::UnifiedAddress::from_receivers(
        Some(orchard.clone()),
        None,
        None,
    )
    .unwrap();
    ua
}

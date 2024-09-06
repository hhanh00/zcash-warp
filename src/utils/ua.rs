use anyhow::Result;
use zcash_keys::{address::Address as RecipientAddress, encoding::AddressCodec};
use zcash_protocol::consensus::Network;

use crate::{account::contacts::ua_of_orchard, data::fb::UAReceiversT};

pub fn decode_ua(network: &Network, ua: &str) -> Result<UAReceiversT> {
    let ua = RecipientAddress::decode(network, ua).ok_or(anyhow::anyhow!("Invalid UA"))?;
    let ua = if let RecipientAddress::Unified(ua) = ua {
        let t = ua.transparent().map(|t| t.encode(network));
        let s = ua.sapling().map(|s| s.encode(network));
        let o = ua.orchard().map(|o| ua_of_orchard(&o.to_raw_address_bytes()).encode(network));
        UAReceiversT {
            transparent: t,
            sapling: s,
            orchard: o,
        }
    } else {
        anyhow::bail!("Not a UA")
    };
    Ok(ua)
}

use anyhow::Result;
use rusqlite::Connection;
use sapling_crypto::zip32::DiversifiableFullViewingKey;
use zcash_keys::encoding::{
    encode_extended_spending_key, encode_payment_address, AddressCodec as _,
};
use zcash_primitives::legacy::keys::NonHardenedChildIndex;
use zcash_protocol::consensus::NetworkConstants;
use zip32::DiversifierIndex;

use crate::{
    coin::COINS,
    ffi::{map_result_bytes, CResult},
};
use crate::{
    data::fb::ZIP32KeysT,
    db::account::get_account_info,
    keys::{export_sk_bip38, AccountKeys},
    network::Network,
    types::TransparentAccountInfo,
};
use flatbuffers::FlatBufferBuilder;
use warp_macros::c_export;

#[c_export]
pub fn derive_zip32_keys(
    network: &Network,
    connection: &Connection,
    account: u32,
    acc_index: u32,
    addr_index: u32,
    use_default: bool,
) -> Result<ZIP32KeysT> {
    let ai = get_account_info(network, connection, account)?;
    let Some(phrase) = ai.seed else {
        anyhow::bail!("No Seed")
    };
    let ak = AccountKeys::from_seed(network, &phrase, acc_index)?;
    let tsk = ak.txsk.as_ref().map(|txsk| {
        let address_index = NonHardenedChildIndex::from_index(addr_index).unwrap();
        let sk = txsk.derive_external_secret_key(address_index).unwrap();
        export_sk_bip38(&sk)
    });
    let taddress = ak
        .tvk
        .as_ref()
        .map(|tvk| TransparentAccountInfo::derive_address(tvk, addr_index).encode(network));
    let zip32 = ZIP32KeysT {
        aindex: acc_index,
        addr_index,
        tsk,
        taddress,
        zsk: ak.ssk.as_ref().map(|sk| {
            encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), &sk)
        }),
        zaddress: ak.svk.as_ref().and_then(|vk| {
            if use_default {
                let pa = vk.default_address().1;
                Some(encode_payment_address(
                    network.hrp_sapling_payment_address(),
                    &pa,
                ))
            } else {
                let di = DiversifierIndex::try_from(addr_index).unwrap();
                vk.address(di)
                    .map(|pa| encode_payment_address(network.hrp_sapling_payment_address(), &pa))
            }
        }),
    };
    Ok(zip32)
}

pub fn find_address_index(sapling: &DiversifiableFullViewingKey, start: u32) -> u32 {
    let (di, _) = sapling.find_address(DiversifierIndex::from(start)).unwrap();
    let next: u32 = di.try_into().unwrap();
    next
}

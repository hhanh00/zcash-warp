use anyhow::Result;
use rusqlite::Connection;
use zcash_keys::encoding::{encode_extended_spending_key, encode_payment_address, AddressCodec as _};
use zcash_protocol::consensus::NetworkConstants;

use crate::{
    data::fb::ZIP32KeysT,
    db::{
        account::get_account_info,
        account_manager::{detect_key, KeyType},
    },
    keys::{derive_bip32, derive_zip32, export_sk_bip38}, network::Network,
};
use warp_macros::c_export;
use crate::{coin::COINS, ffi::{map_result_bytes, CResult}};
use flatbuffers::FlatBufferBuilder;

#[c_export]
pub fn derive_zip32_keys(network: &Network, connection: &Connection, account: u32, acc_index: u32) -> Result<ZIP32KeysT> {
    let ai = get_account_info(network, connection, account)?;
    let keys = ai.seed.as_ref().map(|seed| {
        let KeyType::Seed(_seed_str, seed, _acc_index) =
            detect_key(network, seed, 0)?
        else {
            unreachable!()
        };
        let si = derive_zip32(network, &seed, acc_index);
        let ti = derive_bip32(network, &seed, 0, acc_index, true);
        Ok::<_, anyhow::Error>(ZIP32KeysT {
            tsk: ti.sk.as_ref().map(|sk| export_sk_bip38(sk)),
            taddress: Some(ti.addr.encode(network)),
            zsk: si.sk.map(|sk| encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), &sk)),
            zaddress: Some(encode_payment_address(network.hrp_sapling_payment_address(), &si.addr)),
        })
    }).transpose()?.ok_or(anyhow::anyhow!("No seed"))?;
    Ok(keys)
}

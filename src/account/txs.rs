use crate::{
    data::fb::TransactionInfoT,
    db::{contacts::list_contacts, tx::list_txs},
    utils::to_txid_str,
};
use anyhow::Result;
use rusqlite::Connection;
use zcash_primitives::consensus::Network;
use zcash_keys::address::Address as RecipientAddress;

use super::contacts::recipient_contains;

use crate::{
    coin::COINS,
    ffi::{map_result_bytes, CResult},
};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use warp_macros::c_export;

#[c_export]
pub fn get_txs(
    network: &Network,
    connection: &Connection,
    account: u32,
    bc_height: u32,
) -> Result<Vec<TransactionInfoT>> {
    let txs = list_txs(connection, account)?;
    let contacts = list_contacts(network, connection)?;
    let mut tis = vec![];
    for ertx in txs {
        let rtx = &ertx.rtx;
        let mut contact = None;
        if let Some(tx_address) = &ertx.address {
            let tx_address = RecipientAddress::decode(network, tx_address).unwrap();
            for c in contacts.iter() {
                if recipient_contains(&c.address, &tx_address)? {
                    contact = c.card.name.clone();
                }
            }
        }
        let ti = TransactionInfoT {
            id: rtx.id,
            txid: Some(to_txid_str(&rtx.txid)),
            height: rtx.height,
            confirmations: bc_height - rtx.height + 1,
            timestamp: rtx.timestamp,
            amount: rtx.value,
            address: ertx.address,
            contact,
            memo: ertx.memo,
        };
        tis.push(ti);
    }
    Ok(tis)
}

use std::str::FromStr;

use anyhow::Result;
use rand::rngs::OsRng;
use rusqlite::Connection;
use zcash_protocol::{consensus::Network, memo::Memo};

use crate::{
    data::fb::{PaymentRequests, PaymentRequestsT, TransactionSummary, TransactionSummaryT},
    db::notes::snap_to_checkpoint,
    keys::TSKStore,
    lwd::{get_last_height, get_tree_state, broadcast},
    pay::{make_payment, Payment, PaymentItem, UnsignedTransaction},
    types::PoolMask,
    Client,
};

use crate::{
    coin::COINS,
    ffi::{map_result_bytes, map_result_string, CParam, CResult},
};
use flatbuffers::FlatBufferBuilder;
use warp_macros::c_export;
use std::ffi::c_char;

#[c_export]
pub async fn pay(
    network: &Network,
    connection: &Connection,
    client: &mut Client,
    account: u32,
    recipients: &PaymentRequestsT,
    src_pools: u8,
    fee_paid_by_sender: bool,
    confirmations: u32,
) -> Result<TransactionSummaryT> {
    let bc_height = get_last_height(client).await?;
    let cp_height = snap_to_checkpoint(&connection, bc_height - confirmations + 1)?;
    let (s_tree, o_tree) = get_tree_state(client, cp_height).await?;
    let recipients = recipients
        .payments
        .as_ref()
        .unwrap()
        .iter()
        .map(|p| {
            let memo = Memo::from_str(p.memo_string.as_ref().unwrap()).unwrap();
            PaymentItem {
                address: p.address.clone().unwrap(),
                amount: p.amount,
                memo: Some(memo.into()),
            }
        })
        .collect::<Vec<_>>();
    let p = Payment { recipients };
    let unsigned_tx = make_payment(
        network,
        &connection,
        account,
        cp_height,
        p,
        PoolMask(src_pools),
        fee_paid_by_sender,
        &s_tree,
        &o_tree,
    )?;
    let summary = unsigned_tx.to_summary()?;
    Ok(summary)
}

#[c_export]
pub fn sign(
    network: &Network,
    connection: &Connection,
    summary: &TransactionSummaryT,
    expiration_height: u32,
) -> Result<Vec<u8>> {
    let data = summary.data.as_ref().unwrap();
    let unsigned_tx = bincode::deserialize_from::<_, UnsignedTransaction>(&data[..])?;
    let txb = unsigned_tx.build(
        network,
        connection,
        expiration_height,
        &mut TSKStore::default(),
        OsRng,
    )?;
    tracing::info!("TXBLen {}", txb.len());
    Ok(txb)
}

#[c_export]
pub async fn tx_broadcast(client: &mut Client, txbytes: &[u8]) -> Result<String> {
    tracing::info!("TXBLen {}", txbytes.len());
    let bc_height = get_last_height(client).await?;
    let id = broadcast(client, bc_height, txbytes).await?;
    Ok(id)
}

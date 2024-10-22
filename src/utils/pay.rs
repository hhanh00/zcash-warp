use anyhow::Result;
use rand::rngs::OsRng;
use rusqlite::Connection;
use zcash_protocol::memo::{Memo, MemoBytes};

use crate::{
    account::contacts::commit_unsaved_contacts,
    data::fb::{
        PaymentRequest, PaymentRequestT, RecipientT, TransactionBytes, TransactionBytesT,
        TransactionSummary, TransactionSummaryT,
    },
    db::{
        account::get_account_info, chain::snap_to_checkpoint, notes::mark_notes_unconfirmed_spent,
    },
    fb_unwrap,
    lwd::{broadcast, get_last_height, get_tree_state},
    network::Network,
    pay::{make_payment, UnsignedTransaction},
    Client, EXPIRATION_HEIGHT_DELTA,
};

use crate::{
    coin::COINS,
    ffi::{map_result, map_result_bytes, map_result_string, CParam, CResult},
};
use flatbuffers::FlatBufferBuilder;
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

pub(crate) const COST_PER_ACTION: u64 = 5_000;

#[c_export]
pub async fn prepare_payment(
    network: &Network,
    connection: &Connection,
    client: &mut Client,
    account: u32,
    payment: &PaymentRequestT,
    redirect: &str,
) -> Result<TransactionSummaryT> {
    tracing::info!("{:?}", payment);
    let cp_height = snap_to_checkpoint(&connection, payment.height)?;
    let (s_tree, o_tree) = get_tree_state(client, cp_height).await?;
    let recipients = payment
        .recipients
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| r.normalize_memo())
        .collect::<Result<Vec<_>>>()?;
    let payment = PaymentRequestT {
        recipients: Some(recipients),
        src_pools: payment.src_pools,
        sender_pay_fees: payment.sender_pay_fees,
        use_change: payment.use_change,
        height: cp_height.0,
        expiration: payment.expiration,
    };
    let redirect = if redirect.is_empty() {
        None
    } else {
        Some(redirect.to_string())
    };
    let unsigned_tx = make_payment(
        network,
        &connection,
        account,
        &payment,
        &s_tree,
        &o_tree,
        redirect,
    )?;
    let summary = unsigned_tx.to_summary()?;
    Ok(summary)
}

#[c_export]
pub fn can_sign(
    network: &Network,
    connection: &Connection,
    account: u32,
    summary: &TransactionSummaryT,
) -> Result<bool> {
    let utx = fb_unwrap!(summary.data);
    let utx = bincode::deserialize_from::<_, UnsignedTransaction>(&utx[..])?;
    let ai = get_account_info(network, connection, account)?;
    let mut pools_required = 0;
    for n in utx.tx_notes.iter() {
        match &n.note {
            crate::pay::InputNote::Transparent { .. } => {
                pools_required |= 1;
            }
            crate::pay::InputNote::Sapling { .. } => {
                pools_required |= 2;
            }
            crate::pay::InputNote::Orchard { .. } => {
                pools_required |= 4;
            }
        }
    }
    let mut pools_available = 0;
    if ai
        .transparent
        .as_ref()
        .and_then(|ti| ti.sk.as_ref())
        .is_some()
    {
        pools_available |= 1;
    }
    if ai.sapling.as_ref().and_then(|si| si.sk.as_ref()).is_some() {
        pools_available |= 2;
    }
    if ai.orchard.as_ref().and_then(|oi| oi.sk.as_ref()).is_some() {
        pools_available |= 4;
    }
    let can_sign = (pools_available & pools_required) == pools_required;
    Ok(can_sign)
}

#[c_export]
pub fn sign(
    network: &Network,
    connection: &Connection,
    summary: &TransactionSummaryT,
    expiration_height: u32,
) -> Result<TransactionBytesT> {
    let data = fb_unwrap!(summary.data);
    let unsigned_tx = bincode::deserialize_from::<_, UnsignedTransaction>(&data[..])?;
    let txb = unsigned_tx.build(network, connection, expiration_height, OsRng)?;
    tracing::info!("TXBLen {}", txb.data.as_ref().unwrap().len());
    Ok(txb)
}

#[c_export]
pub async fn tx_broadcast(
    connection: &Connection,
    client: &mut Client,
    txbytes: &TransactionBytesT,
) -> Result<String> {
    let bc_height = get_last_height(client).await?;
    if let Some(id_notes) = txbytes.notes.as_deref() {
        mark_notes_unconfirmed_spent(connection, id_notes, bc_height + EXPIRATION_HEIGHT_DELTA)?;
    }
    let id = broadcast(client, bc_height, txbytes).await?;
    Ok(id)
}

#[c_export]
pub async fn save_contacts(
    network: &Network,
    connection: &Connection,
    client: &mut Client,
    account: u32,
    height: u32,
    redirect: &str,
) -> Result<TransactionSummaryT> {
    let cp_height = snap_to_checkpoint(&connection, height)?;
    let (s_tree, o_tree) = get_tree_state(client, cp_height).await?;
    let unsigned_tx = commit_unsaved_contacts(
        network,
        &connection,
        account,
        7,
        cp_height,
        &s_tree,
        &o_tree,
        Some(redirect.to_string()),
    )?;
    unsigned_tx.to_summary().map_err(anyhow::Error::msg)
}

impl RecipientT {
    pub fn normalize_memo(&self) -> Result<Self> {
        let memo = self.memo.clone().map(|m| m.to_memo()).transpose()?;
        let memo2 = self
            .memo_bytes
            .as_ref()
            .map(|mb| Memo::from_bytes(mb))
            .transpose()?;
        let memo = memo.or(memo2).unwrap_or(Memo::Empty);
        let memo = MemoBytes::from(&memo);
        let r = RecipientT {
            address: self.address.clone(),
            amount: self.amount,
            pools: self.pools,
            memo: None,
            memo_bytes: Some(memo.as_slice().to_vec()),
        };
        Ok(r)
    }
}

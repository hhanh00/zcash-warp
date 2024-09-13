use std::str::FromStr;

use anyhow::Result;
use rand::rngs::OsRng;
use rusqlite::Connection;
use zcash_keys::encoding::AddressCodec as _;
use zcash_protocol::{consensus::Network, memo::Memo};

use crate::{
    account::contacts::commit_unsaved_contacts,
    coin::connect_lwd,
    data::fb::{PaymentRequests, PaymentRequestsT, TransactionSummary, TransactionSummaryT},
    db::{account::get_account_info, chain::snap_to_checkpoint},
    keys::{import_sk_bip38, TSKStore},
    lwd::{broadcast, get_last_height, get_tree_state},
    pay::{
        make_payment,
        sweep::{prepare_sweep, scan_utxo_by_address, scan_utxo_by_seed},
        Payment, PaymentItem, UnsignedTransaction,
    },
    types::{PoolMask, TransparentAccountInfo},
    Client,
};

use crate::{
    coin::COINS,
    ffi::{map_result_bytes, map_result_string, CParam, CResult},
};
use flatbuffers::FlatBufferBuilder;
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

#[c_export]
pub async fn prepare_payment(
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
    let summary = unsigned_tx.to_summary(vec![])?;
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
    let keys = summary.keys.as_ref().unwrap();
    let mut tsk_store = bincode::deserialize_from::<_, TSKStore>(&keys[..])?;
    let txb = unsigned_tx.build(
        network,
        connection,
        expiration_height,
        &mut tsk_store,
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

#[c_export]
pub async fn save_contacts(
    network: &Network,
    connection: &Connection,
    client: &mut Client,
    account: u32,
    height: u32,
    confirmations: u32,
) -> Result<TransactionSummaryT> {
    let cp_height = snap_to_checkpoint(&connection, height - confirmations + 1)?;
    let (s_tree, o_tree) = get_tree_state(client, cp_height).await?;
    let unsigned_tx = commit_unsaved_contacts(
        network,
        &connection,
        account,
        7,
        cp_height,
        &s_tree,
        &o_tree,
    )?;
    unsigned_tx.to_summary(vec![]).map_err(anyhow::Error::msg)
}

#[c_export]
pub async fn prepare_sweep_tx(
    network: &Network,
    connection: &Connection,
    url: String,
    account: u32,
    confirmations: u32,
    destination_address: &str,
    gap_limit: usize,
) -> Result<TransactionSummaryT> {
    let ai = get_account_info(network, connection, account)?;
    let mut client = connect_lwd(&url).await?;
    let bc_height = get_last_height(&mut client).await?;
    let cp_height = snap_to_checkpoint(connection, bc_height - confirmations + 1)?;
    let (s, o) = get_tree_state(&mut client, cp_height).await?;
    let (utxos, tsk_store) =
        scan_utxo_by_seed(network, &url, ai, bc_height, 0, true, gap_limit).await?;
    let unsigned_tx = prepare_sweep(
        network,
        &connection,
        account,
        bc_height,
        &utxos,
        destination_address,
        &s,
        &o,
    )?;
    let keys = bincode::serialize(&tsk_store)?;
    let sweep_tx = unsigned_tx.to_summary(keys)?;
    Ok(sweep_tx)
}

#[c_export]
pub async fn prepare_sweep_tx_by_sk(
    network: &Network,
    connection: &Connection,
    url: String,
    account: u32,
    sk: &str,
    confirmations: u32,
    destination_address: &str,
) -> Result<TransactionSummaryT> {
    let sk = import_sk_bip38(sk)?;
    let ti = TransparentAccountInfo::from_secret_key(sk, true);
    let address = ti.addr.encode(network);
    let mut client = connect_lwd(&url).await?;
    let bc_height = get_last_height(&mut client).await?;
    let cp_height = snap_to_checkpoint(connection, bc_height - confirmations + 1)?;
    let (s, o) = get_tree_state(&mut client, cp_height).await?;
    let utxos = scan_utxo_by_address(url, account, bc_height, address.clone()).await?;
    let unsigned_tx = prepare_sweep(
        network,
        &connection,
        account,
        bc_height,
        &utxos,
        destination_address,
        &s,
        &o,
    )?;
    let mut tsk_store = TSKStore::default();
    tsk_store.0.insert(address, sk);
    let keys = bincode::serialize(&tsk_store)?;
    let sweep_tx = unsigned_tx.to_summary(keys)?;
    Ok(sweep_tx)
}

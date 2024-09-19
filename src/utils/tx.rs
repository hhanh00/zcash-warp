use anyhow::Result;
use rusqlite::Connection;
use zcash_protocol::consensus::Network;

use crate::{coin::connect_lwd, data::fb::TransactionInfoExtendedT, db::tx::{get_txid, store_tx_details}, lwd::get_transaction, txdetails::analyze_raw_transaction};

use warp_macros::c_export;
use crate::{coin::COINS, ffi::{map_result_bytes, CResult}};
use flatbuffers::FlatBufferBuilder;

#[c_export]
pub async fn fetch_tx_details(
    network: &Network,
    connection: &Connection,
    url: String,
    account: u32,
    id: u32,
) -> Result<TransactionInfoExtendedT> {
    let mut client = connect_lwd(&url).await?;
    let (txid, timestamp) = get_txid(&connection, id)?;
    let (height, tx) = get_transaction(network, &mut client, &txid).await?;
    let tx = analyze_raw_transaction(
        network,
        &connection,
        url.clone(),
        height,
        timestamp,
        account,
        tx,
    )?;
    let txb = serde_cbor::to_vec(&tx)?;
    store_tx_details(&connection, id, account, height, &tx.txid, &txb)?;
    let etx = tx.to_transaction_info_ext(network);
    Ok(etx)
}

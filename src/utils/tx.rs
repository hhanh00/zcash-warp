use crate::{coin::CoinDef, network::Network};
use anyhow::Result;
use rusqlite::Connection;

use crate::{
    data::fb::TransactionInfoExtendedT,
    db::tx::{get_txid, store_tx_details},
    lwd::get_transaction,
    txdetails::analyze_raw_transaction,
};


pub async fn fetch_tx_details(
    coin: &CoinDef,
    network: &Network,
    connection: &Connection,
    account: u32,
    id: u32,
) -> Result<TransactionInfoExtendedT> {
    let mut client = coin.connect_lwd()?;
    let (txid, timestamp) = get_txid(&connection, id)?;
    let (height, tx) = get_transaction(network, &mut client, &txid).await?;
    let tx = analyze_raw_transaction(coin, network, &connection, account, height, timestamp, tx)?;
    let txb = serde_cbor::to_vec(&tx)?;
    store_tx_details(&connection, id, account, height, &tx.txid, &txb)?;
    let etx = tx.to_transaction_info_ext(network);
    Ok(etx)
}

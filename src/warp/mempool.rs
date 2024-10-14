use std::{sync::Arc, time::Duration};

use anyhow::Result;
use rusqlite::Connection;
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, Sender},
};
use tonic::Request;
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::{BlockHeight, BranchId};

use crate::{
    coin::CoinDef,
    lwd::rpc::{Empty, RawTransaction},
    network::Network,
    txdetails::analyze_raw_transaction,
};

use crate::{
    coin::COINS,
    ffi::{map_result, CResult},
};
use warp_macros::c_export;

#[derive(Clone, Debug)]
pub enum MempoolMsg {
    Account(u32),
}

pub struct Mempool {}

impl Mempool {
    pub fn run(coin: CoinDef, runtime: Arc<Runtime>) -> Result<Sender<MempoolMsg>> {
        tracing::info!("Running mempool for coin {}", coin.coin);
        let (tx, mut rx) = mpsc::channel::<MempoolMsg>(1);
        runtime.spawn(async move {
            let mut account = 0;
            let mut client = coin.connect_lwd()?;
            let connection = coin.connection()?;
            'outer: loop {
                // clear db of previous unconfirmed_tx
                let mut mempool = client
                    .get_mempool_stream(Request::new(Empty {}))
                    .await?
                    .into_inner();
                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            if let Some(msg) = msg {
                                tracing::info!("Recv {:?}", msg);
                                match msg {
                                    MempoolMsg::Account(new_account) => {
                                        if new_account != account {
                                            account = new_account;
                                            break; // need to request the mempool again
                                        }
                                    }
                                // change of servers?
                                }
                            }
                            else {
                                break 'outer; // we are shutting down
                            }
                        }

                        tx = mempool.message() => {
                            if account == 0 { continue }
                            if let Some(tx) = tx? {
                                let value = compute_tx_value(&coin, &coin.network, &connection, account, &tx).unwrap();
                                tracing::info!("{value}");
                            }
                            else {
                                break;
                            }
                        }
                    }
                }
                tracing::info!("Sleeping before new block");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Ok::<_, anyhow::Error>(())
        });
        Ok(tx)
    }
}

fn compute_tx_value(
    coin: &CoinDef,
    network: &Network,
    connection: &Connection,
    account: u32,
    raw_tx: &RawTransaction,
) -> Result<i64> {
    let height = raw_tx.height as u32;
    let raw_tx = &*raw_tx.data;
    let branch_id = BranchId::for_height(network, BlockHeight::from_u32(height));
    let tx = Transaction::read(raw_tx, branch_id)?;
    let txd = analyze_raw_transaction(coin, network, connection, account, height, 0, tx)?;
    Ok(txd.value)
}

#[c_export]
pub fn mempool_run(coin: &CoinDef) -> Result<()> {
    let mut coin_def = COINS[coin.coin as usize].lock();
    coin_def.run_mempool()?;
    Ok(())
}

#[c_export]
pub async fn mempool_set_account(coin: &CoinDef, account: u32) -> Result<()> {
    if let Some(tx) = coin.mempool_tx.as_ref() {
        let _ = tx.send(MempoolMsg::Account(account)).await;
    };
    Ok(())
}

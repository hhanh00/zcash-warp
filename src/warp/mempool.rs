use std::{sync::Arc, time::Duration};

use anyhow::Result;
use rusqlite::Connection;
use tokio::sync::Mutex;
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, Sender},
};
use tonic::Request;
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::{BlockHeight, BranchId};

use crate::{
    coin::CoinDef,
    db::mempool::{clear_unconfirmed_tx, store_unconfirmed_tx},
    lwd::rpc::{Empty, RawTransaction},
    network::Network,
    txdetails::analyze_raw_transaction,
    utils::ContextExt,
};

use crate::coin::COINS;

use super::sync::ReceivedTx;

#[derive(Clone, Debug)]
pub enum MempoolMsg {
    Account(u32),
}

pub struct Mempool {}

impl Mempool {
    pub fn run(coin: CoinDef, runtime: Arc<Runtime>) -> Result<Sender<MempoolMsg>> {
        tracing::info!("Running mempool for coin {}", coin.coin);
        let (tx, rx) = mpsc::channel::<MempoolMsg>(8);
        let rx = Arc::new(Mutex::new(rx));
        runtime.spawn(async move {
            let mempool_loop = || {
                let c = coin.clone();
                let rx = rx.clone();
                async move {
                    let mut account = 0;
                    let mut client = c.connect_lwd()?;
                    let connection = c.connection()?;
                    'outer: loop {
                        tracing::info!("mempool open");
                        clear_unconfirmed_tx(&connection)?;
                        let mut mempool = client
                            .get_mempool_stream(Request::new(Empty {}))
                            .await
                            .with_file_line(|| "get_mempool_stream")?
                            .into_inner();
                        let mut rx = rx.lock().await;
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
                                        break 'outer Ok::<_, anyhow::Error>(()); // we are shutting down
                                    }
                                }

                                tx = mempool.message() => {
                                    let tx = tx?;
                                    if let Some(tx) = tx {
                                        tracing::info!("{}", tx.height);
                                        if account == 0 { continue }
                                        let tx = parse_raw_tx(&c, &c.network, &connection, account, &tx).unwrap();
                                        if tx.value != 0 {
                                            store_unconfirmed_tx(&connection, &tx)?;
                                        }
                                    }
                                    else {
                                        break;
                                    }
                                }
                            }
                        }
                        tracing::info!("mempool close");
                        tracing::info!("Sleeping before new block");
                        clear_unconfirmed_tx(&connection)?;
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            };

            loop {
                match mempool_loop().await {
                    Ok(_) => {
                        break; // clean shutdown
                    }
                    Err(e) => {
                        tracing::info!("mempool error: {}", e);
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                }
            }
        });
        Ok(tx)
    }
}

fn parse_raw_tx(
    coin: &CoinDef,
    network: &Network,
    connection: &Connection,
    account: u32,
    raw_tx: &RawTransaction,
) -> Result<ReceivedTx> {
    let height = raw_tx.height as u32;
    let raw_tx = &*raw_tx.data;
    let branch_id = BranchId::for_height(network, BlockHeight::from_u32(height));
    let tx = Transaction::read(raw_tx, branch_id)?;
    let txid = tx.txid();
    let txd = analyze_raw_transaction(coin, network, connection, account, height, 0, tx)?;
    let tx = ReceivedTx {
        id: 0,
        account,
        height,
        txid: txid.clone().try_into().unwrap(),
        timestamp: 0,
        ivtx: 0,
        value: txd.value,
    };
    Ok(tx)
}

pub fn mempool_run(coin: &CoinDef) -> Result<()> {
    let mut coin_def = COINS[coin.coin as usize].lock();
    coin_def.run_mempool()?;
    Ok(())
}

pub async fn mempool_set_account(coin: &CoinDef, account: u32) -> Result<()> {
    tracing::info!("ms 1");
    if let Some(tx) = coin.mempool_tx.as_ref() {
        tracing::info!("ms 2");
        let _ = tx.send(MempoolMsg::Account(account)).await;
    };
    tracing::info!("ms 3");
    Ok(())
}

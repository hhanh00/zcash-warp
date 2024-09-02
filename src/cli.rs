use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use clap::{Parser, Subcommand};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use parking_lot::Mutex;
use rand::rngs::OsRng;
use rusqlite::DropBehavior;
use rustyrepl::{Repl, ReplCommandProcessor};
use zcash_client_backend::address::RecipientAddress;
use zcash_primitives::memo::MemoBytes;

use crate::{
    account::{address::get_diversified_address, txs::get_txs},
    coin::CoinDef,
    data::fb::{ShieldedNote, TransactionInfo},
    db::{
        account::{get_account_info, get_balance},
        account_manager::{create_new_account, detect_key},
        migration::init_db,
        notes::{
            get_sync_height, get_txid, get_unspent_notes, store_block, store_tx_details,
            truncate_scan,
        },
        reset_tables,
        tx::get_tx_details,
    },
    fb_vec_to_bytes,
    keys::TSKStore,
    lwd::{
        broadcast, filter_stats, get_compact_block, get_last_height, get_transaction,
        get_tree_state,
    },
    pay::{
        sweep::{prepare_sweep, scan_utxo_by_seed},
        Payment, PaymentBuilder, PaymentItem,
    },
    txdetails::{analyze_raw_transaction, decode_tx_details, retrieve_tx_details},
    types::PoolMask,
    warp::{sync::{builder::test_compact_block, warp_sync}, BlockHeader},
};

/// The enum of sub-commands supported by the CLI
#[derive(Subcommand, Clone, Debug)]
pub enum Command {
    CreateAccount,
    LastHeight,
    SyncHeight,
    Reset {
        height: Option<u32>,
    },
    Sync {
        end_height: Option<u32>,
    },
    Address {
        account: u32,
        mask: u8,
    },
    GetTx {
        account: u32,
        id: u32,
    },
    Balance {
        account: u32,
    },
    GenDiversifiedAddress {
        account: u32,
        pools: u8,
    },
    Pay {
        account: u32,
        address: String,
        amount: u64,
        pools: u8,
        fee_paid_by_sender: u8,
    },
    Sweep {
        account: u32,
        destination_address: String,
    },
    GetTxDetails {
        id: u32,
    },
    DecodeAddress {
        address: String,
    },
    ListTxs {
        account: u32,
    },
    ListNotes {
        account: u32,
    },
    TestFilter,
}

/// The general CLI, essentially a wrapper for the sub-commands [Commands]
#[derive(Parser, Clone, Debug)]
pub struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug)]
pub struct CliProcessor {
    zec: CoinDef,
}

impl CliProcessor {
    pub fn new() -> Self {
        let mut zec = CoinDef::from_network(zcash_primitives::consensus::Network::MainNetwork);
        zec.set_db_path(dotenv::var("DB_PATH").unwrap()).unwrap();
        zec.set_url(&dotenv::var("LWD_URL").unwrap());
        zec.set_warp(&dotenv::var("WARP_URL").unwrap());
        let connection = zec.connection().unwrap();
        // init_db(&connection).unwrap();
        // reset_tables(&connection).unwrap();
        Self { zec }
    }

    async fn start(&self) -> Result<()> {
        // let mut client = self.zec.connect_lwd().await?;
        // test_compact_block(&mut client).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ReplCommandProcessor<Cli> for CliProcessor {
    fn is_quit(&self, command: &str) -> bool {
        matches!(command, "quit" | "exit")
    }

    async fn process_command(&self, command: Cli) -> Result<()> {
        let network = &self.zec.network;
        let mut client = self.zec.connect_lwd().await?;
        let bc_height = get_last_height(&mut client).await?;
        let (s_tree, o_tree) = get_tree_state(&mut client, bc_height).await?;
        match command.command {
            Command::CreateAccount => {
                let connection = self.zec.connection()?;
                let seed = dotenv::var("SEED").unwrap();
                let kt = detect_key(network, &seed, 0, 0)?;
                create_new_account(network, &connection, "Test", kt)?;
            }
            Command::LastHeight => {
                println!("{bc_height}");
            }
            Command::SyncHeight => {
                let connection = self.zec.connection()?;
                let height = get_sync_height(&connection)?;
                println!("{height:?}");
            }
            Command::Reset { height } => {
                let connection = self.zec.connection()?;
                truncate_scan(&connection)?;
                let birth_height = str::parse::<u32>(&dotenv::var("BIRTH").unwrap())?;
                let height = height.unwrap_or(birth_height);
                let mut client = self.zec.connect_lwd().await?;
                let block = get_compact_block(&mut client, height).await?;
                let mut connection = self.zec.connection()?;
                let mut transaction = connection.transaction()?;
                transaction.set_drop_behavior(DropBehavior::Commit);
                store_block(&transaction, &BlockHeader::from(&block))?;
            }
            Command::Sync { end_height } => loop {
                let connection = self.zec.connection()?;
                let end_height = end_height.unwrap_or(bc_height);
                let start_height = get_sync_height(&connection)?.expect("no sync data");
                if start_height >= end_height {
                    break;
                }
                let end_height = (start_height + 100_000).min(end_height);
                warp_sync(&self.zec, start_height, end_height).await?;
                let connection = Mutex::new(self.zec.connection()?);
                retrieve_tx_details(network, connection, self.zec.url.clone()).await?;
            },
            Command::Address { account, mask } => {
                let connection = self.zec.connection()?;
                let ai = get_account_info(network, &connection, account)?;
                let address = ai
                    .to_address(network, PoolMask(mask))
                    .ok_or(anyhow::anyhow!("Invalid mask"))?;
                println!("Address: {}", address);
            }
            Command::Balance { account } => {
                let connection = self.zec.connection()?;
                let height = get_sync_height(&connection)?.unwrap_or_default();
                let balance = get_balance(&connection, account, height)?;
                println!("Balance: {:?}", balance);
            }
            Command::Pay {
                account,
                address,
                amount,
                pools,
                fee_paid_by_sender,
            } => {
                let connection = self.zec.connection()?;
                let p = Payment {
                    recipients: vec![PaymentItem {
                        address,
                        amount,
                        memo: MemoBytes::empty(),
                    }],
                };
                let height = get_sync_height(&connection)?.unwrap();
                let connection = self.zec.connection()?;
                let mut pb = PaymentBuilder::new(
                    network,
                    &connection,
                    account,
                    height,
                    p,
                    PoolMask(pools),
                    &s_tree,
                    &o_tree,
                )?;
                pb.add_account_funds(&connection)?;
                pb.set_use_change(true)?;
                let mut utx = pb.prepare()?;
                if fee_paid_by_sender == 0 {
                    let fee = pb.fee_manager.fee();
                    utx.add_to_change(fee as i64)?;
                }
                let utx = pb.finalize(utx)?;
                let connection = self.zec.connection()?;
                let txb = utx.build(network, &connection, &mut TSKStore::default(), OsRng)?;
                let r = broadcast(&mut client, height, &txb).await?;
                println!("{}", r);
            }
            Command::GetTx { account, id } => {
                let connection = self.zec.connection()?;
                let (txid, timestamp) = get_txid(&connection, id)?;
                let mut client = self.zec.connect_lwd().await?;
                let (height, tx) = get_transaction(network, &mut client, &txid).await?;
                let tx = analyze_raw_transaction(
                    network,
                    &connection,
                    self.zec.url.clone(),
                    height,
                    timestamp,
                    account,
                    tx,
                )?;
                let txb = serde_cbor::to_vec(&tx)?;
                println!("{}", hex::encode(&txb));
                store_tx_details(&connection, id, &tx.txid, &txb)?;
            }
            Command::GenDiversifiedAddress { account, pools } => {
                let connection = self.zec.connection()?;
                let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32;
                let address =
                    get_diversified_address(network, &connection, account, time, PoolMask(pools))?;
                println!("{}", address);
            }
            Command::Sweep {
                account,
                destination_address,
            } => {
                let connection = self.zec.connection()?;
                let ai = get_account_info(network, &connection, account)?;
                let mut client = self.zec.connect_lwd().await?;
                let height = get_last_height(&mut client).await?;
                let (s, o) = get_tree_state(&mut client, height).await?;
                let (utxos, mut tsk_store) =
                    scan_utxo_by_seed(network, &self.zec.url, ai, height, 0, true, 40).await?;
                let connection = self.zec.connection()?;
                let utx = prepare_sweep(
                    network,
                    &connection,
                    account,
                    height,
                    &utxos,
                    destination_address,
                    &s,
                    &o,
                )?;
                tracing::info!("{}", serde_json::to_string(&utx)?);
                let tx = utx.build(network, &connection, &mut tsk_store, OsRng)?;
                let txid = broadcast(&mut client, height, &tx).await?;
                println!("{}", txid);
            }
            Command::GetTxDetails { id } => {
                let connection = self.zec.connection()?;
                let (account, tx) = get_tx_details(&connection, id)?;
                decode_tx_details(network, &connection, account, id, &tx)?;
                let etx = tx.to_transaction_info_ext(network);
                println!("{:?}", etx);
            }
            Command::DecodeAddress { address } => {
                let ra = RecipientAddress::decode(network, &address)
                    .ok_or(anyhow::anyhow!("Invalid Address"))?;
                println!("{:?}", ra);
            }
            Command::ListTxs { account } => {
                let connection = self.zec.connection()?;
                let txs = get_txs(network, &connection, account, bc_height)?;

                let data = fb_vec_to_bytes!(txs, TransactionInfo)?;
                println!("{}", hex::encode(data));
            }
            Command::ListNotes { account } => {
                let connection = self.zec.connection()?;
                let notes = get_unspent_notes(&connection, account, bc_height)?;

                let data = fb_vec_to_bytes!(notes, ShieldedNote)?;
                println!("{}", hex::encode(data));
            }
            Command::TestFilter => {
                let mut client = self.zec.connect_lwd().await?;
                test_compact_block(&mut client).await?;
            }
        }
        Ok(())
    }
}

pub async fn cli_main() -> Result<()> {
    let cli = CliProcessor::new();
    cli.start().await?;
    let processor: Box<dyn ReplCommandProcessor<Cli>> = Box::new(cli);
    let mut repl = Repl::<Cli>::new(processor, None, Some(">> ".to_string()))?;
    repl.process().await?;

    Ok(())
}

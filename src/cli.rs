use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use clap::Parser;
use clap_repl::{
    reedline::{DefaultPrompt, DefaultPromptSegment, FileBackedHistory},
    ClapEditor,
};
use console::style;
use figment::{
    providers::{Env, Format as _, Toml},
    Figment,
};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use parking_lot::Mutex;
use rand::rngs::OsRng;
use rusqlite::DropBehavior;
use serde::Deserialize;
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::memo::MemoBytes;
use zcash_protocol::consensus::{NetworkUpgrade, Parameters};

use crate::{
    account::{address::get_diversified_address, txs::get_txs}, coin::CoinDef, data::fb::{ShieldedNote, TransactionInfo}, db::{
        account::{get_account_info, get_balance},
        account_manager::{create_new_account, detect_key},
        notes::{
            get_sync_height, get_txid, get_unspent_notes, store_block, store_tx_details,
            truncate_scan,
        },
        reset_tables,
        tx::{get_tx_details, list_messages},
    }, fb_vec_to_bytes, keys::{generate_random_mnemonic_phrase, TSKStore}, lwd::{broadcast, get_compact_block, get_last_height, get_transaction, get_tree_state}, pay::{
        sweep::{prepare_sweep, scan_utxo_by_seed},
        Payment, PaymentBuilder, PaymentItem,
    }, txdetails::{analyze_raw_transaction, decode_tx_details, retrieve_tx_details}, types::PoolMask, utils::ua::decode_ua, warp::{sync::warp_sync, BlockHeader}
};

#[derive(Deserialize)]
pub struct Config {
    pub db_path: String,
    pub lwd_url: String,
    pub warp_url: String,
    pub warp_end_height: u32,
    pub seed: String,
    pub birth: u32,
}

/// The enum of sub-commands supported by the CLI
#[derive(Parser, Clone, Debug)]
pub enum Command {
    CreateDatabase,
    CreateAccount {
        key: Option<String>,
        name: Option<String>,
    },
    GenerateSeed,
    Backup { account: u32 },
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
    ListMessages {
        account: u32,
    },
    DecodeUA { 
        ua: String,
    }
}

#[tokio::main]
async fn process_command(command: Command, zec: &CoinDef) -> Result<()> {
    let network = &zec.network;
    match command {
        Command::CreateDatabase => {
            let connection = zec.connection().unwrap();
            reset_tables(&connection)?;
        }
        Command::CreateAccount { key, name } => {
            let connection = zec.connection()?;
            let key = key.unwrap_or(CONFIG.seed.clone());
            let name = name.unwrap_or("Test".to_string());
            let kt = detect_key(network, &key, 0, 0)?;
            create_new_account(network, &connection, &name, kt)?;
        }
        Command::GenerateSeed => {
            let seed = generate_random_mnemonic_phrase(&mut OsRng);
            println!("{seed}");
        }
        Command::Backup { account } => {
            let connection = zec.connection()?;
            let ai = get_account_info(network, &connection, account)?;
            let backup = ai.to_backup(network);
            println!("{}", serde_json::to_string_pretty(&backup).unwrap());
        }
        Command::LastHeight => {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            println!("{bc_height}");
        }
        Command::SyncHeight => {
            let connection = zec.connection()?;
            let height = get_sync_height(&connection)?;
            println!("{height:?}");
        }
        Command::Reset { height } => {
            let connection = zec.connection()?;
            truncate_scan(&connection)?;
            let activation: u32 = network
                .activation_height(NetworkUpgrade::Sapling)
                .unwrap()
                .into();
            let birth_height = CONFIG.birth.max(activation + 1);
            let height = height.unwrap_or(birth_height);
            let mut client = zec.connect_lwd().await?;
            let block = get_compact_block(&mut client, height).await?;
            let mut connection = zec.connection()?;
            let mut transaction = connection.transaction()?;
            transaction.set_drop_behavior(DropBehavior::Commit);
            store_block(&transaction, &BlockHeader::from(&block))?;
        }
        Command::Sync { end_height } => loop {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let connection = zec.connection()?;
            let end_height = end_height.unwrap_or(bc_height);
            let start_height = get_sync_height(&connection)?.expect("no sync data");
            if start_height >= end_height {
                break;
            }
            let end_height = (start_height + 100_000).min(end_height);
            warp_sync(&zec, start_height, end_height).await?;
            let connection = Mutex::new(zec.connection()?);
            retrieve_tx_details(network, connection, zec.url.clone()).await?;
        },
        Command::Address { account, mask } => {
            let connection = zec.connection()?;
            let ai = get_account_info(network, &connection, account)?;
            let address = ai
                .to_address(network, PoolMask(mask))
                .ok_or(anyhow::anyhow!("Invalid mask"))?;
            println!("Address: {}", address);
        }
        Command::Balance { account } => {
            let connection = zec.connection()?;
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
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let (s_tree, o_tree) = get_tree_state(&mut client, bc_height).await?;
            let connection = zec.connection()?;
            let p = Payment {
                recipients: vec![PaymentItem {
                    address,
                    amount,
                    memo: MemoBytes::empty(),
                }],
            };
            let height = get_sync_height(&connection)?.unwrap();
            let connection = zec.connection()?;
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
            let connection = zec.connection()?;
            let txb = utx.build(network, &connection, &mut TSKStore::default(), OsRng)?;
            let r = broadcast(&mut client, height, &txb).await?;
            println!("{}", r);
        }
        Command::GetTx { account, id } => {
            let connection = zec.connection()?;
            let (txid, timestamp) = get_txid(&connection, id)?;
            let mut client = zec.connect_lwd().await?;
            let (height, tx) = get_transaction(network, &mut client, &txid).await?;
            let tx = analyze_raw_transaction(
                network,
                &connection,
                zec.url.clone(),
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
            let connection = zec.connection()?;
            let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32;
            let address =
                get_diversified_address(network, &connection, account, time, PoolMask(pools))?;
            println!("{}", address);
        }
        Command::Sweep {
            account,
            destination_address,
        } => {
            let connection = zec.connection()?;
            let ai = get_account_info(network, &connection, account)?;
            let mut client = zec.connect_lwd().await?;
            let height = get_last_height(&mut client).await?;
            let (s, o) = get_tree_state(&mut client, height).await?;
            let (utxos, mut tsk_store) =
                scan_utxo_by_seed(network, &zec.url, ai, height, 0, true, 40).await?;
            let connection = zec.connection()?;
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
            let connection = zec.connection()?;
            let (account, tx) = get_tx_details(&connection, id)?;
            decode_tx_details(network, &connection, account, id, &tx)?;
            let etx = tx.to_transaction_info_ext(network);
            println!("{}", serde_json::to_string_pretty(&etx).unwrap());
        }
        Command::DecodeAddress { address } => {
            let ra = RecipientAddress::decode(network, &address)
                .ok_or(anyhow::anyhow!("Invalid Address"))?;
            println!("{:?}", ra);
        }
        Command::ListTxs { account } => {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let connection = zec.connection()?;
            let txs = get_txs(network, &connection, account, bc_height)?;

            for tx in txs.iter() {
                println!("{}", serde_json::to_string_pretty(tx).unwrap());
            }
            let _data = fb_vec_to_bytes!(txs, TransactionInfo)?;
            // println!("{}", hex::encode(data));
        }
        Command::ListNotes { account } => {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let connection = zec.connection()?;
            let notes = get_unspent_notes(&connection, account, bc_height)?;

            println!("{}", serde_json::to_string_pretty(&notes).unwrap());
            let _data = fb_vec_to_bytes!(notes, ShieldedNote)?;
        }
        Command::ListMessages { account } => {
            let connection = zec.connection()?;
            let msgs = list_messages(&connection, account)?;
            println!("{}", serde_json::to_string_pretty(&msgs).unwrap());
        }
        Command::DecodeUA { ua } => {
            let ua = decode_ua(network, &ua)?;            
            println!("{}", serde_json::to_string_pretty(&ua).unwrap());
        }
    }
    Ok(())
}

pub fn cli_main() -> Result<()> {
    let mut zec = CoinDef::from_network(zcash_primitives::consensus::Network::MainNetwork);
    zec.set_db_path(&CONFIG.db_path).unwrap();
    zec.set_url(&CONFIG.lwd_url);
    zec.set_warp(&CONFIG.warp_url);
    let prompt = DefaultPrompt {
        left_prompt: DefaultPromptSegment::Basic("zcash-warp".to_owned()),
        ..DefaultPrompt::default()
    };
    let rl = ClapEditor::<Command>::builder()
        .with_prompt(Box::new(prompt))
        .with_editor_hook(|reed| {
            reed.with_history(Box::new(
                FileBackedHistory::with_file(10000, "/tmp/zcash-warp-history".into()).unwrap(),
            ))
        })
        .build();

    rl.repl(|command| {
        if let Err(e) = process_command(command, &zec) {
            println!("{} {}", style("Error:").red().bold(), e);
        }
    });

    Ok(())
}

pub fn init_config() -> Config {
    let config: Config = Figment::new()
        .merge(Toml::file("App.toml"))
        .merge(Env::prefixed("ZCASH_WARP_"))
        .extract()
        .unwrap();
    config
}

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = init_config();
}

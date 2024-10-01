use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    account::address::get_diversified_address,
    data::fb::{TransactionBytesT, ZipDbConfigT},
    db::{
        account::{get_account_info, list_account_transparent_addresses},
        notes::list_utxos,
    },
    fb_unwrap,
    network::{regtest, Network},
    pay::sweep::scan_transparent_addresses,
    types::PoolMask,
    utils::chain::reset_chain,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use clap_repl::{
    reedline::{DefaultPrompt, DefaultPromptSegment, FileBackedHistory},
    ClapEditor,
};
use console::style;
use figment::{
    providers::{Env, Format as _, Toml},
    Figment,
};
use hex::FromHexError;
use rand::rngs::OsRng;
use rusqlite::Connection;
use zcash_protocol::consensus::{NetworkUpgrade, Parameters};

use crate::{
    account::{
        contacts::{add_contact, commit_unsaved_contacts},
        txs::get_txs,
    },
    coin::CoinDef,
    data::fb::{ConfigT, PacketT, PacketsT, PaymentRequestT, RecipientT, TransactionSummaryT},
    db::{
        account::{get_account_property, get_balance, list_accounts, set_account_property},
        account_manager::{
            create_new_account, delete_account, edit_account_birth, edit_account_name,
            get_min_birth, new_transparent_address,
        },
        chain::{get_sync_height, list_checkpoints, rewind, snap_to_checkpoint},
        contacts::{
            delete_contact, edit_contact_address, edit_contact_name, get_contact, list_contacts,
        },
        messages::{get_message, list_messages, mark_all_read, mark_read},
        notes::{exclude_note, get_unspent_notes, reverse_note_exclusion},
        reset_tables,
        tx::{get_tx_details_account, get_txid, store_tx_details},
    },
    keys::generate_random_mnemonic_phrase,
    lwd::{broadcast, get_last_height, get_transaction, get_tree_state},
    txdetails::{analyze_raw_transaction, decode_tx_details, retrieve_tx_details},
    types::CheckpointHeight,
    utils::{
        chain::{get_activation_date, get_height_by_time},
        data_split::{merge, split},
        db::{create_backup, encrypt_db, get_address},
        messages::navigate_message,
        pay::{prepare_payment, sign},
        ua::decode_address,
        uri::{make_payment_uri, parse_payment_uri},
        zip_db::{
            decrypt_zip_database_files, encrypt_zip_database_files, generate_zip_database_keys,
        },
    },
    warp::sync::warp_sync,
    EXPIRATION_HEIGHT_DELTA,
};

#[derive(Parser, Clone, Debug)]
pub struct Account {
    #[structopt(subcommand)]
    command: AccountCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum AccountCommand {
    List,
    Create {
        key: String,
        name: Option<String>,
        birth: Option<u32>,
    },
    EditName {
        account: u32,
        name: String,
    },
    EditBirthHeight {
        account: u32,
        birth: u32,
    },
    Delete {
        account: u32,
    },
    NewTransparentAddress {
        account: u32,
    },
    ListTransparentAddresses {
        account: u32,
    },
    Scan {
        account: u32,
        gap_limit: u32,
    },
    SetProperty {
        account: u32,
        name: String,
        value: String,
    },
    GetProperty {
        account: u32,
        name: String,
    },
}

#[derive(Parser, Clone, Debug)]
pub struct Contact {
    #[structopt(subcommand)]
    command: ContactCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ContactCommand {
    List,
    Create {
        account: u32,
        name: String,
        address: String,
    },
    Get {
        id: u32,
    },
    EditName {
        id: u32,
        name: String,
    },
    EditAddress {
        id: u32,
        address: String,
    },
    Delete {
        id: u32,
    },
    Save {
        account: u32,
    },
}

#[derive(Parser, Clone, Debug)]
pub struct Chain {
    #[structopt(subcommand)]
    command: ChainCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ChainCommand {
    GetActivationDate,
    GetHeightFromTime { time: u32 },
}

#[derive(Parser, Clone, Debug)]
pub struct Message {
    #[structopt(subcommand)]
    command: MessageCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum MessageCommand {
    Prev { id: u32 },
    Next { id: u32 },
    PrevInThread { id: u32 },
    NextInThread { id: u32 },
    List { account: u32 },
    MarkAllRead { account: u32, reverse: u8 },
    MarkRead { id: u32, reverse: u8 },
}

#[derive(Parser, Clone, Debug)]
pub struct Note {
    #[structopt(subcommand)]
    command: NoteCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum NoteCommand {
    List { account: u32 },
    Exclude { id: u32, reverse: u8 },
    Reverse { account: u32 },
    Utxo { account: u32 },
}

#[derive(Parser, Clone, Debug)]
pub struct Database {
    #[structopt(subcommand)]
    command: DatabaseCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DatabaseCommand {
    EncryptDb {
        password: String,
        new_db_path: String,
    },
    SetDbPassword {
        password: String,
    },
    GenerateKeys,
    Encrypt {
        config: ZipDbConfigT,
    },
    Decrypt {
        file_path: String,
        target_directory: String,
        secret_key: String,
    },
}

#[derive(Parser, Clone, Debug)]
pub struct Checkpoint {
    #[structopt(subcommand)]
    command: CheckpointCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum CheckpointCommand {
    List,
    Rewind { height: u32 },
}

#[derive(Parser, Clone, Debug)]
pub struct Keys {
    #[structopt(subcommand)]
    command: KeysCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum KeysCommand {
    ViewingKey { account: u32, pools: u8 },
    GetDiversifiedAddress { account: u32, index: u32, pools: u8 },
}

#[derive(Parser, Clone, Debug)]
pub struct QRData {
    #[structopt(subcommand)]
    command: QRDataCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum QRDataCommand {
    Split { data: String, threshold: u32 },
    Merge { parts: String, data_len: usize },
}

/// The enum of sub-commands supported by the CLI
#[derive(Parser, Clone, Debug)]
pub enum Command {
    Account(Account),
    Contact(Contact),
    Chain(Chain),
    Message(Message),
    Note(Note),
    Database(Database),
    Keys(Keys),
    QRData(QRData),
    Checkpoint(Checkpoint),
    CreateDatabase,
    GenerateSeed,
    Backup {
        account: u32,
    },
    LastHeight,
    SyncHeight,
    Reset {
        height: Option<u32>,
    },
    Sync {
        confirmations: Option<u32>,
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
    Pay {
        account: u32,
        address: String,
        to_pools: u8,
        amount: u64,
        from_pools: u8,
        fee_paid_by_sender: u8,
        use_change: u8,
    },
    MultiPay {
        account: u32,
        payment: PaymentRequestT,
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
    MakePaymentURI {
        payment: PaymentRequestT,
    },
    PayPaymentUri {
        account: u32,
        uri: String,
    },
    BroadcastLatest {
        clear: Option<u8>,
    },
}

macro_rules! impl_fb_from_str {
    ($v: ident) => {
        impl FromStr for $v {
            type Err = serde_json::Error;

            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                serde_json::from_str::<$v>(s)
            }
        }
    };
}

impl_fb_from_str!(PaymentRequestT);
impl_fb_from_str!(ZipDbConfigT);

fn display_tx(
    network: &Network,
    connection: &Connection,
    mut summary: TransactionSummaryT,
) -> Result<TransactionBytesT> {
    let txb = sign(
        network,
        connection,
        &summary,
        summary.height + EXPIRATION_HEIGHT_DELTA,
    )?;
    summary.detach();
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    Ok(txb)
}

#[tokio::main]
async fn process_command(
    command: Command,
    zec: &mut CoinDef,
    txbytes: &mut TransactionBytesT,
) -> Result<()> {
    let network = &zec.network;
    match command {
        Command::CreateDatabase => {
            let connection = zec.connection().unwrap();
            reset_tables(network, &connection, false)?;
        }
        Command::Account(account_cmd) => {
            let mut connection = zec.connection()?;
            match account_cmd.command {
                AccountCommand::List => {
                    let accounts = list_accounts(zec.coin, &connection)?;
                    println!("{}", serde_json::to_string_pretty(&accounts)?);
                }
                AccountCommand::Create { key, name, birth } => {
                    let birth = match birth {
                        Some(b) => b,
                        None => {
                            // Avoid using LWD if the user gave us the wallet birth height
                            let mut client = zec.connect_lwd().await?;
                            let bc_height = get_last_height(&mut client).await?;
                            bc_height
                        }
                    };
                    let name = name.unwrap_or("<unnamed>".to_string());
                    create_new_account(network, &connection, &name, &key, 0, birth)?;
                }
                AccountCommand::NewTransparentAddress { account } => {
                    new_transparent_address(network, &connection, account)?;
                }
                AccountCommand::ListTransparentAddresses { account } => {
                    let t_addresses = list_account_transparent_addresses(&connection, account)?;
                    println!("{:?}", t_addresses);
                }
                AccountCommand::Scan { account, gap_limit } => {
                    let mut client = zec.connect_lwd().await?;
                    scan_transparent_addresses(
                        &network,
                        &mut connection,
                        &mut client,
                        account,
                        gap_limit,
                    )
                    .await?;
                }
                AccountCommand::EditName { account, name } => {
                    edit_account_name(&connection, account, &name)?;
                }
                AccountCommand::EditBirthHeight { account, birth } => {
                    edit_account_birth(&connection, account, birth)?;
                }
                AccountCommand::Delete { account } => {
                    delete_account(&connection, account)?;
                }
                AccountCommand::SetProperty {
                    account,
                    name,
                    value,
                } => {
                    set_account_property(&connection, account, &name, &hex::decode(value)?)?;
                }
                AccountCommand::GetProperty { account, name } => {
                    let value = get_account_property(&connection, account, &name)?;
                    println!("{}", hex::encode(&value));
                }
            }
        }
        Command::Contact(contact_cmd) => {
            let connection = zec.connection()?;
            match contact_cmd.command {
                ContactCommand::List => {
                    let contacts = list_contacts(network, &connection)?;
                    let cards = contacts.iter().map(|c| c.card.clone()).collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&cards).unwrap());
                }
                ContactCommand::Create {
                    account,
                    name,
                    address,
                } => {
                    add_contact(network, &connection, account, &name, &address, false)?;
                }
                ContactCommand::Get { id } => {
                    let contact = get_contact(network, &connection, id)?;
                    println!("{contact:?}");
                }
                ContactCommand::EditName { id, name } => {
                    edit_contact_name(&connection, id, &name)?;
                }
                ContactCommand::EditAddress { id, address } => {
                    edit_contact_address(network, &connection, id, &address)?;
                }
                ContactCommand::Delete { id } => {
                    delete_contact(&connection, id)?;
                }
                ContactCommand::Save { account } => {
                    let mut client = zec.connect_lwd().await?;
                    let bc_height = get_last_height(&mut client).await?;
                    let cp_height =
                        snap_to_checkpoint(&connection, bc_height - zec.config.confirmations + 1)?;
                    let (s_tree, o_tree) = get_tree_state(&mut client, cp_height).await?;
                    let summary = commit_unsaved_contacts(
                        network,
                        &connection,
                        account,
                        7,
                        cp_height,
                        &s_tree,
                        &o_tree,
                        None,
                    )?
                    .to_summary()?;
                    *txbytes = display_tx(network, &connection, summary)?;
                }
            }
        }
        Command::Chain(chain_command) => {
            let mut client = zec.connect_lwd().await?;
            match chain_command.command {
                ChainCommand::GetActivationDate => {
                    let timestamp = get_activation_date(network, &mut client).await?;
                    let datetime = DateTime::<Utc>::from_timestamp(timestamp as i64, 0).unwrap();
                    let timestamp_str = datetime.format("%Y-%m-%d").to_string();
                    println!("{timestamp_str}");
                }
                ChainCommand::GetHeightFromTime { time } => {
                    let height = get_height_by_time(network, &mut client, time).await?;
                    println!("height: {height}");
                }
            }
        }
        Command::Message(message_command) => {
            let connection = zec.connection()?;
            let message = match message_command.command {
                MessageCommand::Prev { id } => {
                    let m = get_message(&connection, id)?;
                    navigate_message(&connection, m.account, m.height, None, true)
                }
                MessageCommand::Next { id } => {
                    let m = get_message(&connection, id)?;
                    navigate_message(&connection, m.account, m.height, None, false)
                }
                MessageCommand::PrevInThread { id } => {
                    let m = get_message(&connection, id)?;
                    let subject = m.memo.as_ref().and_then(|m| m.subject.clone());
                    navigate_message(&connection, m.account, m.height, subject, true)
                }
                MessageCommand::NextInThread { id } => {
                    let m = get_message(&connection, id)?;
                    let subject = m.memo.as_ref().and_then(|m| m.subject.clone());
                    navigate_message(&connection, m.account, m.height, subject, false)
                }
                MessageCommand::List { account } => {
                    let msgs = list_messages(&connection, account)?;
                    println!("{}", serde_json::to_string_pretty(&msgs).unwrap());
                    Ok(None)
                }
                MessageCommand::MarkRead { id, reverse } => {
                    mark_read(&connection, id, reverse != 0)?;
                    Ok(None)
                }
                MessageCommand::MarkAllRead { account, reverse } => {
                    mark_all_read(&connection, account, reverse != 0)?;
                    Ok(None)
                }
            }?;
            println!("{message:?}");
        }
        Command::Note(note_command) => {
            let connection = zec.connection()?;
            match note_command.command {
                NoteCommand::List { account } => {
                    let notes = get_unspent_notes(&connection, account, u32::MAX)?;
                    println!("{}", serde_json::to_string_pretty(&notes).unwrap());
                }
                NoteCommand::Exclude { id, reverse } => {
                    exclude_note(&connection, id, reverse != 0)?;
                }
                NoteCommand::Reverse { account } => {
                    reverse_note_exclusion(&connection, account)?;
                }
                NoteCommand::Utxo { account } => {
                    let utxos = list_utxos(&connection, Some(account), CheckpointHeight(u32::MAX))?;
                    println!("{:?}", utxos);
                }
            }
        }
        Command::Database(database_command) => match database_command.command {
            DatabaseCommand::EncryptDb {
                password,
                new_db_path,
            } => {
                let connection = zec.connection()?;
                encrypt_db(&connection, &password, &new_db_path)?;
            }
            DatabaseCommand::SetDbPassword { password } => {
                zec.db_password = Some(password);
            }
            DatabaseCommand::Encrypt { config } => {
                encrypt_zip_database_files(&config)?;
            }
            DatabaseCommand::Decrypt {
                file_path,
                target_directory,
                secret_key,
            } => {
                decrypt_zip_database_files(&file_path, &target_directory, &secret_key)?;
            }
            DatabaseCommand::GenerateKeys => {
                let keys = generate_zip_database_keys()?;
                println!("{keys:?}");
            }
        },
        Command::Keys(keys_command) => match keys_command.command {
            KeysCommand::ViewingKey { account, pools } => {
                let connection = zec.connection()?;
                let ai = get_account_info(network, &connection, account)?;
                let ai = ai.select_pools(PoolMask(pools));
                let uvk = ai.to_vk()?;
                let uvk = uvk.encode(network);
                println!("{uvk}");
            }
            KeysCommand::GetDiversifiedAddress {
                account,
                index,
                pools,
            } => {
                let connection = zec.connection()?;
                let address =
                    get_diversified_address(network, &connection, account, index, PoolMask(pools))?;
                println!("{address}");
            }
        },
        Command::QRData(qr_command) => match qr_command.command {
            QRDataCommand::Split { data, threshold } => {
                let data = hex::decode(&data)?;
                let packets = split(&data, threshold)?;
                for p in packets {
                    println!("{} {}", data.len(), hex::encode(fb_unwrap!(p.data)));
                }
            }
            QRDataCommand::Merge { parts, data_len } => {
                let parts = parts
                    .split(" ")
                    .map(|p| hex::decode(&p).map(|d| PacketT { data: Some(d) }))
                    .collect::<Result<Vec<_>, FromHexError>>()?;
                let packets = PacketsT {
                    packets: Some(parts),
                    len: data_len as u32,
                };
                let data = merge(&packets)?;
                println!("{:?}", data.data.map(|d| hex::encode(&d)));
            }
        },
        Command::Checkpoint(checkpoint_command) => match checkpoint_command.command {
            CheckpointCommand::List => {
                let connection = zec.connection()?;
                let checkpoints = list_checkpoints(&connection)?;
                println!("{checkpoints:?}");
            }
            CheckpointCommand::Rewind { height } => {
                let mut connection = zec.connection()?;
                let mut client = zec.connect_lwd().await?;
                rewind(&network, &mut connection, &mut client, height).await?;
            }
        },
        Command::GenerateSeed => {
            let seed = generate_random_mnemonic_phrase(&mut OsRng);
            println!("{seed}");
        }
        Command::Backup { account } => {
            let connection = zec.connection()?;
            let backup = create_backup(network, &connection, account)?;
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
            let mut connection = zec.connection()?;
            let mut client = zec.connect_lwd().await?;
            let activation: u32 = network
                .activation_height(NetworkUpgrade::Sapling)
                .unwrap()
                .into();
            let min_birth_height = get_min_birth(&connection)?.unwrap_or(activation);
            let height = height.unwrap_or(min_birth_height).max(activation + 1);
            reset_chain(network, &mut connection, &mut client, height).await?;
        }
        Command::Sync {
            confirmations,
            end_height,
        } => loop {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let confirmations = confirmations.unwrap_or(1);
            if confirmations == 0 {
                anyhow::bail!("# Confirmations must be > 0");
            }
            let connection = zec.connection()?;
            let end_height = end_height.unwrap_or(bc_height - confirmations + 1);
            let start_height = get_sync_height(&connection)?;
            if start_height == 0 {
                anyhow::bail!("no sync data. Have you run reset?");
            }
            if start_height >= end_height {
                break;
            }
            let end_height = (start_height + 100_000).min(end_height);
            warp_sync(&zec, CheckpointHeight(start_height), end_height).await?;
            let connection = zec.connection()?;
            retrieve_tx_details(network, &connection, zec.config.lwd_url.clone().unwrap()).await?;
        },
        Command::Address { account, mask } => {
            let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32;
            let connection = zec.connection()?;
            let address = get_address(network, &connection, account, time, mask)?;
            println!("Address: {}", address);
        }
        Command::Balance { account } => {
            let connection = zec.connection()?;
            let height = get_sync_height(&connection)?;
            let balance = get_balance(&connection, account, height)?;
            println!("Balance: {:?}", balance);
        }
        Command::Pay {
            account,
            address,
            to_pools,
            amount,
            from_pools,
            fee_paid_by_sender,
            use_change,
        } => {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let connection = zec.connection()?;
            let recipient = RecipientT {
                address: Some(address.clone()),
                amount,
                pools: to_pools,
                memo: None,
                memo_bytes: None,
            };
            let payment = PaymentRequestT {
                recipients: Some(vec![recipient]),
                src_pools: from_pools,
                sender_pay_fees: fee_paid_by_sender != 0,
                use_change: use_change != 0,
                height: bc_height,
                expiration: bc_height + 100,
            };
            tracing::info!("{}", serde_json::to_string(&payment)?);
            let summary =
                prepare_payment(network, &connection, &mut client, account, &payment, "").await?;
            *txbytes = display_tx(network, &connection, summary)?;
        }
        Command::MultiPay { account, payment } => {
            let mut client = zec.connect_lwd().await?;
            let connection = zec.connection()?;
            let summary =
                prepare_payment(network, &connection, &mut client, account, &payment, "").await?;
            *txbytes = display_tx(network, &connection, summary)?;
        }
        Command::GetTx { account, id } => {
            let connection = zec.connection()?;
            let (txid, timestamp) = get_txid(&connection, id)?;
            let mut client = zec.connect_lwd().await?;
            let (height, tx) = get_transaction(network, &mut client, &txid).await?;
            let tx = analyze_raw_transaction(
                network,
                &connection,
                zec.config.lwd_url.clone().unwrap(),
                height,
                timestamp,
                account,
                tx,
            )?;
            let txb = serde_cbor::to_vec(&tx)?;
            println!("{}", hex::encode(&txb));
            store_tx_details(&connection, id, account, height, &tx.txid, &txb)?;
        }
        Command::GetTxDetails { id } => {
            let connection = zec.connection()?;
            let (account, tx) = get_tx_details_account(&connection, id)?;
            decode_tx_details(network, &connection, account, id, &tx)?;
            let etx = tx.to_transaction_info_ext(network);
            println!("{}", serde_json::to_string_pretty(&etx).unwrap());
        }
        Command::DecodeAddress { address } => {
            let receivers = decode_address(network, &address)?;
            println!("{:?}", receivers);
        }
        Command::ListTxs { account } => {
            let mut client = zec.connect_lwd().await?;
            let bc_height = get_last_height(&mut client).await?;
            let connection = zec.connection()?;
            let txs = get_txs(&connection, account, bc_height)?;

            for tx in txs.iter() {
                println!("{}", serde_json::to_string_pretty(tx).unwrap());
            }
        }
        Command::MakePaymentURI { payment } => {
            tracing::info!("{}", serde_json::to_string(&payment)?);
            let payment_uri = make_payment_uri(network, &payment)?;
            println!("{}", payment_uri);
        }
        Command::PayPaymentUri { account, uri } => {
            let mut client = zec.connect_lwd().await?;
            let connection = zec.connection()?;
            let bc_height = get_last_height(&mut client).await?;
            let cp_height =
                snap_to_checkpoint(&connection, bc_height - zec.config.confirmations + 1)?;
            let payment = parse_payment_uri(&uri, cp_height.0, cp_height.0 + 50)?;
            let summary =
                prepare_payment(network, &connection, &mut client, account, &payment, "").await?;
            *txbytes = display_tx(network, &connection, summary)?;
        }
        Command::BroadcastLatest { clear } => {
            let clear = clear.unwrap_or(1);
            if clear != 0 {
                if let Some(tx_bytes) = txbytes.data.as_ref() {
                    tracing::info!("{}", hex::encode(tx_bytes));
                    let mut client = zec.connect_lwd().await?;
                    let bc_height = get_last_height(&mut client).await?;
                    let r = broadcast(&mut client, bc_height, txbytes).await?;
                    println!("{}", r);
                }
            }
        }
    }
    Ok(())
}

pub fn cli_main(config: &ConfigT) -> Result<()> {
    let mut zec = CoinDef::from_network(
        0,
        if config.regtest {
            Network::Regtest(regtest())
        } else {
            Network::Main
        },
    );
    zec.set_config(config)?;
    zec.set_path_password(config.db_path.as_deref().unwrap(), "")?;

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

    let mut txbytes = TransactionBytesT::default();
    rl.repl(|command| {
        if let Err(e) = process_command(command, &mut zec, &mut txbytes) {
            println!("{} {}", style("Error:").red().bold(), e);
        }
    });

    Ok(())
}

pub fn init_config() -> ConfigT {
    let config: ConfigT = Figment::new()
        .merge(Toml::file("App.toml"))
        .merge(Env::prefixed("ZCASH_WARP_"))
        .extract()
        .unwrap();
    config
}

use anyhow::Result;
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rusqlite::{Connection, DropBehavior};
use rustyrepl::{Repl, ReplCommandProcessor};
use tracing::info;
use zcash_primitives::{
    memo::MemoBytes,
    merkle_tree::MerklePath,
    sapling::Node,
    transaction::{components::{amount::BalanceError, Amount}, Transaction},
};
use zcash_warp::{
    db::{
        create_new_account, detect_key, get_account_info, get_sync_height, get_witnesses_v1,
        init_db, list_accounts, store_block, truncate_scan,
    },
    lwd::{broadcast, get_compact_block, get_last_height, get_tree_state},
    pay::{Payment, PaymentBuilder, PaymentItem, UnsignedTransaction},
    types::PoolMask,
    warp::{
        hasher::{empty_roots, SaplingHasher},
        sync::warp_sync,
        BlockHeader,
    },
    CoinDef, COINS,
};

pub fn account_tests() -> Result<()> {
    let mut zec = COINS[0].lock();
    zec.set_db_path("/Users/hanhhuynhhuu/zec.db")?;
    let connection = zec.connection()?;
    init_db(&connection)?;
    let accounts = list_accounts(&connection)?;
    println!("{:?}", &accounts);

    for a in accounts.iter() {
        let account = a.account;
        let ai = get_account_info(&zec.network, &connection, account)?;
        // println!("{:?}", ai);

        println!("{}", hex::encode(&ai.to_account_unique_id()));
        println!("{:?}", ai.to_backup(&zec.network));
        println!("{:?}", ai.to_secret_keys());
        println!("{:?}", ai.to_view_keys());
        println!("{:?}", ai.to_address(&zec.network, PoolMask(7)));
    }

    let seed = dotenv::var("SEED").unwrap();
    // Test new account
    // let phrase = generate_random_mnemonic_phrase(OsRng);
    let phrase = seed;
    let k = detect_key(&zec.network, &phrase, 0, 0)?;
    let account = create_new_account(&zec.network, &connection, "Test", &phrase, k)?;

    println!("account {}", account);

    Ok(())
}

pub async fn test_tree_state_root() -> Result<()> {
    let mut zec = COINS[0].lock();
    zec.set_url("https://lwd1.zcash-infra.com:9067");
    let mut client = zec.connect_lwd().await?;

    for i in 0..2000 {
        println!("{i}");
        get_tree_state(&mut client, 2300000 + i * 100).await?;
    }
    Ok(())
}

pub fn drop_tables(connection: &Connection) -> Result<()> {
    connection.execute("DROP TABLE IF EXISTS txs", [])?;
    connection.execute("DROP TABLE IF EXISTS notes", [])?;
    connection.execute("DROP TABLE IF EXISTS witnesses", [])?;
    connection.execute("DROP TABLE IF EXISTS utxos", [])?;
    connection.execute("DROP TABLE IF EXISTS blcks", [])?;

    connection.execute(
        "CREATE TABLE IF NOT EXISTS blcks(
        height INTEGER PRIMARY KEY,
        hash BLOB NOT NULL,
        prev_hash BLOB NOT NULL,
        timestamp INTEGER NOT NULL)",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS txs(
        id_tx INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        txid BLOB NOT NULL,
        height INTEGER NOT NULL,
        timestamp INTEGER NOT NULL,
        value INTEGER NOT NULL,
        UNIQUE (account, txid))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS notes(
        id_note INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        position INTEGER NOT NULL,
        height INTEGER NOT NULL,
        tx INTEGER NULL,
        output_index INTEGER NOT NULL,
        diversifier BLOB NOT NULL,
        value INTEGER NOT NULL,
        rcm BLOB NOT NULL,
        nf BLOB NOT NULL UNIQUE,
        rho BLOB,
        spent INTEGER,
        orchard BOOL NOT NULL,
        UNIQUE (position, orchard))",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS witnesses(
        id_witness INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        note INTEGER NOT NULL,
        height INTEGER NOT NULL,
        witness BLOB NOT NULL)",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS utxos(
        id_utxo INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        height INTEGER NOT NULL,
        txid BLOB NOT NULL,
        vout INTEGER NULL,
        value INTEGER NOT NULL,
        spent INTEGER,
        UNIQUE (txid, vout))",
        [],
    )?;
    Ok(())
}

async fn _test_witness_migration() -> Result<()> {
    let hasher = SaplingHasher::default();
    let empty_roots = empty_roots(&hasher);
    let mut zec = COINS[0].lock();
    zec.set_db_path("/Users/hanhhuynhhuu/zec.db")?;
    zec.set_url("https://lwd1.zcash-infra.com:9067");
    let mut client = zec.connect_lwd().await?;

    let height = 2298033;
    let (sapling_tree, _) = get_tree_state(&mut client, height).await?;
    let edge = sapling_tree.to_edge(&hasher);
    let root = edge.root(&hasher);
    info!("anchor at {} = {}", height, hex::encode(&root));

    let edge_auth_path = edge.to_auth_path(&hasher);
    let connection = zec.connection()?;
    let ws = get_witnesses_v1(&connection, height, "sapling")?;
    for w in ws {
        let auth_path = w.build_auth_path(&edge_auth_path, &empty_roots);
        let mut ap = vec![];
        let mut p = w.position;
        for i in 0..32 {
            ap.push((Node::new(auth_path.0[i]), p & 1 == 1));
            p /= 2;
        }
        let mp = MerklePath::from_path(ap, w.position as u64);
        let root = mp.root(Node::new(w.value));
        info!("root {}", hex::encode(&root.repr));
    }
    Ok(())
}

/// The enum of sub-commands supported by the CLI
#[derive(Subcommand, Clone, Debug)]
pub enum Command {
    LastHeight,
    SyncHeight,
    Reset { height: Option<u32> },
    Sync { end_height: Option<u32> },
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
        let connection = zec.connection().unwrap();
        init_db(&connection).unwrap();
        drop_tables(&connection).unwrap();
        Self { zec }
    }
}

#[async_trait::async_trait]
impl ReplCommandProcessor<Cli> for CliProcessor {
    fn is_quit(&self, command: &str) -> bool {
        matches!(command, "quit" | "exit")
    }

    async fn process_command(&self, command: Cli) -> Result<()> {
        match command.command {
            Command::LastHeight => {
                let mut client = self.zec.connect_lwd().await?;
                let height = get_last_height(&mut client).await?;
                println!("{height}");
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
                let mut client = self.zec.connect_lwd().await?;
                let last_height = get_last_height(&mut client).await?;
                let end_height = end_height.unwrap_or(last_height);
                let start_height = get_sync_height(&connection)?.expect("no sync data");
                if start_height >= end_height {
                    break;
                }
                let end_height = (start_height + 100_000).min(end_height);
                warp_sync(&self.zec, start_height, end_height).await?;
            },
        }
        Ok(())
    }
}

async fn cli_main() -> Result<()> {
    let processor: Box<dyn ReplCommandProcessor<Cli>> = Box::new(CliProcessor::new());
    let mut repl = Repl::<Cli>::new(processor, None, Some(">> ".to_string()))?;
    repl.process().await?;

    Ok(())
}

pub async fn test_payment() -> Result<Vec<u8>> {
    let mut zec = CoinDef::from_network(zcash_primitives::consensus::Network::MainNetwork);
    zec.set_db_path(dotenv::var("DB_PATH").unwrap()).unwrap();
    zec.set_url(&dotenv::var("LWD_URL").unwrap());

    let connection = zec.connection()?;
    let db_height = get_sync_height(&connection)?.unwrap();
    let mut client = zec.connect_lwd().await?;
    let p = PaymentBuilder::new(
        &zec.network,
        &connection,
        &mut client,
        1,
        db_height,
        Payment {
            src_pools: PoolMask(0),
            recipients: vec![PaymentItem {
                address: "t1MgrHHPgt246ZRBnu93G9nEMxYvttEYAVU".to_string(),
                amount: 4665000,
                memo: MemoBytes::empty(),
            }],
        },
    )
    .await?;
    let tx = p.build()?;
    let tx_ser = bincode::serialize(&tx)?;
    tracing::info!("Unsigned Tx size {}", tx_ser.len());

    let tx: UnsignedTransaction = bincode::deserialize_from(&*tx_ser)?;

    let txb = tx.build(&zec.network, &connection, OsRng)?;

    let consensus_branch_id = zcash_primitives::consensus::BranchId::Nu5;
    let tx = Transaction::read(&*txb, consensus_branch_id)?;
    let tx = tx.into_data();
    if let Some(t) = tx.transparent_bundle() {
        println!("t {} {}", t.vin.len(), t.vout.len());        
    }
    if let Some(t) = tx.sapling_bundle() {
        println!("s {} {}", t.shielded_spends().len(), t.shielded_outputs().len());        
    }
    if let Some(t) = tx.orchard_bundle() {
        println!("o {} ", t.actions().len());        
    }

    let fee = tx.fee_paid(|_| Ok::<_, BalanceError>(Amount::zero())).unwrap();
    println!("fee {:?}", fee);

    let r = broadcast(&mut client, 2614161, &txb).await?;
    tracing::info!("{}", r);
    Ok(txb)
}

pub async fn broadcast_garbage() -> Result<()> {
    let mut zec = CoinDef::from_network(zcash_primitives::consensus::Network::MainNetwork);
    zec.set_url(&dotenv::var("LWD_URL").unwrap());
    let mut client = zec.connect_lwd().await?;
    let r = broadcast(&mut client, 2614161, &[1, 2, 3, 4]).await?;
    println!("{r}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv()?;
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // account_tests()?;
    // cli_main().await?;
    let _tx = test_payment().await?;
    // broadcast_garbage().await?;
    Ok(())
}

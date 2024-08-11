use anyhow::Result;
use rand::rngs::OsRng;
use tracing::info;
use tracing_subscriber::FmtSubscriber;
use zcash_primitives::{merkle_tree::MerklePath, sapling::Node};
use zcash_warp::{
    db::{
        create_new_account, detect_key, get_account_info, get_witnesses_v1, init_db, list_accounts, store_received_note,
    },
    generate_random_mnemonic_phrase,
    lwd::{get_compact_block_range, get_tree_state},
    types::PoolMask,
    warp::{
        hasher::{empty_roots, OrchardHasher, SaplingHasher},
        sync::{OrchardSync, SaplingSync},
    },
    COINS,
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

    // Test new account
    let phrase = generate_random_mnemonic_phrase(OsRng);
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

pub async fn test_block_decryption() -> Result<()> {
    let mut zec = COINS[0].lock();
    zec.set_db_path("/Users/hanhhuynhhuu/zec.db")?;
    zec.set_url("http://172.16.11.208:9067");
    let mut client = zec.connect_lwd().await?;

    let height = 2217459;
    let end = 2576848;
    let connection = zec.connection()?;

    let (sapling_state, orchard_state) = get_tree_state(&mut client, height - 1).await?;

    let sap_hasher = SaplingHasher::default();
    let mut sap_dec = SaplingSync::new(
        &zec.network,
        &connection,
        height,
        sapling_state.size() as u32,
        sapling_state.to_edge(&sap_hasher),
    )?;

    let orch_hasher = OrchardHasher::default();
    let mut orch_dec = OrchardSync::new(
        &zec.network,
        &connection,
        height,
        orchard_state.size() as u32,
        orchard_state.to_edge(&orch_hasher),
    )?;

    // let block = get_compact_block(&mut client, height).await?;
    let mut blocks = get_compact_block_range(&mut client, height, end).await?;
    let mut bs = vec![];
    while let Some(block) = blocks.message().await? {
        let height = block.height;
        bs.push(block);
        if bs.len() == 100000 {
            info!("Height {}", height);
            sap_dec.add(&bs)?;
            orch_dec.add(&bs)?;
            bs.clear();
        }
    }
    sap_dec.add(&bs)?;
    orch_dec.add(&bs)?;

    store_received_note(&connection, &*sap_dec.notes)?;
    store_received_note(&connection, &*orch_dec.notes)?;

    let (sap_state, orch_state) = get_tree_state(&mut client, end).await?;
    let sapling = sap_state.to_edge(&sap_hasher);
    let root = sapling.root(&sap_hasher);
    info!("sapling {}", hex::encode(&root));
    let orchard = orch_state.to_edge(&orch_hasher);
    let root = orchard.root(&orch_hasher);
    info!("orchard {}", hex::encode(&root));

    for n in sap_dec.notes.iter() {
        info!("{}", serde_json::to_string(n).unwrap());
    }
    for n in orch_dec.notes.iter() {
        info!("{}", serde_json::to_string(n).unwrap());
    }
    Ok(())
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber)?;

    test_block_decryption().await?;
    Ok(())
}

async fn test_witness_migration() -> Result<()> {
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

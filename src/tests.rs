use anyhow::Result;
use rand::rngs::OsRng;
use tracing::info;
use zcash_primitives::{merkle_tree::MerklePath, sapling::Node};

use crate::{
    coin::COINS,
    db::{
        account::{get_account_info, list_accounts},
        account_manager::{create_new_account, detect_key},
        migration::init_db,
        witnesses::get_witnesses_v1,
    },
    keys::generate_random_mnemonic_phrase,
    lwd::get_tree_state,
    types::PoolMask,
    warp::hasher::{empty_roots, SaplingHasher},
};

pub fn account_tests() -> Result<()> {
    let mut zec = COINS[0].lock();
    zec.set_db_path("/Users/hanhhuynhhuu/zec.db")?;
    let connection = zec.connection()?;
    init_db(&connection)?;
    let accounts = list_accounts(&connection)?;
    println!("{:?}", &accounts);

    generate_random_mnemonic_phrase(OsRng);
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

use std::time::Instant;

use anyhow::Result;
use orchard::keys::Scope;
use rand::rngs::OsRng;
use tracing::info;
use tracing_subscriber::FmtSubscriber;
use zcash_note_encryption::{EphemeralKeyBytes, COMPACT_NOTE_SIZE};
use zcash_primitives::{
    consensus::BlockHeight, merkle_tree::MerklePath, sapling::{
        keys::PreparedIncomingViewingKey, note::ExtractedNoteCommitment, note_encryption::try_sapling_compact_note_decryption, Node
    }, transaction::components::sapling::CompactOutputDescription
};
use zcash_warp::{
    db::{create_new_account, detect_key, get_account_info, get_witnesses_v1, init_db, list_accounts},
    generate_random_mnemonic_phrase,
    lwd::{get_compact_block, get_tree_state},
    types::PoolMask,
    warp::{hasher::{empty_roots, SaplingHasher}, try_orchard_decrypt, try_sapling_decrypt},
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
    zec.set_url("https://lwd1.zcash-infra.com:9067");
    let mut client = zec.connect_lwd().await?;

    let block = get_compact_block(&mut client, 2217459).await?;
    let connection = zec.connection()?;
    let ai = get_account_info(&zec.network, &connection, 1)?;
    let ivk = ai.sapling.vk.fvk.vk.ivk();

    let outputs = block
        .vtx
        .iter()
        .flat_map(|vtx| {
            vtx.outputs.iter().map(|o| {
                (
                    <[u8; 32]>::try_from(o.epk.clone()).unwrap(),
                    &o.ciphertext[..],
                )
            })
        })
        .collect::<Vec<_>>();

    let a = Instant::now();
    try_sapling_decrypt(&[ivk.clone()], &outputs)?;
    println!("{:?}", a.elapsed());

    let a = Instant::now();
    let pvk = PreparedIncomingViewingKey::new(&ivk);
    for vtx in block.vtx.iter() {
        for co in vtx.outputs.iter() {
            let mut cco = CompactOutputDescription {
                ephemeral_key: EphemeralKeyBytes(co.epk.clone().try_into().unwrap()),
                cmu: ExtractedNoteCommitment::from_bytes(&co.cmu.clone().try_into().unwrap()).unwrap(),
                enc_ciphertext: [0u8; COMPACT_NOTE_SIZE],
            };
            cco.enc_ciphertext.copy_from_slice(&co.ciphertext);
            if let Some((note, _pa)) = try_sapling_compact_note_decryption(&zec.network, BlockHeight::from(2217459), &pvk, &cco) {
                println!("{}", note.value().inner());
            }
        }
    }
    println!("{:?}", a.elapsed());

    let block = get_compact_block(&mut client, 2376624).await?;
    let actions = block
        .vtx
        .iter()
        .flat_map(|vtx| {
            vtx.actions.iter().map(|a| {
                (
                    <[u8; 32]>::try_from(a.ephemeral_key.clone()).unwrap(),
                    &a.ciphertext[..],
                )
            })
        })
        .collect::<Vec<_>>();

    let ivk = ai.orchard.unwrap().vk.to_ivk(Scope::External);
    try_orchard_decrypt(&[ivk.clone()], &actions)?;

    Ok(())
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber)?;

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


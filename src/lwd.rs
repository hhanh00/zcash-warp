use anyhow::Result;
use rpc::{BlockId, BlockRange, CompactBlock, TreeState};
use tonic::{Request, Streaming};

use crate::{warp::legacy::CommitmentTreeFrontier, Client};

#[path = "./generated/cash.z.wallet.sdk.rpc.rs"]
pub mod rpc;

pub async fn get_tree_state(
    client: &mut Client,
    height: u32,
) -> Result<(CommitmentTreeFrontier, CommitmentTreeFrontier)> {
    let tree_state = client
        .get_tree_state(Request::new(BlockId {
            height: height as u64,
            hash: vec![],
        }))
        .await?
        .into_inner();

    let TreeState {
        sapling_tree,
        orchard_tree,
        ..
    } = tree_state;

    fn decode_tree_state(s: &str) -> CommitmentTreeFrontier {
        let tree = hex::decode(s).unwrap();
        CommitmentTreeFrontier::read(&*tree).unwrap()
    }

    let sapling = decode_tree_state(&sapling_tree);
    let orchard = decode_tree_state(&orchard_tree);

    #[cfg(test)]
    {
        use crate::warp::hasher::SaplingHasher;
        use zcash_primitives::{merkle_tree::CommitmentTree, sapling::Node};

        let st = hex::decode(&sapling_tree).unwrap();
        let st = CommitmentTree::<Node>::read(&*st)?;
        let root1 = st.root();
        println!("{}", hex::encode(&root1.repr));
        let s_hasher = SaplingHasher::default();
        let edge = sapling.to_edge(&s_hasher);
        let root2 = edge.root(&s_hasher);
        println!("{}", hex::encode(&root2));
        assert_eq!(root1.repr, root2);
    }

    Ok((sapling, orchard))
}

pub async fn get_compact_block(client: &mut Client, height: u32) -> Result<CompactBlock> {
    let mut blocks = client
        .get_block_range(Request::new(BlockRange {
            start: Some(BlockId {
                height: height as u64,
                hash: vec![],
            }),
            end: Some(BlockId {
                height: height as u64,
                hash: vec![],
            }),
            spam_filter_threshold: 0,
        }))
        .await?
        .into_inner();
    while let Some(block) = blocks.message().await? {
        return Ok(block);
    }
    Err(anyhow::anyhow!("No block found"))
}

pub async fn get_compact_block_range(client: &mut Client, start: u32, end: u32) -> Result<Streaming<CompactBlock>> {
    let blocks = client.get_block_range(Request::new(BlockRange {
        start: Some(BlockId {
            height: start as u64,
            hash: vec![],
        }),
        end: Some(BlockId {
            height: end as u64,
            hash: vec![],
        }),
        spam_filter_threshold: 0,
    })).await?.into_inner();
    Ok(blocks)
}

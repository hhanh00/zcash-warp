use crate::{
    coin::{connect_lwd, CoinDef},
    db::{
        notes::{
            get_block_header, mark_shielded_spent, mark_transparent_spent, rewind_checkpoint,
            store_block, store_received_note, store_utxo, update_tx_timestamp,
        },
        tx::add_tx_value,
    },
    lwd::{get_compact_block_range, get_transparent, get_tree_state},
    txdetails::CompressedMemo,
    warp::{
        hasher::{OrchardHasher, SaplingHasher},
        BlockHeader,
    },
    Hash,
};
use anyhow::Result;
use header::BlockHeaderStore;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use thiserror::Error;
use tracing::info;
use transparent::TransparentSync;

use super::Witness;

mod header;
mod orchard;
mod sapling;
mod transparent;
pub mod builder;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Reorganization detected at block {0}")]
    Reorg(u32),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct ReceivedTx {
    pub id: u32,
    pub account: u32,
    pub height: u32,
    #[serde(with = "hex")]
    pub txid: Hash,
    pub timestamp: u32,
    pub ivtx: u32,
    pub value: i64,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct ExtendedReceivedTx {
    pub rtx: ReceivedTx,
    pub address: Option<String>,
    pub memo: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct TxValueUpdate<IDSpent: std::fmt::Debug> {
    pub id_tx: u32,
    pub account: u32,
    pub height: u32,
    pub txid: Hash,
    pub value: i64,
    pub id_spent: Option<IDSpent>,
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlainNote {
    #[serde(with = "serde_bytes")]
    pub address: [u8; 43],
    pub value: u64,
    #[serde(with = "serde_bytes")]
    pub rcm: Hash,
    #[serde(with = "serde_bytes")]
    pub rho: Option<Hash>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FullPlainNote {
    pub note: PlainNote,
    pub memo: CompressedMemo,
    pub incoming: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReceivedNote {
    pub is_new: bool,
    pub id: u32,
    pub account: u32,
    pub position: u32,
    pub height: u32,
    #[serde(with = "serde_bytes")]
    pub address: [u8; 43],
    pub value: u64,
    pub rcm: Hash,
    pub nf: Hash,
    pub rho: Option<Hash>,
    pub vout: u32,
    pub tx: ReceivedTx,
    pub spent: Option<u32>,
    pub witness: Witness,
}

pub use orchard::Synchronizer as OrchardSync;
pub use sapling::Synchronizer as SaplingSync;

pub async fn warp_sync(coin: &CoinDef, start: u32, end: u32) -> Result<(), SyncError> {
    tracing::info!("{}-{}", start, end);
    let mut connection = coin.connection()?;
    let mut client = coin.connect_lwd().await?;
    let (sapling_state, orchard_state) = get_tree_state(&mut client, start).await?;

    let sap_hasher = SaplingHasher::default();
    let mut sap_dec = SaplingSync::new(
        &coin.network,
        &connection,
        start,
        sapling_state.size() as u32,
        sapling_state.to_edge(&sap_hasher),
    )?;

    let orch_hasher = OrchardHasher::default();
    let mut orch_dec = OrchardSync::new(
        &coin.network,
        &connection,
        start,
        orchard_state.size() as u32,
        orchard_state.to_edge(&orch_hasher),
    )?;

    let mut trp_dec = TransparentSync::new(&coin.network, &connection, start)?;

    let addresses = trp_dec.addresses.clone();
    for (account, taddr) in addresses.into_iter() {
        let txs = get_transparent(&coin.network, &mut client, account, taddr, start, end).await?;
        trp_dec.process_txs(&txs)?;
    }
    let heights = trp_dec
        .txs
        .iter()
        .map(|(tx, _, _)| tx.height)
        .collect::<Vec<_>>();
    let mut header_dec = BlockHeaderStore::new();
    header_dec.add_heights(&heights)?;

    let bh = get_block_header(&connection, start)?;
    let mut prev_hash = bh.hash;

    let mut block_client = connect_lwd(&coin.warp).await?;
    let mut blocks = get_compact_block_range(&mut block_client, start + 1, end).await?;
    let mut bs = vec![];
    let mut bh = BlockHeader::default();
    let mut c = 0;
    while let Some(block) = blocks.message().await.map_err(anyhow::Error::new)? {
        tracing::info!("{}", block.height);
        bh = BlockHeader {
            height: block.height as u32,
            hash: block.hash.clone().try_into().unwrap(),
            prev_hash: block.prev_hash.clone().try_into().unwrap(),
            timestamp: block.time,
        };
        if prev_hash != bh.prev_hash {
            rewind_checkpoint(&connection)?;
            return Err(SyncError::Reorg(bh.height));
        }
        prev_hash = bh.hash;

        header_dec.process(&bh)?;
        for vtx in block.vtx.iter() {
            c += vtx.outputs.len();
            c += vtx.actions.len();
            for b in [&vtx.sapling_bridge, &vtx.orchard_bridge] {
                if let Some(b) = b {
                    c += b.len as usize;
                }
            }
        }

        let height = block.height;
        bs.push(block);

        if c >= 1000000 {
            info!("Height {}", height);
            sap_dec.add(&bs)?;
            // orch_dec.add(&bs)?;
            bs.clear();
            c = 0;
        }
    }
    sap_dec.add(&bs)?;
    // orch_dec.add(&bs)?;

    let (s, o) = get_tree_state(&mut client, bh.height as u32).await?;
    let r = s.to_edge(&sap_dec.hasher).root(&sap_dec.hasher);
    info!("s_root {}", hex::encode(&r));
    let r = o.to_edge(&orch_dec.hasher).root(&orch_dec.hasher);
    info!("o_root {}", hex::encode(&r));

    if bh.height != 0 {
        let db_tx = connection.transaction().map_err(anyhow::Error::new)?;

        store_received_note(&db_tx, bh.height, &*sap_dec.notes)?;
        for s in sap_dec.spends.iter() {
            add_tx_value(&db_tx, s)?;
            mark_shielded_spent(&db_tx, s)?;
        }

        store_received_note(&db_tx, bh.height, &*orch_dec.notes)?;
        for s in orch_dec.spends.iter() {
            add_tx_value(&db_tx, s)?;
            mark_shielded_spent(&db_tx, s)?;
        }

        for utxo in trp_dec.utxos.iter() {
            store_utxo(&db_tx, utxo)?;
        }
        for s in trp_dec.tx_updates.iter() {
            add_tx_value(&db_tx, &s)?;
            if s.id_spent.is_some() {
                mark_transparent_spent(&db_tx, s)?;
            }
        }

        update_tx_timestamp(&db_tx, header_dec.heights.values())?;

        store_block(&db_tx, &bh)?;
        db_tx.commit().map_err(anyhow::Error::new)?;
    }

    Ok(())
}

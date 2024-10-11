use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
};

use crate::{
    coin::{connect_lwd, CoinDef},
    db::{
        account::list_account_transparent_addresses,
        chain::{get_block_header, get_sync_height, rewind_checkpoint, store_block},
        notes::{
            mark_shielded_spent, store_received_note, update_account_balances, update_tx_timestamp,
        },
        tx::{
            add_tx_value, copy_block_times_from_tx, drop_transparent_data,
            list_unknown_height_timestamps, store_block_time, update_tx_time, update_tx_values,
        },
    },
    fb_unwrap,
    lwd::{
        get_compact_block, get_compact_block_range, get_transparent, get_tree_state,
        rpc::CompactBlock,
    },
    network::Network,
    txdetails::CompressedMemo,
    types::CheckpointHeight,
    utils::chain::{get_activation_height, reset_chain},
    warp::{
        hasher::{OrchardHasher, SaplingHasher},
        BlockHeader,
    },
    Client, Hash,
};
use anyhow::Result;
use header::BlockHeaderStore;
use lazy_static::lazy_static;
use prost::Message;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{
    mpsc::{channel, Sender},
    Semaphore,
};
use tonic::transport::Channel;
use tracing::info;
use transparent::TransparentSync;
use zcash_keys::encoding::AddressCodec;
use zcash_primitives::legacy::TransparentAddress;
use zip::unstable::{LittleEndianReadExt, LittleEndianWriteExt};

use super::Witness;

use crate::{
    coin::COINS,
    ffi::{map_result, CResult},
};
use std::ffi::{c_char, CStr};

use warp_macros::c_export;

pub mod builder;
mod header;
mod orchard;
mod sapling;
mod transparent;

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
    pub contact: Option<String>,
    pub memo: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct TxValueUpdate {
    pub id_tx: u32,
    pub account: u32,
    pub height: u32,
    pub timestamp: u32,
    pub txid: Hash,
    pub value: i64,
    // pub id_spent: Option<IDSpent>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct IdSpent<NoteRef> {
    pub id_note: u32, // note or utxo
    pub account: u32,
    pub height: u32,
    pub txid: Hash,
    pub note_ref: NoteRef,
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlainNote {
    pub id: u32,
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

#[c_export]
pub async fn download_warp_blocks(
    network: &Network,
    warp_url: &str,
    end: u32,
    dest: &str,
) -> Result<()> {
    tracing::info!("warp url {warp_url}");
    let mut client = connect_lwd(warp_url).await?;
    let dest = File::create(dest)?;
    let mut dest = BufWriter::new(dest);
    let start = get_activation_height(network)?;
    let mut blocks = get_compact_block_range(&mut client, start + 1, end - 1).await?;
    while let Some(block) = blocks.message().await? {
        let v = block.encode_to_vec();
        dest.write_u32_le(v.len() as u32)?;
        dest.write_all(&*v)?;
    }
    Ok(())
}

pub trait CompactBlockSource: Clone {
    fn chunked(&self) -> bool;

    fn run(self, start: u32, end: u32, sender: Sender<CompactBlock>) -> Result<()>;
}

#[derive(Clone)]
pub struct LWDCompactBlockSource {
    channel: Channel,
}

impl LWDCompactBlockSource {
    pub fn new(channel: Channel) -> Result<Self> {
        Ok(Self { channel })
    }
}

impl CompactBlockSource for LWDCompactBlockSource {
    fn chunked(&self) -> bool {
        true
    }

    fn run(self, start: u32, end: u32, sender: Sender<CompactBlock>) -> Result<()> {
        tokio::spawn(async move {
            let mut client = Client::new(self.channel.clone());
            let mut range = get_compact_block_range(&mut client, start + 1, end).await?;
            while let Some(block) = range.message().await? {
                sender.send(block).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        Ok(())
    }
}

pub async fn warp_sync<BS: CompactBlockSource + 'static>(
    coin: &CoinDef,
    start: CheckpointHeight,
    end: u32,
    source: BS,
) -> Result<(), SyncError> {
    tracing::info!("{:?}-{}", start, end);
    let permit = SYNC_LOCK.acquire().await;
    if !permit.is_ok() {
        return Ok(());
    }
    let mut connection = coin.connection()?;
    let mut client = coin.connect_lwd()?;
    let (sapling_state, orchard_state) = get_tree_state(&mut client, start.into()).await?;

    let sap_hasher = SaplingHasher::default();
    let mut sap_dec = SaplingSync::new(
        coin,
        &coin.network,
        &connection,
        start,
        sapling_state.size() as u32,
        sapling_state.to_edge(&sap_hasher),
    )?;

    let orch_hasher = OrchardHasher::default();
    let mut orch_dec = OrchardSync::new(
        coin,
        &coin.network,
        &connection,
        start,
        orchard_state.size() as u32,
        orchard_state.to_edge(&orch_hasher),
    )?;

    let mut trp_dec = TransparentSync::new(&coin.network, &connection)?;

    let addresses = trp_dec.addresses.clone();
    for (account, addr_index, taddr) in addresses.into_iter() {
        let txs = get_transparent(
            &coin.network,
            &mut client,
            account,
            addr_index,
            taddr,
            start.0 + 1,
            end,
        )
        .await?;
        let address = taddr.encode(&coin.network);
        trp_dec.process_txs(&address, &txs)?;
    }
    let heights = trp_dec
        .txs
        .iter()
        .map(|(tx, _, _)| tx.height)
        .collect::<Vec<_>>();
    let mut header_dec = BlockHeaderStore::new();
    header_dec.add_heights(&heights)?;

    let bh = get_block_header(&connection, start.into())?;
    let mut prev_hash = bh.hash;

    let mut bs = vec![];
    let mut bh = BlockHeader::default();
    let mut c = 0;
    let chunked = source.chunked();
    let (block_sender, mut block_recv) = channel::<CompactBlock>(20);
    source.run(start.0, end, block_sender)?;
    while let Some(block) = block_recv.recv().await {
        bh = BlockHeader {
            height: block.height as u32,
            hash: block.hash.clone().try_into().unwrap(),
            prev_hash: block.prev_hash.clone().try_into().unwrap(),
            timestamp: block.time,
        };
        if prev_hash != bh.prev_hash {
            rewind_checkpoint(&coin.network, &mut connection, &mut client).await?;
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
            orch_dec.add(&bs)?;
            bs.clear();
            c = 0;
            if chunked {
                break;
            }
        }
    }
    sap_dec.add(&bs)?;
    orch_dec.add(&bs)?;

    // Verification
    let (s, o) = get_tree_state(&mut client, CheckpointHeight(bh.height as u32)).await?;
    let r = s.to_edge(&sap_dec.hasher).root(&sap_dec.hasher);
    let r2 = sap_dec.tree_state.root(&sap_dec.hasher);
    info!("s_root {}", hex::encode(&r));
    assert_eq!(r, r2);
    let r = o.to_edge(&orch_dec.hasher).root(&orch_dec.hasher);
    let r2 = orch_dec.tree_state.root(&orch_dec.hasher);
    assert_eq!(r, r2);
    info!("o_root {}", hex::encode(&r));

    if bh.height != 0 {
        let db_tx = connection.transaction().map_err(anyhow::Error::new)?;

        store_received_note(&db_tx, bh.height, &*sap_dec.notes)?;
        for (tx_value, spend) in sap_dec.spends.iter() {
            add_tx_value(&db_tx, tx_value)?;
            mark_shielded_spent(&db_tx, spend)?;
        }

        store_received_note(&db_tx, bh.height, &*orch_dec.notes)?;
        for (tx_value, spend) in orch_dec.spends.iter() {
            add_tx_value(&db_tx, tx_value)?;
            mark_shielded_spent(&db_tx, spend)?;
        }

        trp_dec.flush(&db_tx)?;

        update_tx_timestamp(&db_tx, header_dec.heights.values())?;

        store_block(&db_tx, &bh)?;
        update_account_balances(&db_tx, bh.height)?;

        // Save block times
        header_dec.save(&db_tx)?;
        copy_block_times_from_tx(&db_tx)?;

        db_tx.commit().map_err(anyhow::Error::new)?;
    }

    Ok(())
}

#[c_export]
pub async fn warp_synchronize(coin: &CoinDef, end_height: u32) -> Result<()> {
    let mut connection = coin.connection()?;
    let start_height = get_sync_height(&connection)?;
    if start_height == 0 {
        let activation_height = get_activation_height(&coin.network)?;
        let mut client = coin.connect_lwd()?;
        reset_chain(
            &coin.network,
            &mut *connection,
            &mut client,
            activation_height,
        )
        .await?;
    }
    if start_height < end_height {
        let end_height = (start_height + 100_000).min(end_height);
        let channel = if end_height < coin.config.warp_end_height {
            let url = fb_unwrap!(coin.config.warp_url);
            let ep = Channel::from_shared(url.clone()).unwrap();
            ep.connect().await?
        } else {
            fb_unwrap!(coin.channel).clone()
        };
        let bs = LWDCompactBlockSource::new(channel)?;
        warp_sync(&coin, CheckpointHeight(start_height), end_height, bs).await?;
    }
    Ok(())
}

#[derive(Clone)]
struct FileCompactBlockSource {
    file: String,
}

impl CompactBlockSource for FileCompactBlockSource {
    fn chunked(&self) -> bool {
        false
    }

    fn run(self, _start: u32, _end: u32, sender: Sender<CompactBlock>) -> Result<()> {
        tokio::spawn(async move {
            let file = File::open(self.file)?;
            let mut reader = BufReader::new(file);
            while let Ok(size) = reader.read_u32_le() {
                let mut buf = vec![0u8; size as usize];
                reader.read_exact(&mut buf)?;
                let block = CompactBlock::decode(&*buf)?;
                sender.send(block).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        Ok(())
    }
}

#[c_export]
pub async fn warp_synchronize_from_file(coin: &CoinDef, file: &str) -> Result<()> {
    let source = FileCompactBlockSource {
        file: file.to_string(),
    };
    let activation = get_activation_height(&coin.network)?;
    {
        let mut connection = coin.connection()?;
        let mut client = coin.connect_lwd()?;
        reset_chain(&coin.network, &mut connection, &mut client, activation).await?;
    }
    warp_sync(
        &coin,
        CheckpointHeight(activation),
        coin.config.warp_end_height,
        source,
    )
    .await?;
    Ok(())
}

#[c_export]
pub async fn transparent_scan(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
    account: u32,
    end_height: u32,
) -> Result<()> {
    drop_transparent_data(connection, account)?;
    let mut trp_dec = TransparentSync::new(network, connection)?;
    let addresses = list_account_transparent_addresses(connection, account)?;
    let start = get_activation_height(network)?;
    if start >= end_height {
        return Ok(());
    }
    let db_tx = connection.transaction()?;
    for a in addresses {
        let taddr = a.address.as_deref().unwrap();
        let address = TransparentAddress::decode(network, taddr)?;
        let txs = get_transparent(
            network,
            client,
            account,
            a.addr_index,
            address,
            start,
            end_height,
        )
        .await?;
        trp_dec.process_txs(taddr, &*txs)?;
    }
    trp_dec.flush(&db_tx)?;
    update_tx_values(&db_tx)?;

    // there may be some block heights for which we don't have the time
    update_tx_time(&db_tx)?;

    // fetch the missing heights
    let heights = list_unknown_height_timestamps(&db_tx)?;
    for h in heights {
        let cb = get_compact_block(client, h).await?;
        let timestamp = cb.time;
        store_block_time(&db_tx, h, timestamp)?;
    }
    // try again
    update_tx_time(&db_tx)?;
    db_tx.commit()?;

    Ok(())
}

lazy_static! {
    static ref SYNC_LOCK: Arc<Semaphore> = Arc::new(Semaphore::new(1));
}

use std::time::Instant;

use anyhow::Result;
use rpc::{
    BlockId, BlockRange, CompactBlock, Empty, GetAddressUtxosArg, RawTransaction,
    TransparentAddressBlockFilter, TreeState, TxFilter,
};
use tokio::runtime::Handle;
use tonic::{Request, Streaming};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_primitives::{
    consensus::{BlockHeight, BranchId},
    legacy::TransparentAddress,
    transaction::Transaction,
};

use crate::{
    coin::{connect_lwd, CoinDef},
    data::fb::TransactionBytesT,
    network::Network,
    types::CheckpointHeight,
    utils::ContextExt as _,
    warp::{legacy::CommitmentTreeFrontier, OutPoint, TransparentTx, TxOut2, UTXO},
    Client,
};

#[path = "./generated/cash.z.wallet.sdk.rpc.rs"]
pub mod rpc;

pub async fn get_last_height(client: &mut Client) -> Result<u32> {
    let r = client
        .get_lightd_info(Request::new(Empty {}))
        .await?
        .into_inner();
    Ok(r.block_height as u32)
}

pub async fn get_tree_state(
    client: &mut Client,
    height: CheckpointHeight,
) -> Result<(CommitmentTreeFrontier, CommitmentTreeFrontier)> {
    let height: u32 = height.into();
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
        if s.is_empty() {
            CommitmentTreeFrontier::default()
        } else {
            let tree = hex::decode(s).unwrap();
            CommitmentTreeFrontier::read(&*tree).unwrap()
        }
    }

    let sapling = decode_tree_state(&sapling_tree);
    let orchard = decode_tree_state(&orchard_tree);

    #[cfg(test)]
    {
        // use crate::warp::hasher::SaplingHasher;
        // use sapling_crypto::{CommitmentTree, Node};

        // let st = hex::decode(&sapling_tree).unwrap();
        // let st = CommitmentTree::read(&*st)?;
        // let root1 = st.root();
        // println!("{}", hex::encode(&root1.repr));
        // let s_hasher = SaplingHasher::default();
        // let edge = sapling.to_edge(&s_hasher);
        // let root2 = edge.root(&s_hasher);
        // println!("{}", hex::encode(&root2));
        // assert_eq!(root1.repr, root2);
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

pub async fn get_compact_block_range(
    client: &mut Client,
    start: u32,
    end: u32,
) -> Result<Streaming<CompactBlock>> {
    let req = || {
        Request::new(BlockRange {
            start: Some(BlockId {
                height: start as u64,
                hash: vec![],
            }),
            end: Some(BlockId {
                height: end as u64,
                hash: vec![],
            }),
            spam_filter_threshold: 0,
        })
    };
    let blocks = client.get_block_range(req()).await?.into_inner();
    Ok(blocks)
}

pub async fn get_transparent(
    network: &Network,
    client: &mut Client,
    account: u32,
    external: u32,
    addr_index: u32,
    taddr: TransparentAddress,
    start: u32,
    end: u32,
) -> Result<Vec<TransparentTx>> {
    let taddr_string = taddr.encode(network);
    tracing::info!("get_transparent {taddr_string}");
    let mut txs = client
        .get_taddress_txids(Request::new(TransparentAddressBlockFilter {
            address: taddr_string,
            range: Some(BlockRange {
                start: Some(BlockId {
                    height: start as u64,
                    hash: vec![],
                }),
                end: Some(BlockId {
                    height: end as u64,
                    hash: vec![],
                }),
                spam_filter_threshold: 0,
            }),
        }))
        .await?
        .into_inner();
    let mut ttxs = vec![];
    while let Some(raw_tx) = txs.message().await? {
        let height = raw_tx.height as u32;
        let raw_tx = raw_tx.data;
        let branch_id = BranchId::for_height(network, BlockHeight::from_u32(height));
        let tx = Transaction::read(&*raw_tx, branch_id)?;
        let transparent_bundle = tx.transparent_bundle().unwrap();
        let mut vins = vec![];
        for vin in transparent_bundle.vin.iter() {
            let prev_out = crate::warp::OutPoint {
                txid: vin.prevout.hash().clone(),
                vout: vin.prevout.n(),
            };
            vins.push(prev_out);
        }
        let mut vouts = vec![];
        for (vout, txout) in transparent_bundle.vout.iter().enumerate() {
            if let Some(address) = txout.recipient_address() {
                if address == taddr {
                    let out = crate::warp::TxOut {
                        address: txout.recipient_address(),
                        value: txout.value.into(),
                        vout: vout as u32,
                    };
                    vouts.push(out);
                }
            }
        }
        let ttx = TransparentTx {
            account,
            height,
            external,
            addr_index,
            address: taddr.clone(),
            timestamp: 0,
            txid: tx.txid().as_ref().clone().try_into().unwrap(),
            vins,
            vouts,
        };
        ttxs.push(ttx);
    }

    Ok(ttxs)
}

pub async fn broadcast(client: &mut Client, height: u32, tx: &TransactionBytesT) -> Result<String> {
    let bb = tx.data.as_ref();
    let res = client
        .send_transaction(Request::new(RawTransaction {
            data: bb.cloned().unwrap_or_default(),
            height: height as u64,
        }))
        .await?
        .into_inner();
    Ok(res.error_message)
}

pub fn get_txin_coins(coin: &CoinDef, network: Network, ops: Vec<OutPoint>) -> Result<Vec<TxOut2>> {
    tokio::task::block_in_place(move || {
        Handle::current().block_on(async move {
            let mut client = coin.connect_lwd()?;
            let mut txouts = vec![];
            for op in ops {
                let tx = client
                    .get_transaction(Request::new(TxFilter {
                        block: None,
                        index: 0,
                        hash: op.txid.to_vec(),
                    }))
                    .await
                    .with_file_line(|| "get_transaction")?
                    .into_inner();
                let data = &*tx.data;
                let tx = Transaction::read(data, BranchId::Nu5)?;
                let tx_data = tx.into_data();
                let b = tx_data
                    .transparent_bundle()
                    .ok_or(anyhow::anyhow!("No T bundle"))?;
                let txout = &b.vout[op.vout as usize];
                let txout = TxOut2 {
                    address: txout.recipient_address().map(|o| o.encode(&network)),
                    value: txout.value.into(),
                    vout: op.vout,
                };
                txouts.push(txout);
            }
            Ok(txouts)
        })
    })
}

pub async fn get_transaction(
    network: &Network,
    client: &mut Client,
    txid: &[u8],
) -> Result<(u32, Transaction)> {
    let tx = client
        .get_transaction(Request::new(TxFilter {
            block: None,
            index: 0,
            hash: txid.to_vec(),
        }))
        .await
        .with_file_line(|| format!("txid {}", hex::encode(txid)))?
        .into_inner();
    let height = tx.height as u32;
    let tx = Transaction::read(
        &*tx.data,
        BranchId::for_height(network, BlockHeight::from_u32(height)),
    )?;
    Ok((height, tx))
}

pub async fn get_utxos(
    client: &mut Client,
    account: u32,
    external: u32,
    addr_index: u32,
    address: &str,
) -> Result<Vec<UTXO>> {
    let mut utxos = vec![];
    let mut utxo_reps = client
        .get_address_utxos_stream(Request::new(GetAddressUtxosArg {
            addresses: vec![address.to_string()],
            start_height: 1,
            max_entries: u32::MAX,
        }))
        .await?
        .into_inner();
    while let Some(utxo) = utxo_reps.message().await? {
        let utxo = UTXO {
            is_new: true,
            id: 0,
            account,
            external,
            addr_index,
            height: utxo.height as u32,
            timestamp: 0, // no need to retrieve block timestamp for a sweep
            txid: utxo.txid.try_into().unwrap(),
            vout: utxo.index as u32,
            address: utxo.address,
            value: utxo.value_zat as u64,
        };
        utxos.push(utxo);
    }
    Ok(utxos)
}

pub async fn ping(#[allow(unused_variables)] network: &Network, lwd_url: &str) -> Result<u64> {
    let start = Instant::now();
    let mut client = connect_lwd(lwd_url).await?;
    client.get_lightd_info(Request::new(Empty {})).await?;
    let elapsed = Instant::now().duration_since(start);
    Ok(elapsed.as_millis() as u64)
}

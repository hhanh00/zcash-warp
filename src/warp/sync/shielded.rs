use rusqlite::Connection;
use std::marker::PhantomData;
use std::sync::mpsc::Sender;
use std::{collections::HashMap, mem::swap, sync::mpsc::channel};

use crate::coin::CoinDef;
use crate::db::notes::list_all_received_notes;
use crate::lwd::rpc::CompactTx;
use crate::network::Network;
use crate::warp::sync::IdSpent;
use crate::{
    db::account::{get_account_info, list_accounts},
    lwd::rpc::{Bridge, CompactBlock},
    types::{AccountInfo, CheckpointHeight},
    Hash,
};
use anyhow::Result;
use rayon::prelude::*;
use tracing::info;

use crate::warp::{Edge, Hasher, MERKLE_DEPTH};

use super::{ReceivedNote, TxValueUpdate};

pub mod orchard;
pub mod sapling;

pub trait ShieldedProtocol {
    type Hasher: Hasher;
    type IVK: Sync;
    type Spend;
    type Output: Sync;

    fn is_orchard() -> bool;

    fn extract_ivk(ai: &AccountInfo) -> Option<(u32, Self::IVK)>;
    fn extract_inputs(tx: &CompactTx) -> &Vec<Self::Spend>;
    fn extract_outputs(tx: &CompactTx) -> &Vec<Self::Output>;
    fn extract_bridge(tx: &CompactTx) -> Option<&Bridge>;

    fn extract_nf(i: &Self::Spend) -> Hash;
    fn extract_cmx(o: &Self::Output) -> Hash;

    fn try_decrypt(
        network: &Network,
        ivks: &[(u32, Self::IVK)],
        height: u32,
        time: u32,
        ivtx: u32,
        vout: u32,
        output: &Self::Output,
        sender: &mut Sender<ReceivedNote>,
    ) -> Result<()>;
    fn finalize_received_note(txid: Hash, note: &mut ReceivedNote, ai: &AccountInfo) -> Result<()>;
}

#[derive(Debug)]
pub struct Synchronizer<P: ShieldedProtocol> {
    pub hasher: P::Hasher,
    pub network: Network,
    pub account_infos: Vec<AccountInfo>,
    pub start: u32,
    pub notes: Vec<ReceivedNote>,
    pub spends: Vec<(TxValueUpdate, IdSpent<Hash>)>,
    pub position: u32,
    pub tree_state: Edge,
    pub _data: PhantomData<P>,
}

#[derive(Debug)]
struct BridgeExt<'a> {
    b: &'a Bridge,
    s: i32,
    e: i32,
}

impl<P: ShieldedProtocol> Synchronizer<P> {
    pub fn new(
        coin: &CoinDef,
        network: &Network,
        connection: &Connection,
        start: CheckpointHeight,
        position: u32,
        tree_state: Edge,
    ) -> Result<Self> {
        let accounts = list_accounts(coin, connection)?.items.unwrap();
        let mut account_infos = vec![];
        for a in accounts {
            let ai = get_account_info(network, connection, a.id)?;
            account_infos.push(ai);
        }
        let is_orchard = P::is_orchard();
        let notes = list_all_received_notes(connection, start, is_orchard)?;

        Ok(Self {
            hasher: P::Hasher::default(),
            network: *network,
            account_infos,
            start: start.into(),
            notes,
            spends: vec![],
            position,
            tree_state,
            _data: PhantomData::<P>::default(),
        })
    }

    pub fn add(&mut self, blocks: &[CompactBlock]) -> Result<()> {
        let ivks = self
            .account_infos
            .iter()
            .filter_map(P::extract_ivk)
            .collect::<Vec<_>>();

        let outputs = blocks.into_par_iter().flat_map_iter(|b| {
            b.vtx.iter().enumerate().flat_map(move |(ivtx, vtx)| {
                P::extract_outputs(vtx)
                    .iter()
                    .enumerate()
                    .map(move |(vout, o)| (b.height, b.time, ivtx, vout, o))
            })
        });

        let (sender, receiver) = channel();
        outputs
            .into_par_iter()
            .for_each_with(sender, |sender, (height, time, ivtx, vout, o)| {
                P::try_decrypt(
                    &self.network,
                    &ivks,
                    height as u32,
                    time,
                    ivtx as u32,
                    vout as u32,
                    o,
                    sender,
                )
                .unwrap();
            });

        let mut notes = vec![];
        while let Ok(mut note) = receiver.recv() {
            let mut position = self.position;
            for cb in blocks.iter() {
                if cb.height as u32 == note.height {
                    break;
                }
                for tx in cb.vtx.iter() {
                    position += P::extract_outputs(tx).len() as u32;
                    position += P::extract_bridge(tx)
                        .map(|b| b.len as u32)
                        .unwrap_or_default();
                }
            }
            let cb = &blocks[(note.height - self.start - 1) as usize];
            for (ivtx, tx) in cb.vtx.iter().enumerate() {
                if ivtx as u32 == note.tx.ivtx {
                    break;
                }
                position += P::extract_outputs(tx).len() as u32;
                position += P::extract_bridge(tx)
                    .map(|b| b.len as u32)
                    .unwrap_or_default();
            }
            position += note.vout;
            note.position = position;

            let ai = self
                .account_infos
                .iter()
                .find(|&ai| ai.account == note.account)
                .unwrap();
            let txid = cb.vtx[note.tx.ivtx as usize]
                .hash
                .clone()
                .try_into()
                .unwrap();
            P::finalize_received_note(txid, &mut note, ai)?;
            notes.push(note);
        }

        let mut bridges = vec![];
        let mut p = self.position;
        for cb in blocks.iter() {
            for tx in cb.vtx.iter() {
                p += P::extract_outputs(tx).len() as u32;
                if let Some(b) = P::extract_bridge(tx) {
                    let be = BridgeExt {
                        b,
                        s: p as i32,
                        e: (p + b.len - 1) as i32,
                    };
                    bridges.push(be);
                    p += b.len;
                }
            }
        }

        let mut cmxs = vec![];
        let mut count_cmxs = 0;

        for depth in 0..MERKLE_DEPTH as usize {
            let mut position = self.position >> depth;
            // preprend previous trailing node (if resuming a half pair)
            if position % 2 == 1 {
                cmxs.insert(0, Some(self.tree_state.0[depth].unwrap()));
                position -= 1;
            }

            // slightly more efficient than doing it before the insert
            if depth == 0 {
                for cb in blocks.iter() {
                    for vtx in cb.vtx.iter() {
                        for co in P::extract_outputs(vtx).iter() {
                            let cmx = P::extract_cmx(co);
                            cmxs.push(Some(cmx));
                        }
                        count_cmxs += P::extract_outputs(vtx).len();
                        if let Some(b) = P::extract_bridge(vtx) {
                            for _ in 0..b.len {
                                cmxs.push(None);
                            }
                            count_cmxs += b.len as usize;
                        }
                    }
                }
            }

            // restore bridge start/end nodes
            let p = position as i32;
            for be in bridges.iter_mut() {
                let b = be.b;
                // tracing::info!("{depth} {i} {} {}", be.s, be.e);
                // tracing::info!("{} {}", be.s - p, be.e - p);
                // tracing::info!("{:?} {:?}",
                //     b.start.as_ref().unwrap().levels[depth],
                //     b.end.as_ref().unwrap().levels[depth]);
                let h = &b.start.as_ref().unwrap().levels[depth].hash;
                if !h.is_empty() {
                    // fill the *right* node of the be.s pair
                    cmxs[((be.s - p) | 1) as usize] = Some(h.clone().try_into().unwrap())
                }
                let h = &b.end.as_ref().unwrap().levels[depth].hash;
                if !h.is_empty() {
                    assert!(be.e % 2 == 0); // must have half pair, e must be left
                    cmxs[(be.e - p) as usize] = Some(h.clone().try_into().unwrap())
                }
                be.s = be.s / 2;
                be.e = (be.e - 1) / 2;
            }

            // loop on the *new* notes
            for n in notes.iter_mut() {
                let npos = n.position >> depth;
                let nidx = (npos - position) as usize;

                if depth == 0 {
                    n.witness.position = npos;
                    n.witness.value = cmxs[nidx].unwrap();
                }

                if nidx % 2 == 0 {
                    // left node
                    if nidx + 1 < cmxs.len() {
                        // ommer is right node if it exists
                        assert!(
                            cmxs[nidx + 1].is_some(),
                            "{} {} {}",
                            depth,
                            n.position,
                            nidx
                        );
                        n.witness.ommers.0[depth] = cmxs[nidx + 1];
                    } else {
                        n.witness.ommers.0[depth] = None;
                    }
                } else {
                    // right node
                    assert!(
                        cmxs[nidx - 1].is_some(),
                        "{} {} {}",
                        depth,
                        n.position,
                        nidx
                    );
                    n.witness.ommers.0[depth] = cmxs[nidx - 1]; // ommer is left node
                }
            }

            let len = cmxs.len();
            if len >= 2 {
                // loop on *old notes*
                for n in self.notes.iter_mut() {
                    if n.witness.ommers.0[depth].is_none() {
                        // fill right ommer if
                        assert!(cmxs[1].is_some());
                        n.witness.ommers.0[depth] = cmxs[1]; // we just got it
                    }
                }
            }

            // save last node if not a full pair
            if len % 2 == 1 {
                self.tree_state.0[depth] = cmxs[len - 1];
            } else {
                self.tree_state.0[depth] = None;
            }

            // hash and combine to next depth
            let pairs = len / 2;
            let mut cmxs2 = self.hasher.parallel_combine_opt(depth as u8, &cmxs, pairs);
            swap(&mut cmxs, &mut cmxs2);
        }

        tracing::info!("Old notes #{}", self.notes.len());
        tracing::info!("New notes #{}", notes.len());
        self.notes.append(&mut notes);
        self.position += count_cmxs as u32;
        self.start += blocks.len() as u32;

        // detect spends

        let mut nfs: HashMap<Hash, Vec<&mut ReceivedNote>> = HashMap::new();
        for n in self.notes.iter_mut() {
            let e = nfs.entry(n.nf).or_insert(vec![]);
            e.push(n);
        }

        for cb in blocks.iter() {
            for vtx in cb.vtx.iter() {
                for sp in P::extract_inputs(vtx).iter() {
                    let nf = P::extract_nf(sp);
                    if let Some(ns) = nfs.get_mut(&nf) {
                        for n in ns {
                            n.spent = Some(cb.height as u32);
                            let id_spent = IdSpent::<Hash> {
                                id_note: n.id,
                                account: n.account,
                                height: cb.height as u32, // height at which the spent occurs
                                txid: vtx.hash.clone().try_into().unwrap(),
                                note_ref: nf.clone().try_into().unwrap(),
                            };
                            let tx = TxValueUpdate {
                                account: n.account,
                                txid: vtx.hash.clone().try_into().unwrap(),
                                value: -(n.value as i64),
                                id_tx: 0,
                                height: cb.height as u32,
                                timestamp: cb.time,
                            };
                            self.spends.push((tx, id_spent));
                        }
                    }
                }
            }
        }

        info!("# {}", self.notes.len());
        // let auth_path = self.tree_state.to_auth_path(&self.hasher);
        // for note in self.notes.iter() {
        //     let root = note.witness.root(&auth_path, &self.hasher);
        //     info!("{}", hex::encode(&root));
        // }

        Ok(())
    }
}

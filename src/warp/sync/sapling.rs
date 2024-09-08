use jubjub::Fr;
use rusqlite::Connection;
use std::{collections::HashMap, mem::swap, sync::mpsc::channel};

use crate::{
    db::{
        account::{get_account_info, list_accounts},
        notes::list_received_notes,
    },
    lwd::rpc::{Bridge, CompactBlock},
    types::{AccountInfo, CheckpointHeight},
    warp::try_sapling_decrypt,
    Hash,
};
use anyhow::Result;
use rayon::prelude::*;
use tracing::info;
use zcash_primitives::consensus::Network;
use sapling_crypto::{value::NoteValue, Note, PaymentAddress, Rseed};

use crate::warp::{hasher::SaplingHasher, Edge, Hasher, MERKLE_DEPTH};

use super::{ReceivedNote, TxValueUpdate};

#[derive(Debug)]
pub struct Synchronizer {
    pub hasher: SaplingHasher,
    pub network: Network,
    pub account_infos: Vec<AccountInfo>,
    pub start: u32,
    pub notes: Vec<ReceivedNote>,
    pub spends: Vec<TxValueUpdate<Hash>>,
    pub position: u32,
    pub tree_state: Edge,
}

#[derive(Debug)]
struct BridgeExt<'a> {
    b: &'a Bridge,
    s: i32,
    e: i32,
}

impl Synchronizer {
    pub fn new(
        network: &Network,
        connection: &Connection,
        start: CheckpointHeight,
        position: u32,
        tree_state: Edge,
    ) -> Result<Self> {
        let accounts = list_accounts(connection)?;
        let mut account_infos = vec![];
        for a in accounts {
            let ai = get_account_info(network, connection, a.id)?;
            account_infos.push(ai);
        }
        let notes = list_received_notes(connection, start, false)?;

        Ok(Self {
            hasher: SaplingHasher::default(),
            network: *network,
            account_infos,
            start: start.into(),
            notes,
            spends: vec![],
            position,
            tree_state,
        })
    }

    pub fn add(&mut self, blocks: &[CompactBlock]) -> Result<()> {
        let ivks = self
            .account_infos
            .iter()
            .map(|ai| (ai.account, ai.sapling.vk.fvk.vk.ivk()))
            .collect::<Vec<_>>();

        let outputs = blocks.into_par_iter().flat_map_iter(|b| {
            b.vtx.iter().enumerate().flat_map(move |(ivtx, vtx)| {
                vtx.outputs
                    .iter()
                    .enumerate()
                    .map(move |(vout, o)| (b.height, b.time, ivtx, vout, o))
            })
        });

        let (sender, receiver) = channel();
        outputs
            .into_par_iter()
            .for_each_with(sender, |sender, (height, time, ivtx, vout, o)| {
                try_sapling_decrypt(
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
                    position += tx.outputs.len() as u32;
                    position += tx
                        .sapling_bridge
                        .as_ref()
                        .map(|b| b.len as u32)
                        .unwrap_or_default();
                }
            }
            let cb = &blocks[(note.height - self.start - 1) as usize];
            for (ivtx, tx) in cb.vtx.iter().enumerate() {
                if ivtx as u32 == note.tx.ivtx {
                    break;
                }
                position += tx.outputs.len() as u32;
                position += tx
                    .sapling_bridge
                    .as_ref()
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
            let recipient = PaymentAddress::from_bytes(&note.address).unwrap();
            let vk = &ai.sapling.vk.fvk.vk;
            let n = Note::from_parts(
                recipient,
                NoteValue::from_raw(note.value),
                Rseed::BeforeZip212(Fr::from_bytes(&note.rcm).unwrap()),
            );
            let nf = n.nf(&vk.nk, note.position as u64);
            note.nf = nf.0;
            note.tx.txid = cb.vtx[note.tx.ivtx as usize]
                .hash
                .clone()
                .try_into()
                .unwrap();
            notes.push(note);
        }

        let mut bridges = vec![];
        let mut p = self.position;
        for cb in blocks.iter() {
            for tx in cb.vtx.iter() {
                p += tx.outputs.len() as u32;
                if let Some(b) = &tx.sapling_bridge {
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
                        for co in vtx.outputs.iter() {
                            cmxs.push(Some(co.cmu.clone().try_into().unwrap()));
                        }
                        count_cmxs += vtx.outputs.len();
                        if let Some(b) = &vtx.sapling_bridge {
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
                if !h.is_empty() { // fill the *right* node of the be.s pair
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

                if nidx % 2 == 0 { // left node
                    if nidx + 1 < cmxs.len() { // ommer is right node if it exists
                        assert!(cmxs[nidx + 1].is_some(), "{} {} {}", depth, n.position, nidx);
                        n.witness.ommers.0[depth] = cmxs[nidx + 1];
                    } else {
                        n.witness.ommers.0[depth] = None;
                    }
                } else { // right node
                    assert!(cmxs[nidx - 1].is_some(), "{} {} {}", depth, n.position, nidx);
                    n.witness.ommers.0[depth] = cmxs[nidx - 1]; // ommer is left node
                }
            }

            let len = cmxs.len();
            if len >= 2 {
                // loop on *old notes*
                for n in self.notes.iter_mut() {
                    if n.witness.ommers.0[depth].is_none() { // fill right ommer if
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

        let mut nfs = self
            .notes
            .iter_mut()
            .map(|n| (n.nf.clone(), n))
            .collect::<HashMap<_, _>>();
        for cb in blocks.iter() {
            for vtx in cb.vtx.iter() {
                for sp in vtx.spends.iter() {
                    let nf = &*sp.nf;
                    if let Some(n) = nfs.get_mut(nf) {
                        n.spent = Some(cb.height as u32);
                        let tx = TxValueUpdate::<Hash> {
                            account: n.account,
                            txid: vtx.hash.clone().try_into().unwrap(),
                            value: -(n.value as i64),
                            id_tx: 0,
                            height: cb.height as u32,
                            id_spent: Some(n.nf),
                        };
                        self.spends.push(tx);
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

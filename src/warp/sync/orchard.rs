use orchard::{
    keys::Scope,
    note::{RandomSeed, Rho},
    value::NoteValue,
    Address, Note,
};
use rusqlite::Connection;
use std::{collections::HashMap, mem::swap, sync::mpsc::channel};

use crate::{
    db::account::{get_account_info, list_accounts},
    lwd::rpc::{Bridge, CompactBlock},
    types::{AccountInfo, CheckpointHeight},
    warp::{hasher::OrchardHasher, try_orchard_decrypt},
    Hash,
};
use crate::{db::notes::list_all_received_notes, network::Network};
use anyhow::Result;
use rayon::prelude::*;
use tracing::info;

use crate::warp::{Edge, Hasher, MERKLE_DEPTH};

use super::{ReceivedNote, TxValueUpdate};

#[derive(Debug)]
pub struct Synchronizer {
    pub hasher: OrchardHasher,
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
        coin: u8,
        network: &Network,
        connection: &Connection,
        start: CheckpointHeight,
        position: u32,
        tree_state: Edge,
    ) -> Result<Self> {
        let accounts = list_accounts(coin, connection)?;
        let mut account_infos = vec![];
        for a in accounts {
            let ai = get_account_info(network, connection, a.id)?;
            account_infos.push(ai);
        }
        let notes = list_all_received_notes(connection, start, true)?;

        Ok(Self {
            hasher: OrchardHasher::default(),
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
            .filter_map(|ai| {
                ai.orchard
                    .as_ref()
                    .map(|oi| (ai.account, oi.vk.to_ivk(Scope::External)))
            })
            .collect::<Vec<_>>();

        let actions = blocks.into_par_iter().flat_map_iter(|b| {
            b.vtx.iter().enumerate().flat_map(move |(ivtx, vtx)| {
                vtx.actions
                    .iter()
                    .enumerate()
                    .map(move |(vout, a)| (b.height, b.time, ivtx, vout, a))
            })
        });

        let (sender, receiver) = channel();
        actions
            .into_par_iter()
            .for_each_with(sender, |sender, (height, time, ivtx, vout, a)| {
                try_orchard_decrypt(
                    &self.network,
                    &ivks,
                    height as u32,
                    time,
                    ivtx as u32,
                    vout as u32,
                    a,
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
                    position += tx.actions.len() as u32;
                    position += tx
                        .orchard_bridge
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
                position += tx.actions.len() as u32;
                position += tx
                    .orchard_bridge
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
            let recipient = Address::from_raw_address_bytes(&note.address).unwrap();
            let rho = Rho::from_bytes(&note.rho.unwrap()).unwrap();
            let n = Note::from_parts(
                recipient,
                NoteValue::from_raw(note.value),
                rho,
                RandomSeed::from_bytes(note.rcm, &rho).unwrap(),
            )
            .unwrap();
            let vk = &ai.orchard.as_ref().unwrap().vk;
            let nf = n.nullifier(&vk);
            note.nf = nf.to_bytes();
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
                p += tx.actions.len() as u32;
                if let Some(b) = &tx.orchard_bridge {
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
                        for ca in vtx.actions.iter() {
                            cmxs.push(Some(ca.cmx.clone().try_into().unwrap()));
                        }
                        count_cmxs += vtx.actions.len();
                        if let Some(b) = &vtx.orchard_bridge {
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
            for (idx, be) in bridges.iter_mut().enumerate() {
                let b = be.b;
                tracing::debug!("{} {} {}", idx, be.s, be.e);
                tracing::debug!("{:?}", b);
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

        let mut nfs = self
            .notes
            .iter_mut()
            .map(|n| (n.nf.clone(), n))
            .collect::<HashMap<_, _>>();
        for cb in blocks.iter() {
            for vtx in cb.vtx.iter() {
                for ca in vtx.actions.iter() {
                    let nf = &*ca.nullifier;
                    if let Some(n) = nfs.get_mut(nf) {
                        n.spent = Some(cb.height as u32);
                        let tx = TxValueUpdate::<Hash> {
                            account: n.account,
                            txid: vtx.hash.clone().try_into().unwrap(),
                            value: -(n.value as i64),
                            id_tx: 0,
                            height: cb.height as u32,
                            timestamp: cb.time,
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

use jubjub::Fr;
use std::{collections::HashMap, mem::swap, sync::mpsc::channel};

use crate::{
    db::{get_account_info, list_accounts},
    lwd::rpc::CompactBlock,
    types::AccountInfo,
    warp::try_sapling_decrypt,
    Connection,
};
use anyhow::Result;
use rayon::prelude::*;
use tracing::info;
use zcash_primitives::{
    consensus::Network,
    sapling::{value::NoteValue, Diversifier, Note, Rseed},
};

use crate::warp::{hasher::SaplingHasher, Edge, Hasher, MERKLE_DEPTH};

use super::ReceivedNote;

#[derive(Debug)]
pub struct Synchronizer {
    pub hasher: SaplingHasher,
    pub network: Network,
    pub account_infos: Vec<AccountInfo>,
    pub height: u32,
    pub notes: Vec<ReceivedNote>,
    pub spent_notes: Vec<ReceivedNote>,
    pub position: u32,
    pub tree_state: Edge,
}

impl Synchronizer {
    pub fn new(
        network: &Network,
        connection: &Connection,
        height: u32,
        position: u32,
        tree_state: Edge,
    ) -> Result<Self> {
        let accounts = list_accounts(connection)?;
        let mut account_infos = vec![];
        for a in accounts {
            let ai = get_account_info(network, connection, a.account)?;
            account_infos.push(ai);
        }

        Ok(Self {
            hasher: SaplingHasher::default(),
            network: *network,
            account_infos,
            height,
            notes: vec![],
            spent_notes: vec![],
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
                }
            }
            let cb = &blocks[(note.height - self.height) as usize];
            for (ivtx, tx) in cb.vtx.iter().enumerate() {
                if ivtx as u32 == note.tx.ivtx {
                    break;
                }
                position += tx.outputs.len() as u32;
            }
            position += note.vout;
            note.position = position;

            let ai = self
                .account_infos
                .iter()
                .find(|&ai| ai.account == note.account)
                .unwrap();
            let d = Diversifier(note.diversifier);
            let vk = &ai.sapling.vk.fvk.vk;
            let recipient = vk.to_payment_address(d).unwrap();
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

        let mut cmxs = vec![];
        let mut position = self.position;
        let mut count_cmxs = 0;

        for depth in 0..MERKLE_DEPTH {
            if position % 2 == 1 {
                cmxs.insert(0, self.tree_state.0[depth].unwrap());
                position -= 1;
            }

            if depth == 0 {
                for cb in blocks.iter() {
                    for vtx in cb.vtx.iter() {
                        for co in vtx.outputs.iter() {
                            cmxs.push(co.cmu.clone().try_into().unwrap());
                        }
                        count_cmxs += vtx.outputs.len();
                    }
                }
            }

            for n in notes.iter_mut() {
                let npos = n.position >> depth;
                let nidx = (npos - position) as usize;

                if depth == 0 {
                    n.witness.position = npos;
                    n.witness.value = cmxs[nidx];
                }

                if nidx % 2 == 0 {
                    if nidx + 1 < cmxs.len() {
                        n.witness.ommers.0[depth] = Some(cmxs[nidx + 1]);
                    } else {
                        n.witness.ommers.0[depth] = None;
                    }
                } else {
                    n.witness.ommers.0[depth] = Some(cmxs[nidx - 1]);
                }
            }

            let len = cmxs.len();
            if len >= 2 {
                for n in self.notes.iter_mut() {
                    if n.witness.ommers.0[depth].is_none() {
                        n.witness.ommers.0[depth] = Some(cmxs[1]);
                    }
                }
            }

            if len % 2 == 1 {
                self.tree_state.0[depth] = Some(cmxs[len - 1]);
            } else {
                self.tree_state.0[depth] = None;
            }

            let pairs = cmxs.len() / 2;
            let mut cmxs2 = self.hasher.parallel_combine(depth as u8, &cmxs, pairs);
            position /= 2;
            swap(&mut cmxs, &mut cmxs2);
        }

        self.notes.append(&mut notes);
        self.position += count_cmxs as u32;
        self.height += blocks.len() as u32;

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
                    }
                }
            }
        }

        info!("# {}", self.notes.len());
        let auth_path = self.tree_state.to_auth_path(&self.hasher);
        for note in self.notes.iter() {
            let root = note.witness.root(&auth_path, &self.hasher);
            info!("{}", hex::encode(&root));
        }

        Ok(())
    }
}

use crate::Hash;

use super::{Edge, MERKLE_DEPTH};

#[derive(Debug)]
pub struct Witness {
    pub cmx: Hash,
    pub auth_path: [Hash; MERKLE_DEPTH],
}

#[derive(Debug)]
pub struct ReceivedNote {
    pub id: u32,
    pub account: u32,
    pub position: u32,
    pub height: u32,
    pub diversifier: [u8; 11],
    pub value: u64,
    pub rcm: Hash,
    pub rho: Option<Hash>,
    pub txid: Hash,
    pub vout: u32,
    pub witness: Witness,
}

#[derive(Default, Debug)]
pub struct Synchronizer {
    pub height: u32,
    pub notes: Vec<ReceivedNote>,
    pub new_notes: Vec<ReceivedNote>,
    pub spent_notes: Vec<ReceivedNote>,
    pub tree_state: Edge,
}

impl Synchronizer {
    pub fn new(height: u32, tree_state: Edge) -> Self {
        Self {
            height,
            notes: vec![],
            new_notes: vec![],
            spent_notes: vec![],
            tree_state,
        }
    }
    pub fn init_notes() {}
}

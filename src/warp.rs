mod decrypter;
pub mod edge;
pub mod hasher;
pub mod legacy;
mod orchard;
mod sapling;
pub mod sync;

use crate::{lwd::rpc::CompactBlock, Hash};
use serde::{Deserialize, Serialize};

pub(crate) const MERKLE_DEPTH: usize = 32;

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct Edge(pub [Option<Hash>; MERKLE_DEPTH]);

#[derive(Default, Debug)]
pub struct AuthPath(pub [Hash; MERKLE_DEPTH]);

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct Witness {
    pub value: Hash,
    pub position: u32,
    pub ommers: Edge,
}

#[derive(Clone, Default, Serialize, Debug)]
pub struct BlockHeader {
    pub height: u32,
    pub hash: Hash,
    pub prev_hash: Hash,
    pub timestamp: u32,
}

impl From<&CompactBlock> for BlockHeader {
    fn from(block: &CompactBlock) -> Self {
        BlockHeader {
            height: block.height as u32,
            hash: block.hash.clone().try_into().unwrap(),
            prev_hash: block.prev_hash.clone().try_into().unwrap(),
            timestamp: block.time,
        }
    }
}

pub trait Hasher: std::fmt::Debug + Default {
    fn empty(&self) -> Hash;
    fn combine(&self, depth: u8, l: &Hash, r: &Hash) -> Hash;
    fn parallel_combine(&self, depth: u8, layer: &[Hash], pairs: usize) -> Vec<Hash>;
}

#[derive(Default, Debug)]
pub struct OutPoint {
    pub txid: Hash,
    pub vout: u32,
}

#[derive(Default, Debug)]
pub struct TxOut {
    pub address: Option<TransparentAddress>,
    pub value: u64,
    pub vout: u32,
}

#[derive(Default, Debug)]
pub struct TransparentTx {
    pub account: u32,
    pub height: u32,
    pub timestamp: u32,
    pub txid: Hash,
    pub vins: Vec<OutPoint>,
    pub vouts: Vec<TxOut>,
}

#[derive(Default, Debug)]
pub struct UTXO {
    pub is_new: bool,
    pub id: u32,
    pub account: u32,
    pub height: u32,
    pub txid: Hash,
    pub vout: u32,
    pub value: u64,
}

pub use decrypter::{try_orchard_decrypt, try_sapling_decrypt};
use zcash_primitives::legacy::TransparentAddress;

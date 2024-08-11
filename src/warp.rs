mod decrypter;
pub mod edge;
pub mod hasher;
pub mod legacy;
mod orchard;
mod sapling;
pub mod sync;

use crate::Hash;
use serde::Serialize;

pub(crate) const MERKLE_DEPTH: usize = 32;

#[derive(Default, Serialize, Debug)]
pub struct Edge(pub [Option<Hash>; MERKLE_DEPTH]);

#[derive(Default, Debug)]
pub struct AuthPath(pub [Hash; MERKLE_DEPTH]);

#[derive(Default, Serialize, Debug)]
pub struct Witness {
    #[serde(with = "hex")]
    pub value: Hash,
    pub position: u32,
    pub ommers: Edge,
}

pub trait Hasher: std::fmt::Debug + Default {
    fn empty(&self) -> Hash;
    fn combine(&self, depth: u8, l: &Hash, r: &Hash) -> Hash;
    fn parallel_combine(&self, depth: u8, layer: &[Hash], pairs: usize) -> Vec<Hash>;
}

pub use decrypter::{try_orchard_decrypt, try_sapling_decrypt};

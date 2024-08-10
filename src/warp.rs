pub mod legacy;
pub mod hasher;
pub mod edge;
mod decrypter;
mod sapling;
mod orchard;
pub mod sync;

use crate::Hash;
pub(crate) const MERKLE_DEPTH: usize = 32;

#[derive(Default, Debug)]
pub struct Edge(pub [Option<Hash>; MERKLE_DEPTH]);

#[derive(Default, Debug)]
pub struct AuthPath(pub [Hash; MERKLE_DEPTH]);

#[derive(Debug)]
pub struct Witness {
    pub value: Hash,
    pub position: u32,
    pub ommers: Edge,
}

pub trait Hasher: std::fmt::Debug + Default {
    fn empty(&self) -> Hash;
    fn combine(&self, depth: u8, l: &Hash, r: &Hash) -> Hash;
    fn parallel_combine(&self, depth: u8, layer: &[Hash], pairs: usize) -> Vec<Hash>;
}

pub use decrypter::{try_sapling_decrypt, try_orchard_decrypt};

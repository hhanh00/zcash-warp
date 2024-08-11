use serde::Serialize;
use serde_hex::{SerHexOpt, Strict};
use crate::Hash;

use super::Witness;

mod sapling;
mod orchard;

#[derive(Serialize, Default, Debug)]
pub struct ReceivedTx {
    #[serde(with = "hex")]
    pub txid: Hash,
    pub timestamp: u32,
    pub ivtx: u32,
}

#[derive(Serialize, Debug)]
pub struct ReceivedNote {
    pub id: u32,
    pub account: u32,
    pub position: u32,
    pub height: u32,
    #[serde(with = "hex")]
    pub diversifier: [u8; 11],
    pub value: u64,
    #[serde(with = "hex")]
    pub rcm: Hash,
    #[serde(with = "hex")]
    pub nf: Hash,
    #[serde(with = "SerHexOpt::<Strict>")]
    pub rho: Option<Hash>,
    pub vout: u32,
    pub tx: ReceivedTx,
    pub spent: Option<u32>,
    pub witness: Witness,
}

pub use sapling::Synchronizer as SaplingSync;
pub use orchard::Synchronizer as OrchardSync;

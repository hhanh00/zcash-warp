use anyhow::Result;
use serde::{Deserialize, Serialize};
use zcash_client_backend::{address::RecipientAddress, encoding::AddressCodec as _};
use zcash_primitives::{
    consensus::Network, legacy::TransparentAddress, memo::MemoBytes, sapling::PaymentAddress,
};

use crate::warp::sync::ReceivedNote;

use super::{InputNote, OutputNote, TxInput, UTXO};

impl TxInput {
    pub fn from_utxo(utxo: &UTXO) -> Self {
        Self {
            amount: utxo.value,
            remaining: utxo.value,
            pool: 0,
            note: InputNote::Transparent {
                txid: utxo.txid,
                vout: utxo.vout,
                address: utxo.address.clone(),
            },
        }
    }

    pub fn from_sapling(note: &ReceivedNote) -> Self {
        Self {
            amount: note.value,
            remaining: note.value,
            pool: 1,
            note: InputNote::Sapling {
                address: note.address,
                rseed: note.rcm,
                witness: note.witness.clone(),
            },
        }
    }

    pub fn from_orchard(note: &ReceivedNote) -> Self {
        Self {
            amount: note.value,
            remaining: note.value,
            pool: 2,
            note: InputNote::Orchard {
                address: note.address,
                rseed: note.rcm,
                rho: note.rho.unwrap(),
                witness: note.witness.clone(),
            },
        }
    }
}

impl OutputNote {
    pub fn to_address(&self, network: &Network) -> String {
        match self {
            OutputNote::Transparent { pkh, address } => (if *pkh {
                TransparentAddress::PublicKey(address.clone())
            } else {
                TransparentAddress::Script(address.clone())
            })
            .encode(network),
            OutputNote::Sapling { address, .. } => {
                PaymentAddress::from_bytes(address).unwrap().encode(network)
            }
            OutputNote::Orchard { address, .. } => {
                let orchard = orchard::Address::from_raw_address_bytes(address).unwrap();
                let ua = zcash_client_backend::address::UnifiedAddress::from_receivers(
                    Some(orchard),
                    None,
                    None,
                )
                .unwrap();
                ua.encode(network)
            }
        }
    }

    pub fn from_address(network: &Network, address: &str, memo: MemoBytes) -> Result<Self> {
        let address = RecipientAddress::decode(network, address).unwrap();
        let note = match address {
            RecipientAddress::Transparent(t) => match t {
                TransparentAddress::PublicKey(pkh) => OutputNote::Transparent {
                    pkh: true,
                    address: pkh,
                },
                TransparentAddress::Script(h) => OutputNote::Transparent {
                    pkh: false,
                    address: h,
                },
            },
            RecipientAddress::Shielded(s) => OutputNote::Sapling {
                address: s.to_bytes(),
                memo,
            },
            RecipientAddress::Unified(u) => {
                let o = u.orchard().unwrap();
                OutputNote::Orchard {
                    address: o.to_raw_address_bytes(),
                    memo,
                }
            }
        };
        Ok(note)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "MemoBytes")]
pub struct MemoBytesProxy(#[serde(getter = "get_memo_bytes")] pub String);

fn get_memo_bytes(memo: &MemoBytes) -> String {
    hex::encode(memo.as_slice())
}

impl From<MemoBytesProxy> for MemoBytes {
    fn from(p: MemoBytesProxy) -> MemoBytes {
        MemoBytes::from_bytes(&hex::decode(&p.0).unwrap()).unwrap()
    }
}

use std::str::FromStr;

use fee::FeeManager;
use orchard::circuit::ProvingKey;
use rand::{CryptoRng, RngCore};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zcash_primitives::{consensus::Network, memo::MemoBytes};
use zcash_proofs::prover::LocalTxProver;
use zcash_keys::address::Address as RecipientAddress;
use zcash_protocol::memo::Memo;

use self::conv::MemoBytesProxy;
use crate::{
    data::fb::PaymentRequestT, keys::TSKStore, types::{AccountInfo, PoolMask}, warp::{legacy::CommitmentTreeFrontier, AuthPath, Edge, Witness, UTXO}, Hash
};

pub type Result<T> = std::result::Result<T, Error>;

pub mod builder;
pub mod conv;
mod fee;
pub mod prepare;
pub mod sweep;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Not Enough Funds, {0} more needed")]
    NotEnoughFunds(u64),
    #[error("Amount/Fee {0} too high to be paid by the recipient")]
    FeesTooHighForRecipient(u64),
    #[error("Transaction has no recipient")]
    NoRecipient,
    #[error("Transaction has no change output")]
    NoChangeOutput,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Clone, Debug)]
pub struct PaymentItem {
    pub address: String,
    pub amount: u64,
    pub memo: Option<MemoBytes>,
}

impl TryFrom<&PaymentRequestT> for PaymentItem {
    fn try_from(p: &PaymentRequestT) -> Result<Self> {
        let memo = p.memo_string.as_ref().map_or_else(
            || 
            p.memo_bytes.as_ref().map(|b| Memo::from_bytes(&*b)),
            |s| Some(Memo::from_str(&s))
        ).transpose().map_err(anyhow::Error::new)?;
        let memo = memo.map(|memo| MemoBytes::from(&memo));
        Ok(Self {
            address: p.address.clone().unwrap(),
            amount: p.amount,
            memo,
        })
    }
    
    type Error = Error;
}

pub struct Payment {
    pub recipients: Vec<PaymentItem>,
}

#[derive(Clone, Debug)]
pub struct ExtendedPayment {
    pub payment: PaymentItem,
    pub amount: u64,
    pub remaining: u64,
    pub pool: u8,
}

impl ExtendedPayment {
    pub fn to_inner(self) -> PaymentItem {
        self.payment
    }
    fn to_extended(network: &Network, payment: PaymentItem) -> Result<Self> {
        let ua = RecipientAddress::decode(network, &payment.address)
            .ok_or(anyhow::anyhow!("Invalid Address"))?;
        let pool = match ua {
            RecipientAddress::Sapling(_) => 1,
            RecipientAddress::Tex(_) => 0,
            RecipientAddress::Transparent(_) => 0,
            RecipientAddress::Unified(ua) => {
                let s = if ua.sapling().is_some() { 1 } else { 0 };
                let o = if ua.orchard().is_some() { 2 } else { 0 };
                s + o
            }
        };
        Ok(ExtendedPayment {
            amount: payment.amount,
            remaining: payment.amount,
            payment,
            pool,
        })
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TxInput {
    pub amount: u64,
    pub remaining: u64,
    pub pool: u8,
    pub note: InputNote,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum InputNote {
    Transparent {
        txid: Hash,
        vout: u32,
        address: String,
    },
    Sapling {
        #[serde(with = "serde_bytes")]
        address: [u8; 43],
        rseed: Hash,
        witness: Witness,
    },
    Orchard {
        #[serde(with = "serde_bytes")]
        address: [u8; 43],
        rseed: Hash,
        rho: Hash,
        witness: Witness,
    },
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum OutputNote {
    Transparent {
        pkh: bool,
        address: [u8; 20],
    },
    Sapling {
        #[serde(with = "serde_bytes")]
        address: [u8; 43],
        #[serde(with = "MemoBytesProxy")]
        memo: MemoBytes,
    },
    Orchard {
        #[serde(with = "serde_bytes")]
        address: [u8; 43],
        #[serde(with = "MemoBytesProxy")]
        memo: MemoBytes,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TxOutput {
    pub address_string: String,
    pub value: u64,
    pub note: OutputNote,
}

#[derive(Clone, Default, Debug)]
pub struct ChangeOutput {
    pub pools: PoolMask,
    pub value: i64,
    pub note: Option<OutputNote>,
}

pub struct PaymentBuilder {
    pub network: Network,
    pub height: u32,
    pub account: u32,
    pub ai: AccountInfo,
    pub inputs: [Vec<TxInput>; 3],
    pub outputs: Vec<ExtendedPayment>,
    pub account_pools: PoolMask,
    pub src_pools: PoolMask,

    pub fee_manager: FeeManager,
    pub fee: u64,

    pub available: [u64; 3],
    pub change: ChangeOutput,

    pub s_edge: Edge,
    pub o_edge: Edge,
}

#[derive(Debug)]
pub struct AdjustableUnsignedTransaction {
    pub tx_notes: Vec<TxInput>,
    pub tx_outputs: Vec<TxOutput>,
    pub change: ChangeOutput,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UnsignedTransaction {
    pub account: u32,
    pub account_name: String,
    pub account_id: Hash,
    pub height: u32,
    pub tx_notes: Vec<TxInput>,
    pub tx_outputs: Vec<TxOutput>,
    pub roots: [Hash; 2],
    pub edges: [AuthPath; 2],
}

const EXPIRATION_HEIGHT: u32 = 50;

lazy_static::lazy_static! {
    pub static ref PROVER: LocalTxProver = LocalTxProver::with_default_location().unwrap();
    pub static ref ORCHARD_PROVER: ProvingKey = ProvingKey::build();
}

pub fn make_payment<R: RngCore + CryptoRng>(network: &Network, connection: &Connection, account: u32,
    height: u32, p: Payment, src_pools: PoolMask,
    fee_paid_by_sender: bool,
    s_tree: &CommitmentTreeFrontier, o_tree: &CommitmentTreeFrontier,
    mut rng: R) -> Result<Vec<u8>> {
    let mut pb = PaymentBuilder::new(
        network,
        connection,
        account,
        height,
        p,
        src_pools,
        s_tree,
        o_tree,
    )?;
    pb.add_account_funds(&connection)?;
    pb.set_use_change(true)?;
    let mut utx = pb.prepare()?;
    if !fee_paid_by_sender {
        let fee = pb.fee_manager.fee();
        utx.add_to_change(fee as i64)?;
    }
    let utx = pb.finalize(utx)?;
    let txb = utx.build(network, connection, &mut TSKStore::default(), &mut rng)?;
    Ok(txb)
}

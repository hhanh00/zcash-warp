use fee::FeeManager;
use fpdec::Decimal;
use orchard::circuit::ProvingKey;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::memo::MemoBytes;
use zcash_proofs::prover::LocalTxProver;

use self::conv::MemoBytesProxy;
use crate::{
    data::fb::{
        PaymentRequestT, RecipientT, TransactionRecipientT, TransactionSummaryT, UnconfirmedTxT,
    },
    fb_unwrap,
    network::Network,
    types::{AccountInfo, CheckpointHeight, PoolMask},
    warp::{legacy::CommitmentTreeFrontier, AuthPath, Edge, Witness, UTXO},
    Hash,
};

pub type Result<T> = std::result::Result<T, Error>;

pub mod builder;
pub mod conv;
mod fee;
pub mod prepare;
pub mod sweep;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Not Enough Funds, {0} needed, {1} available, {2} more needed")]
    NotEnoughFunds(Decimal, Decimal, Decimal),
    #[error("Amount/Fee {0} too high to be paid by the recipient")]
    FeesTooHighForRecipient(u64),
    #[error("Transaction has no recipient")]
    NoRecipient,
    #[error("Transaction has no change output")]
    NoChangeOutput,
    #[error("No Funds available. Some funds may not have enough confirmations yet.")]
    NoFunds,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Clone, Debug)]
pub struct ExtendedRecipient {
    pub recipient: RecipientT,
    pub amount: u64,
    pub remaining: u64,
    pub pool_mask: PoolMask,
    pub is_change: bool,
}

impl ExtendedRecipient {
    pub fn to_inner(self) -> RecipientT {
        self.recipient
    }

    fn to_extended(network: &Network, recipient: RecipientT) -> Result<Self> {
        let ua = RecipientAddress::decode(network, &fb_unwrap!(recipient.address))
            .ok_or(anyhow::anyhow!("Invalid Address"))?;
        let pools = match ua {
            RecipientAddress::Sapling(_) => 2,
            RecipientAddress::Tex(_) => 1,
            RecipientAddress::Transparent(_) => 1,
            RecipientAddress::Unified(ua) => {
                let t = if ua.transparent().is_some() { 1 } else { 0 };
                let s = if ua.sapling().is_some() { 2 } else { 0 };
                let o = if ua.orchard().is_some() { 4 } else { 0 };
                t | s | o
            }
        };
        let pools = pools & recipient.pools;
        let pools = if pools != 1 { pools & 6 } else { pools }; // remove T
        Ok(ExtendedRecipient {
            amount: recipient.amount,
            remaining: recipient.amount,
            recipient,
            pool_mask: PoolMask(pools),
            is_change: false,
        })
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TxInput {
    pub id: u32,
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
        external: u32,
        addr_index: u32,
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
    pub pool: u8,
    pub amount: u64,
    pub note: OutputNote,
    pub is_change: bool,
}

pub struct PaymentBuilder {
    pub network: Network,
    pub height: u32,
    pub account: u32,
    pub ai: AccountInfo,
    pub inputs: [Vec<TxInput>; 3],
    pub outputs: Vec<ExtendedRecipient>,
    pub account_pools: PoolMask,
    pub src_pools: PoolMask,

    pub fee_manager: FeeManager,
    pub fee: u64,

    pub available: [u64; 3],
    pub used: [bool; 3],
    pub use_change: bool,
    pub use_unique_change: bool,

    pub s_edge: Edge,
    pub o_edge: Edge,
}

#[derive(Debug)]
pub struct AdjustableUnsignedTransaction {
    pub tx_notes: Vec<TxInput>,
    pub tx_outputs: Vec<TxOutput>,
    pub sum_ins: u64,
    pub sum_outs: u64,
    pub change: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UnsignedTransaction {
    pub account: u32,
    pub account_name: String,
    pub height: u32,
    pub tx_notes: Vec<TxInput>,
    pub tx_outputs: Vec<TxOutput>,
    pub roots: [Hash; 2],
    pub edges: [AuthPath; 2],
    pub fees: FeeManager,
    pub tx: UnconfirmedTxT,
    pub redirect: Option<String>,
}

impl UnsignedTransaction {
    pub fn to_summary(&self) -> Result<TransactionSummaryT> {
        let recipients = self
            .tx_outputs
            .iter()
            .filter_map(|o| {
                Some(TransactionRecipientT {
                    address: Some(o.address_string.clone()),
                    amount: o.amount,
                    change: o.is_change,
                })
            })
            .collect::<Vec<_>>();
        let ins = self
            .tx_notes
            .iter()
            .map(|i| match i.note {
                InputNote::Transparent { .. } => PoolBalance(i.amount as i64, 0, 0),
                InputNote::Sapling { .. } => PoolBalance(0, i.amount as i64, 0),
                InputNote::Orchard { .. } => PoolBalance(0, 0, i.amount as i64),
            })
            .sum::<PoolBalance>();
        let outs = self
            .tx_outputs
            .iter()
            .map(|o| match o.note {
                OutputNote::Transparent { .. } => PoolBalance(o.amount as i64, 0, 0),
                OutputNote::Sapling { .. } => PoolBalance(0, o.amount as i64, 0),
                OutputNote::Orchard { .. } => PoolBalance(0, 0, o.amount as i64),
            })
            .sum::<PoolBalance>();
        let net = ins - outs;
        let fee = (net.0 + net.1 + net.2) as u64;
        let data = bincode::serialize(&self).unwrap();
        let privacy_level = if self.fees.num_inputs[0] != 0 && self.fees.num_outputs[0] != 0 {
            0 // both transparent in and out
        } else if self.fees.num_inputs[0] != 0 || self.fees.num_outputs[0] != 0 {
            1 // either transparent in or out
        } else if net.1.abs() as u64 > fee || net.2.abs() as u64 > fee {
            2 // shielded net > fee
        } else {
            3 // fully shielded
        };

        Ok(TransactionSummaryT {
            height: self.height,
            recipients: Some(recipients),
            transparent_ins: ins.0 as u64,
            sapling_net: net.1,
            orchard_net: net.2,
            fee,
            num_inputs: Some(self.fees.num_inputs.to_vec()),
            num_outputs: Some(self.fees.num_outputs.to_vec()),
            privacy_level,
            data: Some(data),
            redirect: self.redirect.clone(),
        })
    }
}

impl TransactionSummaryT {
    pub fn detach(&mut self) -> Vec<u8> {
        let data = self.data.take();
        data.unwrap()
    }
}

#[derive(Clone, Copy, Debug)]
struct PoolBalance(i64, i64, i64);

impl std::iter::Sum for PoolBalance {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(PoolBalance(0, 0, 0), |a, b| a + b)
    }
}

impl std::ops::Add for PoolBalance {
    type Output = PoolBalance;

    fn add(self, rhs: Self) -> Self::Output {
        PoolBalance(self.0 + rhs.0, self.1 + rhs.1, self.2 + rhs.2)
    }
}

impl std::ops::Sub for PoolBalance {
    type Output = PoolBalance;

    fn sub(self, rhs: Self) -> Self::Output {
        PoolBalance(self.0 - rhs.0, self.1 - rhs.1, self.2 - rhs.2)
    }
}

lazy_static::lazy_static! {
    pub static ref PROVER: Mutex<Option<LocalTxProver>> = Mutex::new(LocalTxProver::with_default_location());
    pub static ref ORCHARD_PROVER: ProvingKey = ProvingKey::build();
}

pub fn make_payment(
    network: &Network,
    connection: &Connection,
    account: u32,
    payment: &PaymentRequestT,
    s_tree: &CommitmentTreeFrontier,
    o_tree: &CommitmentTreeFrontier,
    redirect: Option<String>,
) -> Result<UnsignedTransaction> {
    let mut pb = PaymentBuilder::new(
        network,
        connection,
        account,
        CheckpointHeight(payment.height),
        fb_unwrap!(payment.recipients),
        PoolMask(payment.src_pools),
        s_tree,
        o_tree,
    )?;
    pb.add_account_funds(&connection)?;
    pb.set_use_change(payment.use_change)?;
    let mut utx = pb.prepare()?;
    if !payment.sender_pay_fees {
        let fee = pb.fee_manager.fee();
        utx.add_to_change(fee as i64)?;
    }
    let utx = pb.finalize(utx, redirect)?;
    Ok(utx)
}

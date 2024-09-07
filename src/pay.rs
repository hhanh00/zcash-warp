use std::str::FromStr;

use fee::FeeManager;
use orchard::circuit::ProvingKey;
use rand::{CryptoRng, RngCore};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::{consensus::Network, memo::MemoBytes};
use zcash_proofs::prover::LocalTxProver;
use zcash_protocol::memo::Memo;

use self::conv::MemoBytesProxy;
use crate::{
    data::fb::{PaymentRequestT, TransactionRecipientT, TransactionSummaryT},
    keys::TSKStore,
    types::{AccountInfo, PoolMask},
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
        let memo = p
            .memo_string
            .as_ref()
            .map_or_else(
                || p.memo_bytes.as_ref().map(|b| Memo::from_bytes(&*b)),
                |s| Some(Memo::from_str(&s)),
            )
            .transpose()
            .map_err(anyhow::Error::new)?;
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
    pub pool: PoolMask,
    pub is_change: bool,
}

impl ExtendedPayment {
    pub fn to_inner(self) -> PaymentItem {
        self.payment
    }
    fn to_extended(network: &Network, payment: PaymentItem) -> Result<Self> {
        let ua = RecipientAddress::decode(network, &payment.address)
            .ok_or(anyhow::anyhow!("Invalid Address"))?;
        let pool = match ua {
            RecipientAddress::Sapling(_) => 2,
            RecipientAddress::Tex(_) => 1,
            RecipientAddress::Transparent(_) => 1,
            RecipientAddress::Unified(ua) => {
                let s = if ua.sapling().is_some() { 2 } else { 0 };
                let o = if ua.orchard().is_some() { 4 } else { 0 };
                s | o
            }
        };
        Ok(ExtendedPayment {
            amount: payment.amount,
            remaining: payment.amount,
            payment,
            pool: PoolMask(pool),
            is_change: false,
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
    pub amount: u64,
    pub note: OutputNote,
    pub change: bool,
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
    pub use_change: bool,

    pub s_edge: Edge,
    pub o_edge: Edge,
}

#[derive(Debug)]
pub struct AdjustableUnsignedTransaction {
    pub tx_notes: Vec<TxInput>,
    pub tx_outputs: Vec<TxOutput>,
    pub change: i64,
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

impl UnsignedTransaction {
    pub fn to_summary(&self) -> Result<TransactionSummaryT> {
        let recipients = self.tx_outputs.iter().filter_map(|o| {
            if !o.change {
                Some(TransactionRecipientT {
                    address: Some(o.address_string.clone()),
                    amount: o.amount,
                })
            } else {
                None
            }
        }).collect::<Vec<_>>();
        let ins = self.tx_notes.iter().map(|i| {
            match i.note {
                InputNote::Transparent { .. } => PoolBalance(i.amount as i64, 0, 0),
                InputNote::Sapling { .. } => PoolBalance(0, i.amount as i64, 0),
                InputNote::Orchard { .. } => PoolBalance(0, 0, i.amount as i64),
            }
        }).sum::<PoolBalance>();
        let outs = self.tx_outputs.iter().map(|o| {
            match o.note {
                OutputNote::Transparent { .. } => PoolBalance(o.amount as i64, 0, 0),
                OutputNote::Sapling { .. } => PoolBalance(0, o.amount as i64, 0),
                OutputNote::Orchard { .. } => PoolBalance(0, 0, o.amount as i64),
            }
        }).sum::<PoolBalance>();
        let net = ins - outs;
        let fee = (net.0 + net.1 + net.2) as u64;
        let data = bincode::serialize(&self).unwrap();
        Ok(TransactionSummaryT {
            recipients: Some(recipients),
            transparent_ins: ins.0 as u64,
            sapling_net: net.1,
            orchard_net: net.2,
            fee,
            data: Some(data),
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
        iter.fold(PoolBalance(0, 0, 0), |a, b| 
        a + b)
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
    pub static ref PROVER: LocalTxProver = LocalTxProver::with_default_location().unwrap();
    pub static ref ORCHARD_PROVER: ProvingKey = ProvingKey::build();
}

pub fn make_payment(
    network: &Network,
    connection: &Connection,
    account: u32,
    height: u32,
    confirmations: u32,
    p: Payment,
    src_pools: PoolMask,
    fee_paid_by_sender: bool,
    s_tree: &CommitmentTreeFrontier,
    o_tree: &CommitmentTreeFrontier,
) -> Result<UnsignedTransaction> {
    let confirmation_height = height - confirmations + 1;
    let mut pb = PaymentBuilder::new(
        network, connection, account, confirmation_height, p, src_pools, s_tree, o_tree,
    )?;
    pb.add_account_funds(&connection)?;
    pb.set_use_change(true)?;
    let mut utx = pb.prepare()?;
    if !fee_paid_by_sender {
        let fee = pb.fee_manager.fee();
        utx.add_to_change(fee as i64)?;
    }
    let utx = pb.finalize(utx)?;
    Ok(utx)
}

pub fn sign_tx<R: RngCore + CryptoRng>(
    network: &Network,
    connection: &Connection,
    expiration_height: u32,
    utx: UnsignedTransaction,
    tsk_store: &mut TSKStore,
    mut rng: R,
) -> Result<Vec<u8>> {
    let txb = utx.build(network, connection, expiration_height, tsk_store, &mut rng)?;
    Ok(txb)
}

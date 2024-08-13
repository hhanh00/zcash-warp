use anyhow::Result;
use rusqlite::Connection;
use zcash_primitives::memo::MemoBytes;

use crate::types::PoolMask;

pub struct PaymentItem {
    pub address: String,
    pub amount: u64,
    pub memo: MemoBytes,
}

pub struct Payment {
    pub src_pools: PoolMask,
    pub recipients: Vec<PaymentItem>,
}

pub fn prepare_payment(connection: &Connection, account: u32, payment: &Payment) -> Result<()> {
    todo!()
}

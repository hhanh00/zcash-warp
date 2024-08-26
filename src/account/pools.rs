use anyhow::Result;
use rand::{CryptoRng, RngCore};
use rusqlite::Connection;
use zcash_primitives::{consensus::Network, memo::MemoBytes};

use crate::{
    db::account::get_account_info, keys::TSKStore, pay::{Payment, PaymentBuilder, PaymentItem}, warp::legacy::CommitmentTreeFrontier
};

pub fn transfer_pools<R: RngCore + CryptoRng>(
    network: &Network,
    connection: &Connection,
    account: u32,
    height: u32,
    from_pool: u8,
    to_pool: u8,
    mut amount: u64,
    memo: MemoBytes,
    split_amount: u64,
    s: &CommitmentTreeFrontier,
    o: &CommitmentTreeFrontier,
    rng: R,
) -> Result<Vec<u8>> {
    let ai = get_account_info(network, connection, account)?;
    let to_address = ai.to_address(network, Some(to_pool).into()).unwrap();
    let split_amount = if split_amount == 0 {
        amount
    } else {
        split_amount
    };
    let mut recipients = vec![];
    while amount > 0 {
        let a = amount.min(split_amount);
        let p = PaymentItem {
            address: to_address.clone(),
            amount: a,
            memo: memo.clone(),
        };
        recipients.push(p);
        amount -= a;
    }
    let payment = Payment { recipients };
    let mut builder = PaymentBuilder::new(
        network,
        connection,
        account,
        height,
        payment,
        Some(from_pool).into(),
        &s,
        &o,
    )?;
    builder.add_account_funds(connection)?;
    builder.set_use_change(true)?;
    let utx = builder.prepare()?;
    let utx = builder.finalize(utx)?;
    let tx = utx.build(network, connection, &mut TSKStore::default(), rng)?;
    Ok(tx)
}

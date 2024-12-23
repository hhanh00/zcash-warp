use anyhow::Result;
use rand::{CryptoRng, RngCore};
use rusqlite::Connection;
use zcash_primitives::memo::MemoBytes;

use crate::{
    data::{RecipientT, TransactionBytesT},
    db::{account::get_account_info, chain::snap_to_checkpoint},
    network::Network,
    pay::PaymentBuilder,
    warp::legacy::CommitmentTreeFrontier,
    EXPIRATION_HEIGHT_DELTA,
};

pub fn transfer_pools<R: RngCore + CryptoRng>(
    network: &Network,
    connection: &Connection,
    account: u32,
    height: u32,
    confirmations: u32,
    from_pool: u8,
    to_pool: u8,
    mut amount: u64,
    memo: Option<MemoBytes>,
    split_amount: u64,
    s: &CommitmentTreeFrontier,
    o: &CommitmentTreeFrontier,
    rng: R,
) -> Result<TransactionBytesT> {
    let ai = get_account_info(network, connection, account)?;
    let to_address = ai.to_address(network, Some(to_pool).into()).unwrap();
    let split_amount = if split_amount == 0 {
        amount
    } else {
        split_amount
    };
    let mut recipients = vec![];
    let memo = memo.map(|memo| memo.as_slice().to_vec());
    while amount > 0 {
        let a = amount.min(split_amount);
        let p = RecipientT {
            address: to_address.clone(),
            amount: a,
            pools: 7,
            memo: None,
            memo_bytes: memo.clone().unwrap_or_default(),
        };
        recipients.push(p);
        amount -= a;
    }
    let confirmation_height = snap_to_checkpoint(connection, height - confirmations + 1)?;
    let mut builder = PaymentBuilder::new(
        network,
        connection,
        account,
        confirmation_height,
        &recipients,
        Some(from_pool).into(),
        &s,
        &o,
    )?;
    builder.add_account_funds(connection)?;
    builder.set_use_change(true)?;
    let utx = builder.prepare()?;
    let utx = builder.finalize(utx, None)?;
    let tx = utx.build(network, connection, height + EXPIRATION_HEIGHT_DELTA, rng)?;
    Ok(tx)
}

use crate::{data::fb::TransactionInfoT, db::tx::list_txs};
use anyhow::Result;
use rusqlite::Connection;

pub fn get_txs(
    connection: &Connection,
    account: u32,
    bc_height: u32,
) -> Result<Vec<TransactionInfoT>> {
    let txs = list_txs(connection, account)?;
    let mut tis = vec![];
    for ertx in txs {
        let rtx = &ertx.rtx;
        let ti = TransactionInfoT {
            id: rtx.id,
            txid: Some(rtx.txid.to_vec()),
            height: rtx.height,
            confirmations: bc_height - rtx.height + 1,
            timestamp: rtx.timestamp,
            amount: rtx.value,
            address: ertx.address,
            contact: ertx.contact,
            memo: ertx.memo,
        };
        tis.push(ti);
    }
    Ok(tis)
}

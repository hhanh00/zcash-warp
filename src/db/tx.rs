use std::io::Read;

use crate::{
    db::account::get_account_info,
    messages::ZMessage,
    txdetails::{ShieldedOutput, Transaction as TransactionDetails},
    types::PoolMask,
    Hash,
};
use anyhow::Result;
use flate2::read::ZlibDecoder;
use rusqlite::{params, Connection};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_primitives::{
    consensus::Network,
    memo::{Memo, MemoBytes},
    sapling::PaymentAddress,
};

pub fn list_new_txids(connection: &Connection) -> Result<Vec<(u32, u32, u32, Hash)>> {
    let mut s = connection.prepare(
        "SELECT t.id_tx, t.account, t.timestamp, t.txid FROM txs t
        LEFT JOIN txdetails d ON t.txid = d.txid WHERE d.txid IS NULL",
    )?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    let mut res = vec![];
    for r in rows {
        let (id_tx, account, timestamp, txid) = r?;
        let txid: Hash = txid.try_into().unwrap();
        res.push((id_tx, account, timestamp, txid));
    }
    Ok(res)
}

pub fn get_tx_details(
    network: &Network,
    connection: &Connection,
    id_tx: u32,
) -> Result<TransactionDetails> {
    let (account, tx_bin) = connection.query_row(
        "SELECT t.account, d.data FROM txs t
        JOIN txdetails d ON t.id_tx = d.id_tx 
        WHERE t.id_tx = ?1",
        [id_tx],
        |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?)),
    )?;
    let mut authenticated = false;
    let ai = get_account_info(network, connection, account)?;
    let account_address = ai.to_address(network, PoolMask(7)).unwrap();
    let tx: TransactionDetails = bincode::deserialize_from(&*tx_bin)?;
    tracing::info!("{:?}", tx);
    let mut spend_address = None;
    if let Some(taddr) = ai.transparent.as_ref().map(|ti| ti.addr) {
        let taddr = taddr.encode(network);
        for tin in tx.tins.iter() {
            if let Some(address) = tin.coin.address.as_ref() {
                spend_address = Some(address.clone());
                if address == &taddr {
                    authenticated = true;
                }
            }
        }
    }

    for input in tx.sins.iter().chain(tx.oins.iter()) {
        if input.is_some() {
            authenticated = true;
        }
    }

    for (nout, output) in tx
        .souts
        .iter()
        .map(|o| (o, false))
        .chain(tx.oouts.iter().map(|o| (o, true)))
        .enumerate()
    {
        if let (Some(output), orchard) = output {
            let note_address = if orchard {
                let a = orchard::Address::from_raw_address_bytes(&output.address).unwrap();
                let a = zcash_client_backend::address::UnifiedAddress::from_receivers(
                    Some(a),
                    None,
                    None,
                )
                .unwrap();
                a.encode(network)
            } else {
                let a = PaymentAddress::from_bytes(&output.address).unwrap();
                a.encode(network)
            };
            let sender = if output.incoming {
                spend_address.clone()
            } else {
                Some(account_address.clone())
            };
            let recipient = note_address;

            let memo = decode_shielded_output_memo(output)?;
            visit_memo(
                connection,
                account,
                id_tx,
                &tx,
                nout as u32,
                output.incoming,
                authenticated,
                sender,
                recipient,
                &memo,
            )?;
        }
    }
    Ok(tx)
}

fn decode_shielded_output_memo(output: &ShieldedOutput) -> Result<Memo> {
    let mut d = ZlibDecoder::new(&*output.memo);
    let mut memo = vec![];
    d.read_to_end(&mut memo)?;
    let memo = MemoBytes::from_bytes(&memo)?;
    let memo: Memo = memo.try_into()?;
    Ok(memo)
}

fn visit_memo(
    connection: &Connection,
    account: u32,
    id_tx: u32,
    tx: &TransactionDetails,
    nout: u32,
    incoming: bool,
    authenticated: bool,
    sender: Option<String>,
    recipient: String,
    memo: &Memo,
) -> Result<()> {
    tracing::info!("{} {:?}", authenticated, memo);
    match memo {
        Memo::Text(text) => {
            let msg = parse_memo_text(
                id_tx,
                nout,
                tx.height,
                tx.timestamp,
                incoming,
                sender,
                recipient,
                &*text,
            )?;
            store_message(connection, account, &tx, nout, &msg)?;
        }
        Memo::Arbitrary(_) => todo!(),
        _ => {}
    }
    Ok(())
}

fn parse_memo_text(
    id_tx: u32,
    nout: u32,
    height: u32,
    timestamp: u32,
    incoming: bool,
    sender: Option<String>,
    recipient: String,
    memo: &str,
) -> Result<ZMessage> {
    let memo_lines: Vec<_> = memo.splitn(4, '\n').collect();
    let msg = if memo_lines.len() == 4 && memo_lines[0] == "\u{1F6E1}MSG" {
        ZMessage {
            id_tx,
            nout,
            height,
            timestamp,
            incoming,
            sender: if memo_lines[1].is_empty() {
                sender
            } else {
                Some(memo_lines[1].to_string())
            },
            recipient: recipient.to_string(),
            subject: memo_lines[2].to_string(),
            body: memo_lines[3].to_string(),
        }
    } else {
        ZMessage {
            id_tx,
            height,
            timestamp,
            incoming,
            nout,
            sender: None,
            recipient: recipient.to_string(),
            subject: String::new(),
            body: memo.to_string(),
        }
    };
    tracing::info!("{:?}", msg);
    Ok(msg)
}

pub fn store_message(
    connection: &Connection,
    account: u32,
    tx: &TransactionDetails,
    nout: u32,
    message: &ZMessage,
) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO msgs
        (account, height, timestamp, txid, nout, 
        sender, recipient, subject, body, read)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, false)
        ON CONFLICT DO NOTHING",
    )?;
    s.execute(params![
        account,
        tx.height,
        tx.timestamp,
        tx.txid,
        nout,
        message.sender,
        message.recipient,
        message.subject,
        message.body
    ])?;
    Ok(())
}

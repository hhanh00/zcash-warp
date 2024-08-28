use std::io::{Read, Write};

use anyhow::{Error, Result};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use orchard::{keys::Scope, note::ExtractedNoteCommitment, note_encryption::OrchardDomain};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};
use zcash_primitives::{
    consensus::Network,
    memo::{Memo, MemoBytes},
    sapling::{note_encryption::SaplingDomain, PaymentAddress},
    transaction::Transaction as ZTransaction,
};

use crate::{
    account::contacts::{add_contact, ua_of_orchard, ChunkedContactV1, ChunkedMemoDecoder},
    coin::connect_lwd,
    db::{
        account::get_account_info,
        notes::{get_note_by_nf, store_tx_details},
        tx::{get_tx, list_new_txids, store_message, update_tx_primary_address_memo},
    },
    lwd::{get_transaction, get_txin_coins},
    messages::ZMessage,
    types::{Addresses, PoolMask},
    warp::{
        sync::{PlainNote, ReceivedTx},
        OutPoint, TxOut2,
    },
    Hash, PooledSQLConnection,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct TransparentInput {
    pub out_point: OutPoint,
    pub coin: TxOut2,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TransparentOutput {
    pub coin: TxOut2,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShieldedInput {
    pub note: PlainNote,
    #[serde(with = "serde_bytes")]
    pub nf: Hash,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShieldedOutput {
    pub incoming: bool,
    pub note: PlainNote,
    #[serde(with = "serde_bytes")]
    pub cmx: Hash,
    #[serde(with = "serde_bytes")]
    pub address: [u8; 43],
    #[serde(with = "serde_bytes")]
    pub memo: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TransactionDetails {
    pub height: u32,
    pub timestamp: u32,
    #[serde(with = "serde_bytes")]
    pub txid: Hash,
    pub tins: Vec<TransparentInput>,
    pub touts: Vec<TransparentOutput>,
    pub sins: Vec<Option<ShieldedInput>>,
    pub souts: Vec<Option<ShieldedOutput>>,
    pub oins: Vec<Option<ShieldedInput>>,
    pub oouts: Vec<Option<ShieldedOutput>>,
}

pub fn analyze_raw_transaction(
    network: &Network,
    connection: &Connection,
    url: String,
    height: u32,
    timestamp: u32,
    account: u32,
    tx: ZTransaction,
) -> Result<TransactionDetails> {
    let ai = get_account_info(network, connection, account)?;
    let txid: Hash = tx.txid().as_ref().clone();
    let data = tx.into_data();
    let mut tins = vec![];
    let mut touts = vec![];
    if let Some(b) = data.transparent_bundle() {
        for vin in b.vin.iter() {
            let tin = TransparentInput {
                out_point: OutPoint {
                    txid: vin.prevout.hash().clone(),
                    vout: vin.prevout.n(),
                },
                coin: TxOut2::default(),
            };
            tins.push(tin);
        }

        for (n, vout) in b.vout.iter().enumerate() {
            let tout = TransparentOutput {
                coin: TxOut2 {
                    address: vout.recipient_address().map(|a| a.encode(network)),
                    value: vout.value.into(),
                    vout: n as u32,
                },
            };
            touts.push(tout);
        }
    }
    let mut sins = vec![];
    let mut souts = vec![];
    if let Some(b) = data.sapling_bundle() {
        let ivk = zcash_primitives::sapling::keys::PreparedIncomingViewingKey::new(
            &ai.sapling.vk.fvk.vk.ivk(),
        );
        let ovk = &ai.sapling.vk.fvk.ovk;
        for sin in b.shielded_spends() {
            let spend = get_note_by_nf(connection, &sin.nullifier.0)?;
            sins.push(spend.map(|pn| ShieldedInput {
                note: pn,
                nf: sin.nullifier.0.clone(),
            }));
        }
        for sout in b.shielded_outputs() {
            let domain = SaplingDomain::for_height(*network, height.into());
            let r = try_note_decryption(&domain, &ivk, sout)
                .map(|(n, p, m)| (n, p, m, true))
                .or_else(|| {
                    try_output_recovery_with_ovk(
                        &domain,
                        ovk,
                        sout,
                        sout.cv(),
                        sout.out_ciphertext(),
                    )
                    .map(|(n, p, m)| (n, p, m, false))
                });
            let output = r
                .map(|(note, address, memo, incoming)| {
                    let cmx = note.cmu().to_bytes();
                    let note = PlainNote {
                        diversifier: address.diversifier().0,
                        value: note.value.inner(),
                        rcm: note.rcm().to_bytes(),
                        rho: None,
                    };
                    let address = address.to_bytes();
                    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
                    e.write_all(memo.as_slice())?;
                    let memo = e.finish().map_err(Error::msg)?;

                    Ok::<_, Error>(ShieldedOutput {
                        incoming,
                        note,
                        cmx,
                        address,
                        memo,
                    })
                })
                .transpose()?;
            souts.push(output);
        }
    }
    let mut oins = vec![];
    let mut oouts = vec![];
    if let Some(b) = data.orchard_bundle() {
        let orchard = ai.orchard.as_ref().unwrap();
        let ivk =
            orchard::keys::PreparedIncomingViewingKey::new(&orchard.vk.to_ivk(Scope::External));
        let ovk = &orchard.vk.to_ovk(Scope::External);
        for a in b.actions() {
            let spend = get_note_by_nf(connection, &a.nullifier().to_bytes())?;
            oins.push(spend.map(|pn| ShieldedInput {
                note: pn,
                nf: a.nullifier().to_bytes(),
            }));

            let domain = OrchardDomain::for_nullifier(a.nullifier().clone());
            let r = try_note_decryption(&domain, &ivk, a)
                .map(|(n, p, m)| (n, p, m, true))
                .or_else(|| {
                    try_output_recovery_with_ovk(
                        &domain,
                        ovk,
                        a,
                        a.cv_net(),
                        &a.encrypted_note().out_ciphertext,
                    )
                    .map(|(n, p, m)| (n, p, m, false))
                });
            let output = r
                .map(|(note, address, memo, incoming)| {
                    let cmx = ExtractedNoteCommitment::from(note.commitment());
                    let cmx = cmx.to_bytes();
                    let note = PlainNote {
                        diversifier: address.diversifier().as_array().clone(),
                        value: note.value().inner(),
                        rcm: note.rseed().as_bytes().clone(),
                        rho: Some(a.nullifier().to_bytes()),
                    };
                    let address = address.to_raw_address_bytes();
                    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
                    e.write_all(memo.as_slice())?;
                    let memo = e.finish().map_err(Error::msg)?;

                    Ok::<_, Error>(ShieldedOutput {
                        incoming,
                        note,
                        cmx,
                        address,
                        memo,
                    })
                })
                .transpose()?;
            oouts.push(output);
        }
    }

    let ops = tins
        .iter()
        .map(|tin| tin.out_point.clone())
        .collect::<Vec<_>>();
    let txouts = get_txin_coins(*network, url.clone(), ops)?;
    for (tin, txout) in tins.iter_mut().zip(txouts.into_iter()) {
        tin.coin = txout;
    }

    let tx = TransactionDetails {
        height,
        timestamp,
        txid,
        tins,
        touts,
        sins,
        souts,
        oins,
        oouts,
    };
    Ok(tx)
}

pub async fn retrieve_tx_details(
    network: &Network,
    connection: Mutex<PooledSQLConnection>,
    url: String,
) -> Result<()> {
    let txids = list_new_txids(&connection.lock())?;
    let mut client = connect_lwd(&url).await?;
    for (id_tx, account, timestamp, txid) in txids {
        let ai = get_account_info(network, &connection.lock(), account)?;
        let account_addrs = ai.to_addresses(network);
        let rtx = get_tx(&connection.lock(), id_tx)?;
        let (height, tx) = get_transaction(network, &mut client, &txid).await?;
        let txd = analyze_raw_transaction(
            network,
            &connection.lock(),
            url.clone(),
            height,
            timestamp,
            account,
            tx,
        )?;
        let tx_bin = bincode::serialize(&txd)?;
        store_tx_details(&connection.lock(), id_tx, &txid, &tx_bin)?;
        let (tx_address, tx_memo) = get_tx_primary_address_memo(network, &account_addrs, &rtx, &txd)?;
        update_tx_primary_address_memo(&connection.lock(), id_tx, tx_address, tx_memo)?;
    }
    Ok(())
}

pub fn decode_tx_details(
    network: &Network,
    connection: &Connection,
    account: u32,
    id_tx: u32,
    tx: &TransactionDetails,
) -> Result<()> {
    let mut authenticated = false;
    let ai = get_account_info(network, connection, account)?;
    let account_address = ai.to_address(network, PoolMask(7)).unwrap();
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

    let mut contact_decoder =
        ChunkedMemoDecoder::<ChunkedContactV1>::new(tx.souts.len().max(tx.oouts.len()));

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
            contact_decoder.add_memo(&memo.into())?;
        }
    }
    let contacts = contact_decoder.finalize()?;
    tracing::info!("Contacts {:?}", contacts);
    for c in contacts.iter() {
        add_contact(network, connection, account, &c.name, &c.address)?;
    }
    Ok(())
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

pub fn get_tx_primary_address_memo(
    network: &Network,
    addrs: &Addresses,
    tx: &ReceivedTx,
    txd: &TransactionDetails,
) -> Result<(Option<String>, Option<String>)> {
    let mut address = None;
    let mut memo = None;
    'once: loop {
        if tx.value > 0 {
            // incoming
            for tin in txd.tins.iter() {
                address = tin.coin.address.clone();
                break 'once;
            }
        } else {
            if let Some(taddr) = addrs.transparent.as_ref() {
                for tout in txd.touts.iter() {
                    if let Some(tout_addr) = tout.coin.address.as_ref() {
                        if tout_addr != taddr {
                            address = Some(tout_addr.clone());
                            break 'once;
                        }
                    }
                }
            }

            if let Some(saddr) = addrs.sapling.as_ref() {
                for sout in txd.souts.iter() {
                    if let Some(sout) = sout.as_ref() {
                        let pa = PaymentAddress::from_bytes(&sout.address).unwrap();
                        let sout_addr = pa.encode(network);
                        if &sout_addr != saddr {
                            address = Some(sout_addr.clone());
                            let m = decode_shielded_output_memo(sout)?;
                            if let Memo::Text(text) = m {
                                memo = Some(text.to_string());
                            }
                            break 'once;
                        }
                    }
                }
            }

            if let Some(oaddr) = addrs.orchard.as_ref() {
                for oout in txd.oouts.iter() {
                    if let Some(oout) = oout.as_ref() {
                        let oout_addr = ua_of_orchard(&oout.address).encode(network);
                        if &oout_addr != oaddr {
                            address = Some(oout_addr.clone());
                            let m = decode_shielded_output_memo(oout)?;
                            if let Memo::Text(text) = m {
                                memo = Some(text.to_string());
                            }
                            break 'once;
                        }
                    }
                }
            }
        }
        break 'once;
    }

    Ok((address, memo))
}

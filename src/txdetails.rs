use std::io::{Read, Write};

use anyhow::Result;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use orchard::{keys::Scope, note_encryption::OrchardDomain};
use parking_lot::Mutex;
use rusqlite::Connection;
use sapling_crypto::{note_encryption::SaplingDomain, PaymentAddress};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};
use zcash_primitives::{
    consensus::Network,
    memo::Memo,
//    sapling::{note_encryption::SaplingDomain, PaymentAddress},
    transaction::{components::sapling::zip212_enforcement, Transaction as ZTransaction},
};

use crate::{
    account::contacts::{add_contact, ChunkedContactV1, ChunkedMemoDecoder}, coin::connect_lwd, data::fb::{
        InputShieldedT, InputTransparentT, OutputShieldedT, OutputTransparentT, ShieldedMessageT, TransactionInfoExtendedT
    }, db::{
        account::get_account_info,
        notes::{get_note_by_nf, store_tx_details},
        tx::{get_tx, list_new_txids, store_message, update_tx_primary_address_memo},
    }, lwd::{get_transaction, get_txin_coins}, types::{Addresses, PoolMask}, utils::ua::ua_of_orchard, warp::{
        sync::{FullPlainNote, PlainNote, ReceivedTx},
        OutPoint, TxOut2,
    }, Hash, PooledSQLConnection
};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransparentInput {
    pub out_point: OutPoint,
    pub coin: TxOut2,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransparentOutput {
    pub coin: TxOut2,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ShieldedInput {
    #[serde(with = "serde_bytes")]
    pub nf: Hash,
    pub note: Option<PlainNote>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ShieldedOutput {
    #[serde(with = "serde_bytes")]
    pub cmx: Hash,
    pub note: Option<FullPlainNote>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShieldedOutputUncompressed {
    pub incoming: bool,
    pub note: Option<PlainNote>,
    #[serde(with = "serde_bytes")]
    pub cmx: Hash,
    #[serde(with = "serde_bytes")]
    pub address: Option<[u8; 43]>,
    pub memo: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CompressedMemo(pub Vec<u8>);

impl ToString for CompressedMemo {
    fn to_string(&self) -> String {
        let memo = Memo::from_bytes(&self.0).unwrap();
        match memo {
            Memo::Text(txt) => txt.to_string(),
            _ => String::new(),
        }
    }
}

impl Serialize for CompressedMemo {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(self.0.as_slice()).unwrap();
        let memo = e.finish().unwrap();
        s.serialize_bytes(&memo)
    }
}

impl<'de> Deserialize<'de> for CompressedMemo {
    fn deserialize<D>(d: D) -> Result<CompressedMemo, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = Vec::<u8>::deserialize(d)?;
        let mut d = ZlibDecoder::new(&*data);
        let mut memo = vec![];
        d.read_to_end(&mut memo).unwrap();
        Ok(CompressedMemo(memo))
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransactionDetails {
    pub height: u32,
    pub timestamp: u32,
    #[serde(with = "serde_bytes")]
    pub txid: Hash,
    pub tins: Vec<TransparentInput>,
    pub touts: Vec<TransparentOutput>,
    pub sins: Vec<ShieldedInput>,
    pub souts: Vec<ShieldedOutput>,
    pub oins: Vec<ShieldedInput>,
    pub oouts: Vec<ShieldedOutput>,
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
    let zip212_enforcement = zip212_enforcement(network, height.into());
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
        let ivk = sapling_crypto::keys::PreparedIncomingViewingKey::new(
            &ai.sapling.vk.fvk.vk.ivk(),
        );
        let ovk = &ai.sapling.vk.fvk.ovk;
        for sin in b.shielded_spends() {
            let spend = get_note_by_nf(connection, &sin.nullifier().0)?;
            sins.push(ShieldedInput {
                note: spend,
                nf: sin.nullifier().0.clone(),
            });
        }
        for sout in b.shielded_outputs() {
            let domain = SaplingDomain::new(zip212_enforcement);
            let fnote = try_note_decryption(&domain, &ivk, sout)
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
                })
                .map(|(n, p, m, incoming)| FullPlainNote {
                    note: PlainNote {
                        address: p.to_bytes(),
                        value: n.value().inner(),
                        rcm: n.rcm().to_bytes(),
                        rho: None,
                    },
                    memo: CompressedMemo(m.as_slice().to_vec()),
                    incoming,
                });
            let cmx = sout.cmu().to_bytes();
            let output = ShieldedOutput { cmx, note: fnote };
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
            oins.push(ShieldedInput {
                note: spend,
                nf: a.nullifier().to_bytes(),
            });

            let domain = OrchardDomain::for_rho(&a.rho());
            let fnote = try_note_decryption(&domain, &ivk, a)
                .map(|(n, a, m)| (n, a, m, true))
                .or_else(|| {
                    try_output_recovery_with_ovk(
                        &domain,
                        ovk,
                        a,
                        a.cv_net(),
                        &a.encrypted_note().out_ciphertext,
                    )
                    .map(|(n, a, m)| (n, a, m, false))
                })
                .map(|(n, addr, m, incoming)| FullPlainNote {
                    note: PlainNote {
                        address: addr.to_raw_address_bytes(),
                        value: n.value().inner(),
                        rcm: n.rseed().as_bytes().clone(),
                        rho: Some(a.nullifier().to_bytes()),
                    },
                    memo: CompressedMemo(m.to_vec()),
                    incoming,
                });
            let cmx = a.cmx();
            let cmx = cmx.to_bytes();
            let output = ShieldedOutput { cmx, note: fnote };
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
        let (tx_address, tx_memo) =
            get_tx_primary_address_memo(network, &account_addrs, &rtx, &txd)?;
        update_tx_primary_address_memo(&connection.lock(), id_tx, tx_address, tx_memo)?;
        decode_tx_details(network, &connection.lock(), account, id_tx, &txd)?;
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
        if input.note.is_some() {
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
        if let (
            ShieldedOutput {
                note: Some(fnote), ..
            },
            orchard,
        ) = output
        {
            let note_address = if orchard {
                ua_of_orchard(&fnote.note.address).encode(network)
            } else {
                let a = PaymentAddress::from_bytes(&fnote.note.address).unwrap();
                a.encode(network)
            };
            let sender = if fnote.incoming {
                spend_address.clone()
            } else {
                Some(account_address.clone())
            };
            let recipient = note_address;

            let memo = Memo::from_bytes(&fnote.memo.0)?;
            visit_memo(
                connection,
                account,
                id_tx,
                &tx,
                nout as u32,
                fnote.incoming,
                authenticated,
                sender,
                recipient,
                &memo,
            )?;
            contact_decoder.add_memo(&memo.into())?;
        }
    }
    let contacts = contact_decoder.finalize()?;
    for c in contacts.iter() {
        add_contact(connection, account, &c.name, &c.address, true)?;
    }
    Ok(())
}

fn visit_memo(
    connection: &Connection,
    account: u32,
    id_tx: u32,
    tx: &TransactionDetails,
    nout: u32,
    incoming: bool,
    _authenticated: bool,
    sender: Option<String>,
    recipient: String,
    memo: &Memo,
) -> Result<()> {
    match memo {
        Memo::Text(text) => {
            let msg = parse_memo_text(
                id_tx,
                &tx.txid,
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
    txid: &Hash,
    nout: u32,
    height: u32,
    timestamp: u32,
    incoming: bool,
    sender: Option<String>,
    recipient: String,
    memo: &str,
) -> Result<ShieldedMessageT> {
    let memo_lines: Vec<_> = memo.splitn(4, '\n').collect();
    let msg = if memo_lines.len() == 4 && memo_lines[0] == "\u{1F6E1}MSG" {
        ShieldedMessageT {
            id_msg: 0,
            id_tx,
            txid: Some(txid.to_vec()),
            nout,
            height,
            timestamp,
            incoming,
            sender: if memo_lines[1].is_empty() {
                sender
            } else {
                Some(memo_lines[1].to_string())
            },
            recipient: Some(recipient.to_string()),
            subject: Some(memo_lines[2].to_string()),
            body: Some(memo_lines[3].to_string()),
            read: false,
        }
    } else {
        ShieldedMessageT {
            id_msg: 0,
            id_tx,
            txid: Some(txid.to_vec()),
            height,
            timestamp,
            incoming,
            nout,
            sender: None,
            recipient: Some(recipient.to_string()),
            subject: Some(String::new()),
            body: Some(memo.to_string()),
            read: false,
        }
    };
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
                    if let Some(sout) = &sout.note {
                        let pa = PaymentAddress::from_bytes(&sout.note.address).unwrap();
                        let sout_addr = pa.encode(network);
                        if &sout_addr != saddr {
                            address = Some(sout_addr.clone());
                            let m = Memo::from_bytes(&sout.memo.0)?;
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
                    if let Some(oout) = &oout.note {
                        let oout_addr = ua_of_orchard(&oout.note.address).encode(network);
                        if &oout_addr != oaddr {
                            address = Some(oout_addr.clone());
                            let m = Memo::from_bytes(&oout.memo.0)?;
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

impl TransactionDetails {
    pub fn to_transaction_info_ext(self, network: &Network) -> TransactionInfoExtendedT {
        let tins = self
            .tins
            .into_iter()
            .map(|tin| InputTransparentT {
                txid: Some(tin.out_point.txid.to_vec()),
                vout: tin.out_point.vout,
                address: tin.coin.address,
                value: tin.coin.value,
            })
            .collect::<Vec<_>>();
        let touts = self
            .touts
            .into_iter()
            .map(|tout| OutputTransparentT {
                address: tout.coin.address,
                value: tout.coin.value,
            })
            .collect::<Vec<_>>();
        let sins = self
            .sins
            .into_iter()
            .map(|sin| {
                let note = sin.note.as_ref();
                InputShieldedT {
                    nf: Some(sin.nf.to_vec()),
                    address: note.map(|n| {
                        PaymentAddress::from_bytes(&n.address)
                            .unwrap()
                            .encode(network)
                    }),
                    value: note.map(|n| n.value).unwrap_or_default(),
                    rcm: note.map(|n| n.rcm.to_vec()),
                    rho: None,
                }
            })
            .collect::<Vec<_>>();
        let souts = self
            .souts
            .into_iter()
            .map(|sout| {
                let note = sout.note.as_ref();
                OutputShieldedT {
                    cmx: Some(sout.cmx.to_vec()),
                    incoming: note.map(|n| n.incoming).unwrap_or_default(),
                    address: note.map(|n| {
                        PaymentAddress::from_bytes(&n.note.address)
                            .unwrap()
                            .encode(network)
                    }),
                    value: note.map(|n| n.note.value).unwrap_or_default(),
                    rcm: note.map(|n| n.note.rcm.to_vec()),
                    rho: note.map(|n| n.note.rho.map(|r| r.to_vec()).unwrap_or_default()),
                    memo: note.map(|n| n.memo.to_string()),
                }
            })
            .collect::<Vec<_>>();
        let oins = self
            .oins
            .into_iter()
            .map(|sin| {
                let note = sin.note.as_ref();
                InputShieldedT {
                    nf: Some(sin.nf.to_vec()),
                    address: note.map(|n| ua_of_orchard(&n.address).encode(network)),
                    value: note.map(|n| n.value).unwrap_or_default(),
                    rcm: note.map(|n| n.rcm.to_vec()),
                    rho: note.and_then(|n| n.rho.map(|r| r.to_vec())),
                }
            })
            .collect::<Vec<_>>();
        let oouts = self
            .oouts
            .into_iter()
            .map(|sout| {
                let note = sout.note.as_ref();
                OutputShieldedT {
                    cmx: Some(sout.cmx.to_vec()),
                    incoming: note.map(|n| n.incoming).unwrap_or_default(),
                    address: note.map(|n| ua_of_orchard(&n.note.address).encode(network)),
                    value: note.map(|n| n.note.value).unwrap_or_default(),
                    rcm: note.map(|n| n.note.rcm.to_vec()),
                    rho: note.map(|n| n.note.rho.map(|r| r.to_vec()).unwrap_or_default()),
                    memo: note.map(|n| n.memo.to_string()),
                }
            })
            .collect::<Vec<_>>();

        let etx = TransactionInfoExtendedT {
            height: self.height,
            timestamp: self.timestamp,
            txid: Some(self.txid.to_vec()),
            tins: Some(tins),
            touts: Some(touts),
            sins: Some(sins),
            souts: Some(souts),
            oins: Some(oins),
            oouts: Some(oouts),
        };
        etx
    }
}

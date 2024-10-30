use std::io::{Read, Write};

use anyhow::Result;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use orchard::{keys::Scope, note_encryption::OrchardDomain, Address};
use parking_lot::Mutex;
use rusqlite::Connection;
use sapling_crypto::{note_encryption::SaplingDomain, PaymentAddress};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};
use zcash_primitives::{
    memo::Memo,
    transaction::{components::sapling::zip212_enforcement, Transaction as ZTransaction},
};

use crate::{
    account::contacts::{add_contact, ChunkedContactV1, ChunkedMemoDecoder},
    coin::CoinDef,
    data::fb::{
        InputShieldedT, InputTransparentT, OutputShieldedT, OutputTransparentT, ShieldedMessageT,
        TransactionInfoExtendedT, UserMemoT,
    },
    db::{
        account::{get_account_info, list_account_transparent_addresses},
        messages::store_message,
        notes::{get_note_by_nf, list_utxos},
        tx::{get_tx, list_new_txids, store_tx_details, update_tx_primary_address_memo},
    },
    fb_unwrap,
    lwd::{get_transaction, get_txin_coins},
    network::Network,
    types::{Addresses, CheckpointHeight, PoolMask},
    utils::ua::ua_of_orchard,
    warp::{
        sync::{FullPlainNote, PlainNote, ReceivedTx, TransparentNote},
        OutPoint, TxOut2,
    },
    Hash,
};

use warp_macros::c_export;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransparentInput {
    pub out_point: OutPoint,
    pub coin: TxOut2,
    pub note: Option<TransparentNote>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransparentOutput {
    pub coin: TxOut2,
    pub note: Option<TransparentNote>,
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
            Memo::Empty => String::new(),
            Memo::Future(_) => "(Future data)".to_string(),
            Memo::Arbitrary(data) => hex::encode(&*data),
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
    pub value: i64,
    pub tins: Vec<TransparentInput>,
    pub touts: Vec<TransparentOutput>,
    pub sins: Vec<ShieldedInput>,
    pub souts: Vec<ShieldedOutput>,
    pub oins: Vec<ShieldedInput>,
    pub oouts: Vec<ShieldedOutput>,
}

pub fn analyze_raw_transaction(
    coin: &CoinDef,
    network: &Network,
    connection: &Connection,
    account: u32,
    height: u32,
    timestamp: u32,
    tx: ZTransaction,
) -> Result<TransactionDetails> {
    let ai = get_account_info(network, connection, account)?;
    let txid: Hash = tx.txid().as_ref().clone();
    let data = tx.into_data();
    let zip212_enforcement = zip212_enforcement(network, height.into());
    let utxos = list_utxos(connection, account, CheckpointHeight(height))?;
    let account_addresses = list_account_transparent_addresses(connection, account)?;

    let mut tins = vec![];
    let mut touts = vec![];
    if let Some(b) = data.transparent_bundle() {
        for vin in b.vin.iter() {
            let prev_utxo = utxos
                .iter()
                .find(|&utxo| &utxo.txid == vin.prevout.hash() && utxo.vout == vin.prevout.n());
            let note = prev_utxo.map(|n| TransparentNote {
                id: 0,
                address: n.address.clone(),
                value: n.value,
            });
            let tin = TransparentInput {
                out_point: OutPoint {
                    txid: vin.prevout.hash().clone(),
                    vout: vin.prevout.n(),
                },
                coin: TxOut2::default(),
                note,
            };
            tins.push(tin);
        }

        for (n, vout) in b.vout.iter().enumerate() {
            let address = vout.recipient_address().map(|a| a.encode(network));
            let note = address.as_ref().and_then(|a| {
                let note = account_addresses
                    .iter()
                    .find(|&ta| fb_unwrap!(ta.address) == a);
                note
            });
            let value = vout.value.into();
            let note = note.map(|n| TransparentNote {
                id: 0,
                address: n.address.clone().unwrap(),
                value,
            });
            let tout = TransparentOutput {
                coin: TxOut2 {
                    address,
                    value,
                    vout: n as u32,
                },
                note,
            };
            touts.push(tout);
        }
    }
    let mut sins = vec![];
    let mut souts = vec![];
    if let Some(b) = data.sapling_bundle() {
        if let Some(si) = ai.sapling.as_ref() {
            let ivk = sapling_crypto::keys::PreparedIncomingViewingKey::new(&si.vk.fvk().vk.ivk());
            let ovk = &si.vk.fvk().ovk;
            for sin in b.shielded_spends() {
                let spend = get_note_by_nf(connection, account, &sin.nullifier().0)?;
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
                            id: 0,
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
    }
    let mut oins = vec![];
    let mut oouts = vec![];
    if let Some(b) = data.orchard_bundle() {
        if let Some(orchard) = ai.orchard.as_ref() {
            let ivk =
                orchard::keys::PreparedIncomingViewingKey::new(&orchard.vk.to_ivk(Scope::External));
            for a in b.actions() {
                let spend = get_note_by_nf(connection, account, &a.nullifier().to_bytes())?;
                oins.push(ShieldedInput {
                    note: spend,
                    nf: a.nullifier().to_bytes(),
                });

                let domain = OrchardDomain::for_rho(&a.rho());
                let fnote =
                    try_note_decryption(&domain, &ivk, a).map(|(n, addr, m)| FullPlainNote {
                        note: PlainNote {
                            id: 0,
                            address: addr.to_raw_address_bytes(),
                            value: n.value().inner(),
                            rcm: n.rseed().as_bytes().clone(),
                            rho: Some(a.nullifier().to_bytes()),
                        },
                        memo: CompressedMemo(m.to_vec()),
                        incoming: true,
                    });
                let cmx = a.cmx();
                let cmx = cmx.to_bytes();
                let output = ShieldedOutput { cmx, note: fnote };
                oouts.push(output);
            }
        }
    }

    let ops = tins
        .iter()
        .map(|tin| tin.out_point.clone())
        .collect::<Vec<_>>();
    let txouts = get_txin_coins(coin, *network, ops)?;
    for (tin, txout) in tins.iter_mut().zip(txouts.into_iter()) {
        tin.coin = txout;
    }

    let tin_value = tins
        .iter()
        .map(|tin| {
            tin.note
                .as_ref()
                .map(|n| n.value as i64)
                .unwrap_or_default()
        })
        .sum::<i64>();
    let tout_value = touts
        .iter()
        .map(|tout| {
            tout.note
                .as_ref()
                .map(|n| n.value as i64)
                .unwrap_or_default()
        })
        .sum::<i64>();
    let sin_value = sins
        .iter()
        .map(|sin| {
            sin.note
                .as_ref()
                .map(|n| n.value as i64)
                .unwrap_or_default()
        })
        .sum::<i64>();
    let sout_value = souts
        .iter()
        .map(|sout| {
            sout.note
                .as_ref()
                .map(|n| n.note.value as i64)
                .unwrap_or_default()
        })
        .sum::<i64>();
    let oin_value = oins
        .iter()
        .map(|sin| {
            sin.note
                .as_ref()
                .map(|n| n.value as i64)
                .unwrap_or_default()
        })
        .sum::<i64>();
    let oout_value = oouts
        .iter()
        .map(|sout| {
            sout.note
                .as_ref()
                .map(|n| n.note.value as i64)
                .unwrap_or_default()
        })
        .sum::<i64>();
    let value = (tout_value + sout_value + oout_value) - (tin_value + sin_value + oin_value);
    // tracing::info!("{tin_value} {tout_value} {sin_value} {sout_value} {oin_value} {oout_value} = {value}");
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
        value,
    };
    Ok(tx)
}

#[c_export]
pub async fn retrieve_tx_details(
    coin: &CoinDef,
    network: &Network,
    connection: &Connection,
) -> Result<()> {
    let connection = Mutex::new(connection);
    let txids = list_new_txids(&connection.lock())?;
    let mut client = coin.connect_lwd()?;
    for (id_tx, account, timestamp, txid) in txids {
        let ai = get_account_info(network, &connection.lock(), account)?;
        let account_addrs = ai.to_addresses(network);
        let rtx = get_tx(&connection.lock(), id_tx)?;
        let (height, tx) = get_transaction(network, &mut client, &txid).await?;
        let txd = analyze_raw_transaction(
            coin,
            network,
            &connection.lock(),
            account,
            height,
            timestamp,
            tx,
        )?;
        let tx_bin = bincode::serialize(&txd)?;
        store_tx_details(&connection.lock(), id_tx, account, height, &txid, &tx_bin)?;
        let (tx_address, tx_memo) =
            get_tx_primary_address_memo(network, &account_addrs, &rtx, &txd)?;
        update_tx_primary_address_memo(network, &connection.lock(), id_tx, tx_address, tx_memo)?;
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
                ua_of_orchard(&Address::from_raw_address_bytes(&fnote.note.address).unwrap())
                    .encode(network)
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
                network,
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
        add_contact(network, connection, account, &c.name, &c.address, true)?;
    }
    Ok(())
}

fn visit_memo(
    network: &Network,
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
                account,
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
            store_message(network, connection, account, &tx, nout, &msg)?;
        }
        _ => {}
    }
    Ok(())
}

fn parse_memo_text(
    account: u32,
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
    let memo = UserMemoT::from_text(sender.as_deref(), &recipient, &memo);
    let msg = ShieldedMessageT {
        id_msg: 0,
        account,
        id_tx,
        txid: Some(txid.to_vec()),
        nout,
        height,
        timestamp,
        incoming,
        memo: Some(Box::new(memo)),
        contact: None,
        read: false,
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
    let mut self_address = None;
    let mut memo = None;
    if tx.value > 0 {
        // incoming
        for tin in txd.tins.iter() {
            address = tin.coin.address.clone();
        }
    } else if let Some(taddr) = addrs.transparent.as_ref() {
        for tout in txd.touts.iter() {
            if let Some(tout_addr) = tout.coin.address.as_ref() {
                if tout_addr != taddr {
                    address = Some(tout_addr.clone());
                } else {
                    self_address = Some(tout_addr.clone());
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
                } else {
                    self_address = Some(sout_addr.clone());
                }
                if memo.is_none() {
                    let m = Memo::from_bytes(&sout.memo.0)?;
                    if let Memo::Text(text) = m {
                        memo = Some(text.to_string());
                    }
                }
            }
        }
    }

    if let Some(oaddr) = addrs.orchard.as_ref() {
        for oout in txd.oouts.iter() {
            if let Some(oout) = &oout.note {
                let oout_addr =
                    ua_of_orchard(&Address::from_raw_address_bytes(&oout.note.address).unwrap())
                        .encode(network);
                if &oout_addr != oaddr {
                    address = Some(oout_addr.clone());
                } else {
                    self_address = Some(oout_addr.clone());
                }
                if memo.is_none() {
                    let m = Memo::from_bytes(&oout.memo.0)?;
                    if let Memo::Text(text) = m {
                        memo = Some(text.to_string());
                    }
                }
            }
        }
    }
    address = address.or(self_address);

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
                    orchard: false,
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
                    orchard: false,
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
                    orchard: true,
                    nf: Some(sin.nf.to_vec()),
                    address: note.map(|n| {
                        ua_of_orchard(&Address::from_raw_address_bytes(&n.address).unwrap())
                            .encode(network)
                    }),
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
                    orchard: true,
                    cmx: Some(sout.cmx.to_vec()),
                    incoming: note.map(|n| n.incoming).unwrap_or_default(),
                    address: note.map(|n| {
                        ua_of_orchard(&Address::from_raw_address_bytes(&n.note.address).unwrap())
                            .encode(network)
                    }),
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

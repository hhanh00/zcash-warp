use std::io::Write;

use anyhow::{Error, Result};
use flate2::{write::ZlibEncoder, Compression};
use orchard::{keys::Scope, note::ExtractedNoteCommitment, note_encryption::OrchardDomain};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};
use zcash_primitives::{
    consensus::Network, sapling::note_encryption::SaplingDomain,
    transaction::Transaction as ZTransaction,
};

use crate::{
    coin::connect_lwd, db::{account::get_account_info, notes::{get_note_by_nf, store_tx_details}, tx::list_new_txids}, lwd::{get_transaction, get_txin_coins}, warp::{sync::PlainNote, OutPoint, TxOut2}, Hash, PooledSQLConnection
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
pub struct Transaction {
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
) -> Result<Transaction> {
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
                        sout.out_ciphertext()).map(|(n, p, m)| (n, p, m, false))
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
            .map(|(n, p, m)| (n, p, m, true)).or_else(|| {
                try_output_recovery_with_ovk(
                    &domain,
                    ovk,
                    a,
                    a.cv_net(),
                    &a.encrypted_note().out_ciphertext,
                ).map(|(n, p, m)| (n, p, m, false))
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

    let tx = Transaction {
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
        let (height, tx) = get_transaction(network, &mut client, &txid).await?;
        let tx_details = analyze_raw_transaction(
            network,
            &connection.lock(),
            url.clone(),
            height,
            timestamp,
            account,
            tx,
        )?;
        let tx_bin = bincode::serialize(&tx_details)?;
        store_tx_details(&connection.lock(), id_tx, &txid, &tx_bin)?;
    }
    Ok(())
}

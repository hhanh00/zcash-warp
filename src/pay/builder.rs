use crate::{
    db::account::{get_account_info, list_account_tsk},
    keys::TSKStore,
    warp::{
        hasher::{empty_roots, OrchardHasher, SaplingHasher},
        MERKLE_DEPTH,
    },
};
use anyhow::Result;
use sapling_crypto::{note_encryption::Zip212Enforcement, PaymentAddress};
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_protocol::value::Zatoshis;

use super::{
    InputNote, OutputNote, UnsignedTransaction, ORCHARD_PROVER, PROVER,
};
use jubjub::Fr;
use orchard::{
    builder::{Builder as OrchardBuilder, BundleType},
    bundle::Flags,
    keys::{Scope, SpendAuthorizingKey},
    note::Rho,
    tree::MerkleHashOrchard,
    Address,
};
use rand::{CryptoRng, RngCore};
use rusqlite::Connection;
use zcash_primitives::{
    consensus::{BlockHeight, BranchId},
    legacy::TransparentAddress,
    transaction::{
        components::{transparent::builder::TransparentBuilder, OutPoint, TxOut},
        sighash::{signature_hash, SignableInput},
        txid::TxIdDigester,
        TransactionData, TxVersion,
    },
};
use zcash_proofs::prover::LocalTxProver;

use warp_macros::c_export;
use crate::{network::Network, coin::COINS, ffi::{map_result, CParam, CResult}};

const DUST: u64 = 54;

impl UnsignedTransaction {
    pub fn build<R: RngCore + CryptoRng>(
        &self,
        network: &Network,
        connection: &Connection,
        expiration_height: u32,
        tsk_store: &mut TSKStore,
        mut rng: R,
    ) -> Result<Vec<u8>> {
        let ai = get_account_info(network, connection, self.account)?;
        if ai.fingerprint != self.account_id {
            anyhow::bail!("Invalid Account");
        }
        let sks = ai.to_secret_keys();
        sks.sapling.ok_or(anyhow::anyhow!("No Secret Keys"))?;

        if let Some(_ti) = ai.transparent.as_ref() {
            let tsks = list_account_tsk(connection, self.account)?;
            for tsk in tsks.iter() {
                tsk_store.0.insert(tsk.address.clone(), tsk.sk.clone());
            }
        }

        let er = [
            empty_roots(&SaplingHasher::default()),
            empty_roots(&OrchardHasher::default()),
        ];

        let mut transparent_builder = TransparentBuilder::empty();
        let mut sapling_builder = sapling_crypto::builder::Builder::new(
            Zip212Enforcement::On,
            sapling_crypto::builder::BundleType::Transactional {
                bundle_required: false,
            },
            sapling_crypto::Anchor::from_bytes(self.roots[0].clone())
                .unwrap()
                .into(),
        );
        let mut orchard_builder = OrchardBuilder::new(
            BundleType::Transactional {
                flags: Flags::from_byte(3).unwrap(),
                bundle_required: false,
            },
            orchard::Anchor::from_bytes(self.roots[1].clone()).unwrap(),
        );

        for txin in self.tx_notes.iter() {
            match &txin.note {
                InputNote::Transparent {
                    txid,
                    vout,
                    address,
                } => {
                    let Some(sk) = tsk_store.0.get(address) else {
                        anyhow::bail!("No Secret Key for address {}", address);
                    };
                    let ta = TransparentAddress::decode(network, address)?;
                    tracing::info!("{} {}", address, txin.amount);
                    transparent_builder
                        .add_input(
                            sk.clone(),
                            OutPoint::new(txid.clone(), *vout),
                            TxOut {
                                value: Zatoshis::from_u64(txin.amount).unwrap(),
                                script_pubkey: ta.script(),
                            },
                        )
                        .map_err(anyhow::Error::msg)?;
                }

                InputNote::Sapling {
                    address,
                    rseed,
                    witness,
                } => {
                    let extsk = ai.sapling.as_ref().and_then(|si| si.sk.as_ref());
                    let extsk = extsk.ok_or(anyhow::anyhow!("No sapling secret key"))?;
                    let recipient = PaymentAddress::from_bytes(address).unwrap();
                    let note = sapling_crypto::Note::from_parts(
                        recipient,
                        sapling_crypto::value::NoteValue::from_raw(txin.amount),
                        sapling_crypto::Rseed::BeforeZip212(Fr::from_bytes(&rseed).unwrap()),
                    );
                    let auth_path = witness.build_auth_path(&self.edges[0], &er[0]);
                    let mut mp = vec![];
                    for i in 0..MERKLE_DEPTH as usize {
                        mp.push(sapling_crypto::Node::from_bytes(auth_path.0[i]).unwrap());
                    }
                    let merkle_path = sapling_crypto::MerklePath::from_parts(
                        mp,
                        (witness.position as u64).into(),
                    )
                    .unwrap();
                    sapling_builder
                        .add_spend(&extsk, note, merkle_path)
                        .map_err(anyhow::Error::msg)?;
                }

                InputNote::Orchard {
                    address,
                    rseed,
                    rho,
                    witness,
                } => {
                    let vk = ai
                        .orchard
                        .as_ref()
                        .map(|oi| oi.vk.clone())
                        .ok_or(anyhow::anyhow!("No Orchard Account"))?;
                    let recipient = Address::from_raw_address_bytes(address).unwrap();
                    let rho = Rho::from_bytes(rho).unwrap();
                    let rseed = orchard::note::RandomSeed::from_bytes(rseed.clone(), &rho).unwrap();
                    let note = orchard::Note::from_parts(
                        recipient,
                        orchard::value::NoteValue::from_raw(txin.amount),
                        rho,
                        rseed,
                    )
                    .unwrap();
                    let auth_path = witness.build_auth_path(&self.edges[1], &er[1]);
                    let auth_path = auth_path
                        .0
                        .iter()
                        .map(|a| MerkleHashOrchard::from_bytes(a).unwrap())
                        .collect::<Vec<_>>();
                    let auth_path: [MerkleHashOrchard; MERKLE_DEPTH as usize] =
                        auth_path.try_into().unwrap();
                    let merkle_path =
                        orchard::tree::MerklePath::from_parts(witness.position as u32, auth_path);

                    orchard_builder
                        .add_spend(vk, note, merkle_path)
                        .map_err(anyhow::Error::msg)?;
                }
            }
        }

        for txout in self.tx_outputs.iter() {
            if txout.is_change && txout.amount < DUST { continue; }
            match &txout.note {
                OutputNote::Transparent { pkh, address } => {
                    let taddr = if *pkh {
                        TransparentAddress::PublicKeyHash(address.clone())
                    } else {
                        TransparentAddress::ScriptHash(address.clone())
                    };
                    transparent_builder
                        .add_output(&taddr, Zatoshis::from_u64(txout.amount).unwrap())
                        .map_err(anyhow::Error::msg)?;
                }
                OutputNote::Sapling { address, memo } => {
                    let vk = ai.sapling.as_ref().map(|si| &si.vk);
                    let ovk = vk.map(|vk| vk.fvk.ovk);
                    let recipient = PaymentAddress::from_bytes(address).unwrap();
                    sapling_builder
                        .add_output(
                            ovk,
                            recipient,
                            sapling_crypto::value::NoteValue::from_raw(txout.amount),
                            Some(memo.as_array().clone()),
                        )
                        .map_err(anyhow::Error::msg)?;
                }
                OutputNote::Orchard { address, memo } => {
                    let vk = ai.orchard.as_ref().map(|oi| oi.vk.clone());
                    let ovk = vk.map(|vk| vk.to_ovk(Scope::External));
                    let recipient = orchard::Address::from_raw_address_bytes(address).unwrap();
                    orchard_builder
                        .add_output(
                            ovk,
                            recipient,
                            orchard::value::NoteValue::from_raw(txout.amount),
                            Some(memo.as_array().clone()),
                        )
                        .map_err(anyhow::Error::msg)?;
                }
            }
        }

        let transparent_bundle = transparent_builder.build();
        let prover = PROVER.lock();
        let prover = prover.as_ref().ok_or(anyhow::anyhow!("Sapling prover not initialized"))?;
        let sapling_bundle = sapling_builder
            .build::<LocalTxProver, LocalTxProver, _, _>(&mut rng)
            .unwrap()
            .map(|pair| pair.0);
        let sapling_bundle =
            sapling_bundle.map(|sb| sb.create_proofs(prover, prover, &mut rng, ()));

        let has_orchard = self.tx_notes.iter().any(|n| match n.note {
            InputNote::Orchard { .. } => true,
            _ => false,
        }) || self.tx_outputs.iter().any(|o| match o.note {
            OutputNote::Orchard { .. } => true,
            _ => false,
        });

        let mut orchard_bundle = None;
        if has_orchard {
            orchard_bundle = orchard_builder.build(&mut rng).unwrap().map(|pair| pair.0);
        }

        let consensus_branch_id = BranchId::for_height(network, BlockHeight::from_u32(self.height));
        let version = TxVersion::suggested_for_branch(consensus_branch_id);

        let unauthed_tx: TransactionData<zcash_primitives::transaction::Unauthorized> =
            TransactionData::from_parts(
                version,
                consensus_branch_id,
                0,
                BlockHeight::from_u32(expiration_height),
                transparent_bundle,
                None,
                sapling_bundle,
                orchard_bundle,
            );

        let txid_parts = unauthed_tx.digest(TxIdDigester);
        let sig_hash = signature_hash(&unauthed_tx, &SignableInput::Shielded, &txid_parts);
        let sig_hash: [u8; 32] = sig_hash.as_ref().clone();

        let transparent_bundle = unauthed_tx
            .transparent_bundle()
            .map(|tb| tb.clone().apply_signatures(&unauthed_tx, &txid_parts));

        let ask = ai.sapling.as_ref().and_then(|si| si.sk.as_ref().map(|sk| &sk.expsk.ask));
        let mut signing_keys = vec![];
        if let Some(ask) = ask {
            signing_keys.push(ask.clone());
        }
        let sapling_bundle = unauthed_tx.sapling_bundle().map(|sb| {
            sb.clone()
                .apply_signatures(&mut rng, sig_hash, &signing_keys)
                .unwrap()
        });

        let orchard_bundle = unauthed_tx.orchard_bundle().map(|ob| {
            let sk = ai.orchard.as_ref().and_then(|oi| oi.sk).unwrap();
            let sak = SpendAuthorizingKey::from(&sk);
            let proven = ob.clone().create_proof(&ORCHARD_PROVER, &mut rng).unwrap();
            proven
                .apply_signatures(&mut rng, sig_hash, std::slice::from_ref(&sak))
                .unwrap()
        });

        let tx_data: TransactionData<zcash_primitives::transaction::Authorized> =
            TransactionData::from_parts(
                version,
                consensus_branch_id,
                0,
                BlockHeight::from_u32(expiration_height),
                transparent_bundle,
                None,
                sapling_bundle,
                orchard_bundle,
            );
        let tx = tx_data.freeze().unwrap();

        let mut tx_bytes = vec![];
        tx.write(&mut tx_bytes).unwrap();

        Ok(tx_bytes)
    }
}

#[c_export]
pub fn init_sapling_prover(spend: &[u8], output: &[u8]) -> Result<()> {
    let prover = LocalTxProver::from_bytes(spend, output);
    *PROVER.lock() = Some(prover);
    Ok(())
}

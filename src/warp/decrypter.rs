use std::sync::mpsc::Sender;

use crate::{
    network::Network,
    lwd::rpc::{CompactOrchardAction, CompactSaplingOutput},
    warp::{
        sync::{ReceivedNote, ReceivedTx},
        Witness,
    },
};

use anyhow::Result;
use blake2b_simd::Params;
use chacha20::{
    cipher::{KeyIvInit, StreamCipher, StreamCipherSeek},
    ChaCha20,
};
use group::{ff::PrimeField as _, Curve as _, GroupEncoding};
use halo2_proofs::pasta::{pallas::Point, Fq};
use orchard::{
    keys::IncomingViewingKey,
    note::{ExtractedNoteCommitment, Rho},
    note_encryption::OrchardDomain,
};
use sapling_crypto::{
    note_encryption::{plaintext_version_is_valid, SaplingDomain, KDF_SAPLING_PERSONALIZATION},
    SaplingIvk,
};
use zcash_note_encryption::COMPACT_NOTE_SIZE;
use zcash_primitives::transaction::components::sapling::zip212_enforcement;

pub fn try_sapling_decrypt(
    network: &Network,
    ivks: &[(u32, SaplingIvk)],
    height: u32,
    timestamp: u32,
    ivtx: u32,
    vout: u32,
    co: &CompactSaplingOutput,
    sender: &mut Sender<ReceivedNote>,
) -> Result<()> {
    let epkb = &*co.epk;
    let epk = jubjub::AffinePoint::from_bytes(epkb.try_into().unwrap()).unwrap();
    let enc = &co.ciphertext;
    let epk = epk.mul_by_cofactor().to_niels();
    let zip212_enforcement = zip212_enforcement(network, height.into());
    for (account, ivk) in ivks {
        let ka = epk.multiply_bits(&ivk.to_repr()).to_affine();
        let key = Params::new()
            .hash_length(32)
            .personal(KDF_SAPLING_PERSONALIZATION)
            .to_state()
            .update(&ka.to_bytes())
            .update(epkb)
            .finalize();
        let mut plaintext = [0; COMPACT_NOTE_SIZE];
        plaintext.copy_from_slice(enc);
        let mut keystream = ChaCha20::new(key.as_ref().into(), [0u8; 12][..].into());
        keystream.seek(64);
        keystream.apply_keystream(&mut plaintext);
        if (plaintext[0] == 0x01 || plaintext[0] == 0x02)
            && plaintext_version_is_valid(zip212_enforcement, plaintext[0])
        {
            use zcash_note_encryption::Domain;
            let pivk = sapling_crypto::keys::PreparedIncomingViewingKey::new(&ivk);
            let d = SaplingDomain::new(zip212_enforcement);
            if let Some((note, recipient)) =
                d.parse_note_plaintext_without_memo_ivk(&pivk, &plaintext)
            {
                let cmx = note.cmu();
                if &cmx.to_bytes() == &*co.cmu {
                    let value = note.value().inner();
                    let note = ReceivedNote {
                        is_new: true,
                        id: 0,
                        account: *account,
                        position: 0,
                        height,
                        address: recipient.to_bytes(),
                        value,
                        rcm: note.rcm().to_bytes(),
                        rho: None,
                        tx: ReceivedTx {
                            id: 0,
                            account: *account,
                            height,
                            txid: [0u8; 32],
                            timestamp,
                            ivtx,
                            value: value as i64,
                        },
                        vout,
                        witness: Witness::default(),
                        nf: [0u8; 32],
                        spent: None,
                    };
                    sender.send(note)?;
                }
            }
        }
    }
    Ok(())
}

const KDF_ORCHARD_PERSONALIZATION: &[u8; 16] = b"Zcash_OrchardKDF";

pub fn try_orchard_decrypt(
    network: &Network,
    ivks: &[(u32, IncomingViewingKey)],
    height: u32,
    timestamp: u32,
    ivtx: u32,
    vout: u32,
    ca: &CompactOrchardAction,
    sender: &mut Sender<ReceivedNote>,
) -> Result<()> {
    let zip212_enforcement = zip212_enforcement(network, height.into());
    for (account, ivk) in ivks {
        let bb = ivk.to_bytes();
        let ivk_fq = Fq::from_repr(bb[32..64].try_into().unwrap()).unwrap();

        let epk = Point::from_bytes(&ca.ephemeral_key.clone().try_into().unwrap())
            .unwrap()
            .to_affine();
        let ka = epk * ivk_fq;
        let key = Params::new()
            .hash_length(32)
            .personal(KDF_ORCHARD_PERSONALIZATION)
            .to_state()
            .update(&ka.to_bytes())
            .update(&ca.ephemeral_key)
            .finalize();
        let mut plaintext = [0; COMPACT_NOTE_SIZE];
        plaintext.copy_from_slice(&ca.ciphertext);
        let mut keystream = ChaCha20::new(key.as_ref().into(), [0u8; 12][..].into());
        keystream.seek(64);
        keystream.apply_keystream(&mut plaintext);

        if (plaintext[0] == 0x01 || plaintext[0] == 0x02)
            && plaintext_version_is_valid(zip212_enforcement, plaintext[0])
        {
            use zcash_note_encryption::Domain;
            let pivk = orchard::keys::PreparedIncomingViewingKey::new(&ivk);
            let rho = Rho::from_bytes(&ca.nullifier.clone().try_into().unwrap()).unwrap();
            let d = OrchardDomain::for_rho(&rho);
            if let Some((note, recipient)) =
                d.parse_note_plaintext_without_memo_ivk(&pivk, &plaintext)
            {
                let cmx = ExtractedNoteCommitment::from(note.commitment());
                let value = note.value().inner();
                if &cmx.to_bytes() == &*ca.cmx {
                    let note = ReceivedNote {
                        is_new: true,
                        id: 0,
                        account: *account,
                        position: 0,
                        height,
                        address: recipient.to_raw_address_bytes(),
                        value,
                        rcm: note.rseed().as_bytes().clone(),
                        rho: Some(rho.to_bytes()),
                        tx: ReceivedTx {
                            id: 0,
                            account: *account,
                            height,
                            txid: [0u8; 32],
                            timestamp,
                            ivtx,
                            value: value as i64,
                        },
                        vout,
                        witness: Witness::default(),
                        nf: [0u8; 32],
                        spent: None,
                    };
                    sender.send(note)?;
                }
            }
        }
    }
    Ok(())
}

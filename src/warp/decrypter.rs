use super::Hash;
use anyhow::Result;
use blake2b_simd::Params;
use chacha20::{
    cipher::{KeyIvInit, StreamCipher, StreamCipherSeek},
    ChaCha20,
};
use group::{ff::PrimeField as _, Curve as _, GroupEncoding};
use halo2_proofs::pasta::{pallas::Point, Fq};
use orchard::keys::IncomingViewingKey;
use zcash_note_encryption::COMPACT_NOTE_SIZE;
use zcash_primitives::sapling::{
    note_encryption::KDF_SAPLING_PERSONALIZATION, SaplingIvk,
};

pub fn try_sapling_decrypt(ivks: &[SaplingIvk], outputs: &[(Hash, &[u8])]) -> Result<()> {
    let epks = jubjub::AffinePoint::batch_from_bytes(outputs.iter().map(|(epk, _)| epk.clone()).into_iter())
        .into_iter()
        .map(|p| p.unwrap())
        .collect::<Vec<_>>();
    for (epk, (epkb, enc)) in epks.iter().zip(outputs) {
        let epk = epk.mul_by_cofactor().to_niels();
        for ivk in ivks {
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
            println!("{}", hex::encode(&plaintext));
        }
    }
    Ok(())
}

const KDF_ORCHARD_PERSONALIZATION: &[u8; 16] = b"Zcash_OrchardKDF";

pub fn try_orchard_decrypt(ivks: &[IncomingViewingKey], outputs: &[(Hash, &[u8])]) -> Result<()> {
    let ivks = ivks.iter().map(|ivk| {
        let bb = ivk.to_bytes();
        Fq::from_repr(bb[32..64].try_into().unwrap()).unwrap()
    }
    ).collect::<Vec<_>>();

    for (epkb, enc) in outputs {
        let epk = Point::from_bytes(epkb).unwrap().to_affine();
        for ivk in ivks.iter() {
            let ka = epk * ivk;
            let key = Params::new()
                .hash_length(32)
                .personal(KDF_ORCHARD_PERSONALIZATION)
                .to_state()
                .update(&ka.to_bytes())
                .update(epkb)
                .finalize();
            let mut plaintext = [0; COMPACT_NOTE_SIZE];
            plaintext.copy_from_slice(enc);
            let mut keystream = ChaCha20::new(key.as_ref().into(), [0u8; 12][..].into());
            keystream.seek(64);
            keystream.apply_keystream(&mut plaintext);
            println!("{}", hex::encode(&plaintext));
        }
    }
    Ok(())
}

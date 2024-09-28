use anyhow::Result;
use base58check::{FromBase58Check, ToBase58Check};
use bip39::{Mnemonic, Seed};
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use rand::{rngs::OsRng, CryptoRng, RngCore};
use ripemd::{Digest as _, Ripemd160};
use sapling_crypto::zip32::ExtendedSpendingKey;
use secp256k1::{All, PublicKey, Secp256k1, SecretKey};
use sha2::Sha256;
use zcash_protocol::consensus::NetworkConstants as _;
use tiny_hderive::bip32::ExtendedPrivKey;
use zcash_primitives::legacy::TransparentAddress;
use zip32::ChildIndex;

use crate::types::{OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo};

use crate::{
    network::Network,
    coin::COINS,
    ffi::{map_result_string, CResult},
};
use std::ffi::c_char;
use warp_macros::c_export;

pub fn generate_random_mnemonic_phrase<R: RngCore + CryptoRng>(mut rng: R) -> String {
    let mut entropy = [0u8; 32];
    rng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy(&entropy, bip39::Language::English)
        .unwrap()
        .into_phrase()
}

#[c_export]
pub fn generate_random_mnemonic_phrase_os_rng() -> Result<String> {
    Ok(generate_random_mnemonic_phrase(OsRng))
}

pub fn export_sk_bip38(sk: &SecretKey) -> String {
    let mut v = sk.as_ref().to_vec();
    v.push(0x01);
    v.to_base58check(0x80)
}

pub fn import_sk_bip38(key: &str) -> Result<SecretKey> {
    let (_, sk) = key
        .from_base58check()
        .map_err(|_| anyhow::anyhow!("Not Base58 Encoded"))?;
    let sk = &sk[0..sk.len() - 1]; // remove compressed pub key marker
    let secret_key = SecretKey::from_slice(&sk)?;
    Ok(secret_key)
}

pub fn derive_zip32(network: &Network, seed: &Seed, acc_index: u32) -> SaplingAccountInfo {
    let master = ExtendedSpendingKey::master(seed.as_bytes());
    let path = [
        ChildIndex::hardened(32),
        ChildIndex::hardened(network.coin_type()),
        ChildIndex::hardened(acc_index),
    ];
    let sk = ExtendedSpendingKey::from_path(&master, &path);
    SaplingAccountInfo::from_sk(&sk)
}

pub fn derive_bip32(
    network: &Network,
    seed: &Seed,
    acc_index: u32,
    change: u32,
    addr_index: u32,
    compressed: bool,
) -> TransparentAccountInfo {
    let bip44_path = format!(
        "m/44'/{}'/{}'/{}/{}",
        network.coin_type(),
        acc_index,
        change,
        addr_index
    );
    let ext = ExtendedPrivKey::derive(seed.as_bytes(), &*bip44_path).unwrap();
    let sk = SecretKey::from_slice(&ext.secret()).unwrap();
    TransparentAccountInfo::from_secret_key(&sk, compressed)
}

impl TransparentAccountInfo {
    pub fn from_secret_key(sk: &SecretKey, compressed: bool) -> Self {
        let secp = Secp256k1::<All>::new();
        let pub_key = PublicKey::from_secret_key(&secp, &sk);
        let pub_key = if compressed {
            pub_key.serialize().to_vec()
        } else {
            pub_key.serialize_uncompressed().to_vec()
        };
        let pub_key = Ripemd160::digest(&Sha256::digest(&pub_key));
        let addr = TransparentAddress::PublicKeyHash(pub_key.into());
        TransparentAccountInfo { index: None, sk: Some(sk.clone()), addr }
    }
}

pub fn derive_orchard_zip32(network: &Network, seed: &Seed, acc_index: u32) -> OrchardAccountInfo {
    let sk = SpendingKey::from_zip32_seed(
        seed.as_bytes(),
        network.coin_type(),
        acc_index.try_into().unwrap(),
    )
    .unwrap();
    let vk = FullViewingKey::from(&sk);
    let addr = vk.address_at(0u64, Scope::External);

    OrchardAccountInfo {
        sk: Some(sk),
        vk,
        addr,
    }
}

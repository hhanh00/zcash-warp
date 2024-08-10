use anyhow::Result;
use base58check::{FromBase58Check, ToBase58Check};
use bip39::{Mnemonic, Seed};
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use rand::{CryptoRng, RngCore};
use ripemd::{Digest as _, Ripemd160};
use secp256k1::{All, PublicKey, Secp256k1, SecretKey};
use sha2::Sha256;
use tiny_hderive::bip32::ExtendedPrivKey;
use zcash_primitives::{
    consensus::{Network, Parameters as _},
    legacy::TransparentAddress,
    zip32::{ChildIndex, ExtendedSpendingKey},
};

use crate::types::{OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo};

pub fn generate_random_mnemonic_phrase<R: RngCore + CryptoRng>(mut rng: R) -> String {
    let mut entropy = [0u8; 32];
    rng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy(&entropy, bip39::Language::English).unwrap().into_phrase()
}

pub fn export_sk_bip38(sk: &SecretKey) -> String {
    let mut v = sk.serialize_secret().to_vec();
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
        ChildIndex::Hardened(32),
        ChildIndex::Hardened(network.coin_type()),
        ChildIndex::Hardened(acc_index),
    ];
    let sk = ExtendedSpendingKey::from_path(&master, &path);
    #[allow(deprecated)]
    let vk = sk.to_extended_full_viewing_key();
    let (_, addr) = vk.default_address();

    SaplingAccountInfo {
        sk: Some(sk),
        vk,
        addr,
    }
}

pub fn derive_bip32(
    network: &Network,
    seed: &Seed,
    acc_index: u32,
    addr_index: u32,
) -> TransparentAccountInfo {
    let bip44_path = format!(
        "m/44'/{}'/{}'/0/{}",
        network.coin_type(),
        acc_index,
        addr_index
    );
    let ext = ExtendedPrivKey::derive(seed.as_bytes(), &*bip44_path).unwrap();
    let sk = SecretKey::from_slice(&ext.secret()).unwrap();
    let secp = Secp256k1::<All>::new();
    let pub_key = PublicKey::from_secret_key(&secp, &sk);
    let pub_key = pub_key.serialize();
    let pub_key = Ripemd160::digest(&Sha256::digest(&pub_key));
    let addr = TransparentAddress::PublicKey(pub_key.into());
    TransparentAccountInfo { sk, addr }
}

pub fn derive_orchard_zip32(network: &Network, seed: &Seed, acc_index: u32) -> OrchardAccountInfo {
    let sk =
        SpendingKey::from_zip32_seed(seed.as_bytes(), network.coin_type(), acc_index).unwrap();
    let vk = FullViewingKey::from(&sk);
    let addr = vk.address_at(0u64, Scope::External);

    OrchardAccountInfo {
        sk: Some(sk),
        vk,
        addr,
    }
}

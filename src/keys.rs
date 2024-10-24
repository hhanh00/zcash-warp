use anyhow::Result;
use base58check::{FromBase58Check, ToBase58Check};
use bip32::{ExtendedPrivateKey, ExtendedPublicKey};
use bip39::Mnemonic;
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use prost::bytes::BufMut as _;
use rand::{rngs::OsRng, CryptoRng, RngCore};
use ripemd::{Digest as _, Ripemd160};
use sapling_crypto::zip32::{
    DiversifiableFullViewingKey, ExtendedFullViewingKey, ExtendedSpendingKey,
};
use secp256k1::{All, PublicKey, Secp256k1, SecretKey};
use sha2::Sha256;
use zcash_keys::keys::{UnifiedAddressRequest, UnifiedSpendingKey};
use zcash_primitives::legacy::keys::{
    AccountPrivKey, AccountPubKey, IncomingViewingKey as _, NonHardenedChildIndex,
    TransparentKeyScope,
};
use zcash_primitives::legacy::TransparentAddress;
use zip32::{AccountId, DiversifierIndex};

use crate::db::account_manager::parse_seed_phrase;
use crate::types::{OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo};

use crate::{
    coin::COINS,
    ffi::{map_result_string, CResult},
    network::Network,
};
use std::ffi::c_char;
use std::str::FromStr;
use warp_macros::c_export;

#[derive(Debug)]
pub struct AccountKeys {
    pub seed: Option<String>,
    pub aindex: u32,
    pub dindex: u32,
    pub cindex: Option<u32>,
    pub txsk: Option<AccountPrivKey>,
    pub tsk: Option<SecretKey>,
    pub tvk: Option<AccountPubKey>,
    pub taddr: Option<TransparentAddress>,
    pub ssk: Option<ExtendedSpendingKey>,
    pub svk: Option<DiversifiableFullViewingKey>,
    pub osk: Option<SpendingKey>,
    pub ovk: Option<FullViewingKey>,
}

impl AccountKeys {
    pub fn from_seed(network: &Network, phrase: &str, acc_index: u32) -> Result<Self> {
        let seed = parse_seed_phrase(phrase)?;
        let usk = UnifiedSpendingKey::from_seed(
            network,
            seed.as_bytes(),
            AccountId::try_from(acc_index).unwrap(),
        )?;
        let uvk = usk.to_unified_full_viewing_key();
        let (_, di) = usk.default_address(UnifiedAddressRequest::all().unwrap());
        let di: u32 = di.try_into().unwrap();
        let addr_index = NonHardenedChildIndex::from_index(di).unwrap();
        let txsk = usk.transparent().clone();
        let tsk = usk
            .transparent()
            .derive_secret_key(TransparentKeyScope::EXTERNAL, addr_index)
            .unwrap();
        let tvk = uvk.transparent().cloned();
        let taddr = tvk
            .as_ref()
            .map(|tvk| TransparentAccountInfo::derive_address(tvk, 0, di));

        Ok(AccountKeys {
            seed: Some(phrase.to_string()),
            aindex: acc_index,
            dindex: di,
            cindex: None,
            txsk: Some(txsk),
            tsk: Some(tsk),
            tvk,
            taddr,
            ssk: Some(usk.sapling().clone()),
            svk: uvk.sapling().cloned(),
            osk: Some(usk.orchard().clone()),
            ovk: uvk.orchard().cloned(),
        })
    }

    pub fn to_transparent(&self) -> Option<TransparentAccountInfo> {
        if let Some(taddr) = self.taddr.as_ref() {
            Some(TransparentAccountInfo {
                index: self.dindex,
                change_index: self.cindex,
                xsk: self.txsk.clone(),
                sk: self.tsk.clone(),
                vk: self.tvk.clone(),
                addr: taddr.clone(),
            })
        } else {
            None
        }
    }

    pub fn to_sapling(&self) -> Option<SaplingAccountInfo> {
        if let Some(svk) = self.svk.as_ref() {
            let di = DiversifierIndex::try_from(self.dindex).unwrap();
            let addr = svk.address(di).unwrap();
            Some(SaplingAccountInfo {
                sk: self.ssk.clone(),
                vk: svk.clone(),
                addr,
            })
        } else {
            None
        }
    }

    pub fn to_orchard(&self) -> Option<OrchardAccountInfo> {
        if let Some(ovk) = self.ovk.as_ref() {
            let di = DiversifierIndex::try_from(self.dindex).unwrap();
            let addr = ovk.address_at(di, Scope::External);
            Some(OrchardAccountInfo {
                sk: self.osk.clone(),
                vk: ovk.clone(),
                addr,
            })
        } else {
            None
        }
    }

    fn _check_invariants(&self) {
        // if seed -> di, tsk, tvk, taddr, ssk, svk, osk, ovk
        // if ssk -> di, ssk, svk
        // if svk -> di, svk
        // if usk -> same as seed
        // if uvk -> di, tvk, taddr, svk, ovk
        // if tsk -> tsk, taddr (NO di)
        // if taddr -> taddr (NO di)
    }
}

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

pub fn decode_extended_private_key(key: &str) -> Result<AccountPrivKey> {
    let exsk = ExtendedPrivateKey::<SecretKey>::from_str(key)?;
    let xsk = AccountPrivKey::from_extended_privkey(exsk);
    Ok(xsk)
}

pub fn decode_extended_public_key(key: &str) -> Result<AccountPubKey> {
    let exvk = ExtendedPublicKey::<PublicKey>::from_str(key)?;
    let mut bytes = [0u8; 65];
    bytes[..32].copy_from_slice(&exvk.attrs().chain_code);
    bytes[32..].copy_from_slice(&exvk.public_key().serialize());
    let vk = AccountPubKey::deserialize(&bytes)?;
    Ok(vk)
}

pub fn import_sk_bip38(key: &str) -> Result<SecretKey> {
    let (_, sk) = key
        .from_base58check()
        .map_err(|_| anyhow::anyhow!("Not Base58 Encoded"))?;
    let sk = &sk[0..sk.len() - 1]; // remove compressed pub key marker
    let secret_key = SecretKey::from_slice(&sk)?;
    Ok(secret_key)
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
        TransparentAccountInfo {
            index: 0,
            change_index: None,
            xsk: None,
            sk: Some(sk.clone()),
            vk: None,
            addr,
        }
    }

    pub fn derive_sk(xsk: &AccountPrivKey, external: u32, addr_index: u32) -> SecretKey {
        let addr_index = NonHardenedChildIndex::from_index(addr_index).unwrap();
        match external {
            0 => xsk.derive_external_secret_key(addr_index).unwrap(),
            1 => xsk.derive_internal_secret_key(addr_index).unwrap(),
            _ => unreachable!(),
        }
    }

    pub fn derive_address(
        tvk: &AccountPubKey,
        external: u32,
        addr_index: u32,
    ) -> TransparentAddress {
        let addr_index = NonHardenedChildIndex::from_index(addr_index).unwrap();
        match external {
            0 => tvk
                .derive_external_ivk()
                .unwrap()
                .derive_address(addr_index)
                .unwrap(),
            1 => tvk
                .derive_internal_ivk()
                .unwrap()
                .derive_address(addr_index)
                .unwrap(),
            _ => unreachable!(),
        }
    }
}

pub fn to_extended_full_viewing_key(
    dk: &DiversifiableFullViewingKey,
) -> Result<ExtendedFullViewingKey> {
    let mut b = vec![];
    b.put_u8(0);
    b.put_u32(0);
    b.put_u32(0);
    b.put_bytes(0, 32);
    b.put(&dk.to_bytes()[..]);
    let efvk = ExtendedFullViewingKey::read(&*b)?;
    Ok(efvk)
}

pub fn sk_to_address(sk: &SecretKey) -> TransparentAddress {
    let secp256k1 = Secp256k1::<All>::new();
    let pubkey = sk.public_key(&secp256k1);
    TransparentAddress::PublicKeyHash(
        *ripemd::Ripemd160::digest(Sha256::digest(pubkey.serialize())).as_ref(),
    )
}

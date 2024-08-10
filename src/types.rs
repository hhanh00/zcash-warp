use orchard::{keys::{FullViewingKey, SpendingKey}, Address};
use secp256k1::SecretKey;
use zcash_client_backend::{address::UnifiedAddress, encoding::{encode_extended_full_viewing_key, encode_extended_spending_key, AddressCodec as _}};
use zcash_client_backend::keys::UnifiedFullViewingKey;
use zcash_primitives::{consensus::{Network, Parameters as _}, legacy::TransparentAddress, sapling::PaymentAddress, zip32::{DiversifiableFullViewingKey, ExtendedFullViewingKey, ExtendedSpendingKey}};

use crate::{data::fb::BackupT, keys::export_sk_bip38};

#[derive(Debug)]
pub struct PoolMask(pub u8);

#[derive(Debug)]
pub enum AccountType {
    Seed,
    SaplingSK,
    SaplingVK,
    UnifiedVK,
}

#[derive(Debug)]
pub struct AccountName {
    pub account: u32,
    pub name: String,
}

#[derive(Debug)]
pub struct TransparentAccountInfo {
    pub sk: secp256k1::SecretKey,
    pub addr: TransparentAddress,
}

#[derive(Debug)]
pub struct SaplingAccountInfo {
    pub sk: Option<ExtendedSpendingKey>,
    pub vk: ExtendedFullViewingKey,
    pub addr: PaymentAddress,
}

#[derive(Debug)]
pub struct OrchardAccountInfo {
    pub sk: Option<SpendingKey>,
    pub vk: FullViewingKey,
    pub addr: Address,
}

#[derive(Debug)]
pub struct AccountInfo {
    pub name: String,
    pub seed: Option<String>,
    pub aindex: u32,
    pub saved: bool,
    pub transparent: Option<TransparentAccountInfo>,
    pub sapling: SaplingAccountInfo,
    pub orchard: Option<OrchardAccountInfo>,
}

impl AccountInfo {
    pub fn to_account_unique_id(&self) -> [u8; 32] {
        self.sapling.vk.fvk.vk.ivk().to_repr()
    }

    pub fn account_type(&self) -> AccountType {
        if self.seed.is_some() { return AccountType::Seed }
        if self.sapling.sk.is_some() { return AccountType::SaplingSK }
        if self.orchard.is_some() { return AccountType::SaplingVK }
        AccountType::SaplingVK
    }

    pub fn to_backup(&self, network: &Network) -> BackupT {
        let sk = self.sapling.sk.as_ref().map(|sk| {
            encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), &sk)
        });
        let fvk = encode_extended_full_viewing_key(
            network.hrp_sapling_extended_full_viewing_key(),
            &self.sapling.vk,
        );
        let dfvk = DiversifiableFullViewingKey::from(&self.sapling.vk);
        let ofvk = self.orchard.as_ref().map(|o| o.vk.clone());

        let uvk = UnifiedFullViewingKey::new(None, Some(dfvk), ofvk).unwrap();
        let uvk = uvk.encode(network);

        let tsk = self.transparent.as_ref().map(|t| export_sk_bip38(&t.sk));

        BackupT {
            name: Some(self.name.clone()),
            seed: self.seed.clone(),
            index: self.aindex,
            sk,
            fvk: Some(fvk),
            uvk: Some(uvk),
            tsk,
            saved: self.saved,
        }
    }

    pub fn to_secret_keys(&self) -> SecretKeys {
        SecretKeys {
            transparent: self.transparent.as_ref().map(|ti| ti.sk),
            sapling: self.sapling.sk.clone(),
            orchard: self.orchard.as_ref().and_then(|oi| oi.sk),
        }
    }

    pub fn to_view_keys(&self) -> ViewKeys {
        ViewKeys {
            sapling: Some(self.sapling.vk.clone()),
            orchard: self.orchard.as_ref().map(|oi| oi.vk.clone()),
        }
    }

    pub fn to_address(&self, network: &Network, pool_mask: PoolMask) -> Option<String> {
        let pool_mask = pool_mask.0;
        let taddr = if pool_mask & 1 != 0 {
            self.transparent.as_ref().map(|ti| ti.addr)
        } else {
            None
        };
        let saddr = if pool_mask & 2 != 0 {
            Some(self.sapling.addr.clone())
        } else {
            None
        };
        let oaddr = if pool_mask & 4 != 0 {
            self.orchard.as_ref().map(|oi| oi.addr)
        } else {
            None
        };

        let t = if taddr.is_some() { 1 } else { 0 };
        let s = if saddr.is_some() { 1 } else { 0 };
        let o = if oaddr.is_some() { 1 } else { 0 };
        let tpe = t | (s << 1) | (o << 2);

        let addr = match tpe {
            0 => None,
            1 => taddr.map(|t| t.encode(network)),
            2 => saddr.map(|s| s.encode(network)),
            _ => {
                let ua = UnifiedAddress::from_receivers(oaddr, saddr, taddr);
                ua.map(|ua| ua.encode(network))
            }
        };

        addr
    }
}

#[derive(Debug)]
pub struct SecretKeys {
    pub transparent: Option<SecretKey>,
    pub sapling: Option<ExtendedSpendingKey>,
    pub orchard: Option<SpendingKey>,
}

#[derive(Debug)]
pub struct ViewKeys {
    pub sapling: Option<ExtendedFullViewingKey>,
    pub orchard: Option<FullViewingKey>,
}

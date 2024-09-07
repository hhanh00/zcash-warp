use anyhow::Result;
use bip39::Seed;
use orchard::{
    keys::{FullViewingKey, SpendingKey},
    Address,
};
use prost::bytes::BufMut as _;
use sapling_crypto::{
    zip32::{DiversifiableFullViewingKey, ExtendedFullViewingKey, ExtendedSpendingKey},
    PaymentAddress,
};
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};
use zcash_client_backend::keys::UnifiedFullViewingKey;
use zcash_client_backend::{
    address::UnifiedAddress,
    encoding::{encode_extended_full_viewing_key, encode_extended_spending_key, AddressCodec as _},
};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::{
    consensus::{Network, NetworkConstants as _},
    legacy::TransparentAddress,
};

use crate::{data::fb::{BackupT, ContactCardT}, db::account_manager::parse_seed_phrase, keys::export_sk_bip38};

#[derive(Clone, Copy, Default, Debug)]
pub struct PoolMask(pub u8);

impl PoolMask {
    pub fn from_pool(pool: u8) -> Self {
        Self(1 << pool)
    }

    pub fn to_pool(&self) -> Option<u8> {
        if self.0 & 4 != 0 {
            return Some(2);
        }
        if self.0 & 2 != 0 {
            return Some(1);
        }
        if self.0 & 1 != 0 {
            return Some(0);
        }
        None
    }

    pub fn single_pool(&self) -> bool {
        self.0 & (self.0 - 1) == 0
    }
}

impl From<Option<u8>> for PoolMask {
    fn from(value: Option<u8>) -> Self {
        let p = match value {
            Some(p) => 1 << p,
            None => 0,
        };
        PoolMask(p)
    }
}

#[derive(Debug)]
pub enum AccountType {
    Seed(Seed),
    SaplingSK(ExtendedSpendingKey),
    SaplingVK(ExtendedFullViewingKey),
    UnifiedVK(UnifiedFullViewingKey),
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
    pub account: u32,
    pub name: String,
    pub seed: Option<String>,
    pub aindex: u32,
    pub saved: bool,
    pub transparent: Option<TransparentAccountInfo>,
    pub sapling: SaplingAccountInfo,
    pub orchard: Option<OrchardAccountInfo>,
}

impl SaplingAccountInfo {
    pub fn from_sk(sk: &ExtendedSpendingKey) -> Self {
        #[allow(deprecated)]
        let vk = sk.to_extended_full_viewing_key();
        let (_, addr) = vk.default_address();
        Self {
            sk: Some(sk.clone()),
            vk,
            addr,
        }
    }

    pub fn from_vk(vk: &ExtendedFullViewingKey) -> Self {
        let (_, addr) = vk.default_address();
        Self {
            sk: None,
            vk: vk.clone(),
            addr,
        }
    }

    pub fn from_dvk(dvk: &DiversifiableFullViewingKey) -> Self {
        let (_, addr) = dvk.default_address();
        // There is no public api to build a ExtendedFullViewingKey from DiversifiableFullViewingKey
        // we use the binary serialization format as a workaround
        let mut evk = vec![];
        evk.put_u8(0); // depth
        evk.put_u32(0); // tag
        evk.put_u32(0); // index
        evk.put_bytes(0, 32); // chain code
        evk.put(&dvk.to_bytes()[..]);
        let evk = ExtendedFullViewingKey::read(&*evk).unwrap();

        Self {
            sk: None,
            vk: evk,
            addr,
        }
    }
}

impl OrchardAccountInfo {
    pub fn from_vk(vk: &FullViewingKey) -> Self {
        let addr = vk.address_at(0u64, orchard::keys::Scope::External);
        Self {
            sk: None,
            vk: vk.clone(),
            addr,
        }
    }
}

impl AccountInfo {
    pub fn to_account_unique_id(&self) -> [u8; 32] {
        self.sapling.vk.fvk.vk.ivk().to_repr()
    }

    pub fn account_type(&self) -> Result<AccountType> {
        if let Some(phrase) = &self.seed {
            let seed = parse_seed_phrase(&phrase)?;
            return Ok(AccountType::Seed(seed));
        }
        if let Some(ssk) = &self.sapling.sk {
            return Ok(AccountType::SaplingSK(ssk.clone()));
        }
        if let Some(ovk) = self.orchard.as_ref().map(|oi| &oi.vk) {
            let svk = self.sapling.vk.to_diversifiable_full_viewing_key();
            let uvk = UnifiedFullViewingKey::new(None, Some(svk), Some(ovk.clone()))?;
            return Ok(AccountType::UnifiedVK(uvk));
        }
        let svk = &self.sapling.vk;
        Ok(AccountType::SaplingVK(svk.clone()))
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

    pub fn to_addresses(&self, network: &Network) -> Addresses {
        Addresses {
            transparent: self.to_address(network, PoolMask(1)),
            sapling: self.to_address(network, PoolMask(2)),
            orchard: self.to_address(network, PoolMask(4)),
        }
    }

    pub fn select_pools(self, pools: PoolMask) -> OptionAccountInfo {
        let pools = pools.0;
        OptionAccountInfo {
            account: self.account,
            name: self.name,
            seed: self.seed,
            aindex: self.aindex,
            saved: self.saved,
            transparent: if pools & 1 != 0 {
                self.transparent
            } else {
                None
            },
            sapling: if pools & 2 != 0 {
                Some(self.sapling)
            } else {
                None
            },
            orchard: if pools & 4 != 0 { self.orchard } else { None },
        }
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

#[derive(Debug)]
pub struct Addresses {
    pub transparent: Option<String>,
    pub sapling: Option<String>,
    pub orchard: Option<String>,
}

#[derive(Debug)]
pub struct Balance {
    pub transparent: u64,
    pub sapling: u64,
    pub orchard: u64,
}

#[derive(Debug)]
pub struct OptionAccountInfo {
    pub account: u32,
    pub name: String,
    pub seed: Option<String>,
    pub aindex: u32,
    pub saved: bool,
    pub transparent: Option<TransparentAccountInfo>,
    pub sapling: Option<SaplingAccountInfo>,
    pub orchard: Option<OrchardAccountInfo>,
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub card: ContactCardT,
    pub address: RecipientAddress,
}

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
use zcash_client_backend::keys::UnifiedFullViewingKey;
use zcash_client_backend::{
    address::UnifiedAddress,
    encoding::{encode_extended_full_viewing_key, encode_extended_spending_key, AddressCodec as _},
};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::{
    consensus::NetworkConstants as _,
    legacy::TransparentAddress,
};

use crate::{
    network::Network,
    data::fb::{BackupT, ContactCardT},
    db::account_manager::parse_seed_phrase,
    keys::export_sk_bip38,
};

#[derive(Clone, Copy, Default, Debug)]
pub struct CheckpointHeight(pub u32);

impl From<CheckpointHeight> for u32 {
    fn from(value: CheckpointHeight) -> Self {
        value.0
    }
}

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
        if self.0 != 0 {
            (self.0 & (self.0 - 1)) == 0
        } else {
            false
        }
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
    pub index: Option<u32>,
    pub sk: Option<secp256k1::SecretKey>,
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
    pub fingerprint: Vec<u8>,
    pub seed: Option<String>,
    pub aindex: u32,
    pub birth: u32,
    pub saved: bool,
    pub transparent: Option<TransparentAccountInfo>,
    pub sapling: Option<SaplingAccountInfo>,
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
    pub fn account_type(&self) -> Result<AccountType> {
        if let Some(phrase) = &self.seed {
            let seed = parse_seed_phrase(&phrase)?;
            return Ok(AccountType::Seed(seed));
        }
        if let Some(ssk) = self.sapling.as_ref().and_then(|si| si.sk.as_ref()) {
            return Ok(AccountType::SaplingSK(ssk.clone()));
        }
        if let Some(ovk) = self.orchard.as_ref().map(|oi| &oi.vk) {
            let svk = self
                .sapling
                .as_ref()
                .map(|si| si.vk.to_diversifiable_full_viewing_key());
            let uvk = UnifiedFullViewingKey::new(None, svk, Some(ovk.clone()))?;
            return Ok(AccountType::UnifiedVK(uvk));
        }
        if let Some(svk) = self.sapling.as_ref().map(|si| &si.vk) {
            return Ok(AccountType::SaplingVK(svk.clone()));
        }
        anyhow::bail!("Unknown account type");
    }

    pub fn to_backup(&self, network: &Network) -> BackupT {
        let sk = self.sapling.as_ref().and_then(|si| {
            si.sk.as_ref().map(|sk| {
                encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), &sk)
            })
        });
        let fvk = self.sapling.as_ref().map(|si| {
            encode_extended_full_viewing_key(
                network.hrp_sapling_extended_full_viewing_key(),
                &si.vk,
            )
        });
        let dfvk = self
            .sapling
            .as_ref()
            .map(|si| DiversifiableFullViewingKey::from(&si.vk));
        let ofvk = self.orchard.as_ref().map(|o| o.vk.clone());

        let uvk = if dfvk.is_some() || ofvk.is_some() {
            Some(UnifiedFullViewingKey::new(None, dfvk, ofvk).unwrap())
        } else {
            None
        };
        let uvk = uvk.map(|uvk| uvk.encode(network));

        let tsk = self
            .transparent
            .as_ref()
            .and_then(|ti| ti.sk.map(|sk| export_sk_bip38(&sk)));

        BackupT {
            name: Some(self.name.clone()),
            seed: self.seed.clone(),
            index: self.aindex,
            birth: self.birth,
            sk,
            fvk,
            uvk,
            tsk,
            saved: self.saved,
        }
    }

    pub fn to_secret_keys(&self) -> SecretKeys {
        SecretKeys {
            transparent: self.transparent.as_ref().and_then(|ti| ti.sk.clone()),
            sapling: self.sapling.as_ref().and_then(|si| si.sk.clone()),
            orchard: self.orchard.as_ref().and_then(|oi| oi.sk),
        }
    }

    pub fn to_view_keys(&self) -> ViewKeys {
        ViewKeys {
            sapling: self.sapling.as_ref().map(|si| si.vk.clone()),
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
            self.sapling.as_ref().map(|si| si.addr.clone())
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
            sapling: if pools & 2 != 0 { self.sapling } else { None },
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

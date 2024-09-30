use anyhow::Result;
use orchard::{
    keys::{FullViewingKey, SpendingKey},
    Address,
};
use sapling_crypto::{
    zip32::{DiversifiableFullViewingKey, ExtendedSpendingKey},
    PaymentAddress,
};
use secp256k1::SecretKey;
use zcash_client_backend::keys::UnifiedFullViewingKey;
use zcash_client_backend::{
    address::UnifiedAddress,
    encoding::{encode_extended_full_viewing_key, encode_extended_spending_key, AddressCodec as _},
};
use zcash_keys::{address::Address as RecipientAddress};
use zcash_primitives::{
    consensus::NetworkConstants as _,
    legacy::{
        keys::{AccountPrivKey, AccountPubKey},
        TransparentAddress,
    },
};

use crate::{
    data::fb::{BackupT, ContactCardT},
    keys::{export_sk_bip38, to_extended_full_viewing_key, AccountKeys},
    network::Network,
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
pub struct TransparentAccountInfo {
    pub index: Option<u32>,
    pub xsk: Option<AccountPrivKey>,
    pub sk: Option<SecretKey>,
    pub vk: Option<AccountPubKey>,
    pub addr: TransparentAddress,
}

#[derive(Debug)]
pub struct SaplingAccountInfo {
    pub sk: Option<ExtendedSpendingKey>,
    pub vk: DiversifiableFullViewingKey,
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
    pub dindex: Option<u32>,
    pub birth: u32,
    pub saved: bool,
    pub transparent: Option<TransparentAccountInfo>,
    pub sapling: Option<SaplingAccountInfo>,
    pub orchard: Option<OrchardAccountInfo>,
}

impl SaplingAccountInfo {
    pub fn from_sk(sk: &ExtendedSpendingKey, di: u32) -> Self {
        let vk = sk.to_diversifiable_full_viewing_key();
        Self::from_vk(&vk, di)
    }

    pub fn from_vk(vk: &DiversifiableFullViewingKey, di: u32) -> Self {
        let (_, addr) = vk.find_address(di.into()).unwrap();
        Self {
            sk: None,
            vk: vk.clone(),
            addr,
        }
    }
}

impl OrchardAccountInfo {
    pub fn from_vk(vk: &FullViewingKey, di: u32) -> Self {
        let addr = vk.address_at(di as u64, orchard::keys::Scope::External);
        Self {
            sk: None,
            vk: vk.clone(),
            addr,
        }
    }
}

impl AccountInfo {
    pub fn pools(&self) -> PoolMask {
        let t = if self.transparent.is_some() { 1 } else { 0 };
        let s = if self.sapling.is_some() { 2 } else { 0 };
        let o = if self.orchard.is_some() { 4 } else { 0 };
        PoolMask(t | s | o)
    }

    pub fn keys(&self) -> Result<AccountKeys> {
        let mut ak = AccountKeys {
            seed: self.seed.clone(),
            aindex: self.aindex,
            dindex: self.dindex.clone(),
            txsk: None,
            tsk: None,
            tvk: None,
            taddr: None,
            ssk: None,
            svk: None,
            osk: None,
            ovk: None,
        };

        if let Some(ti) = self.transparent.as_ref() {
            ak.txsk = ti.xsk.clone();
            ak.tsk = ti.sk.clone();
            ak.tvk = ti.vk.clone();
            ak.taddr = Some(ti.addr.clone());
        }
        if let Some(si) = self.sapling.as_ref() {
            ak.ssk = si.sk.clone();
            ak.svk = Some(si.vk.clone());
        }
        if let Some(oi) = self.orchard.as_ref() {
            ak.osk = oi.sk.clone();
            ak.ovk = Some(oi.vk.clone());
        }
        Ok(ak)
    }

    pub fn to_backup(&self, network: &Network) -> BackupT {
        let sk = self.sapling.as_ref().and_then(|si| {
            si.sk.as_ref().map(|sk| {
                encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), &sk)
            })
        });
        let tfvk = self.transparent.as_ref().and_then(|ti| ti.vk.clone());
        let dfvk = self.sapling.as_ref().map(|si| si.vk.clone());
        let fvk = dfvk.as_ref().map(|dfvk| {
            let efvk = to_extended_full_viewing_key(&dfvk).unwrap();
            encode_extended_full_viewing_key(network.hrp_sapling_extended_full_viewing_key(), &efvk)
        });
        let ofvk = self.orchard.as_ref().map(|o| o.vk.clone());

        let uvk = if dfvk.is_some() || ofvk.is_some() {
            Some(UnifiedFullViewingKey::new(tfvk, dfvk, ofvk).unwrap())
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
            transparent: self.transparent.as_ref().and_then(|ti| ti.vk.clone()),
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
    pub transparent: Option<AccountPubKey>,
    pub sapling: Option<DiversifiableFullViewingKey>,
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

impl OptionAccountInfo {
    pub fn to_vk(&self) -> Result<UnifiedFullViewingKey> {
        let tvk = self.transparent.as_ref().and_then(|ti| ti.vk.clone());
        let svk = self.sapling.as_ref().map(|si| si.vk.clone());
        let ovk = self.orchard.as_ref().map(|oi| oi.vk.clone());
        let uvk = UnifiedFullViewingKey::new(tvk, svk, ovk)?;
        Ok(uvk)
    }

    pub fn to_mask(&self) -> u8 {
        let t = if self.transparent.is_some() { 1 } else { 0 };
        let s = if self.sapling.is_some() { 2 } else { 0 };
        let o = if self.orchard.is_some() { 4 } else { 0 };
        t | s | o
    }

    pub fn to_uvk(&self) -> Result<UnifiedFullViewingKey> {
        let tvk = self.transparent.as_ref().and_then(|ti| ti.vk.clone());
        let svk = self.sapling.as_ref().map(|si| si.vk.clone());
        let ovk = self.orchard.as_ref().map(|oi| oi.vk.clone());
        let uvk = UnifiedFullViewingKey::new(tvk, svk, ovk)?;
        Ok(uvk)
    }
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub card: ContactCardT,
    pub address: RecipientAddress,
}

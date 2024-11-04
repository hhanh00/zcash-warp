use std::u32;

use anyhow::Result;
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use rusqlite::{params, Connection, OptionalExtension as _};
use zcash_client_backend::encoding::{
    decode_extended_full_viewing_key, decode_extended_spending_key, decode_payment_address,
    AddressCodec as _,
};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::consensus::NetworkConstants as _;
use zcash_primitives::legacy::keys::{AccountPrivKey, AccountPubKey};
use zcash_primitives::legacy::TransparentAddress;

use crate::account::contacts::recipient_contains;
use crate::coin::CoinDef;
use crate::data::fb::{
    AccountNameListT, AccountNameT, AccountSigningCapabilitiesT, BalanceT, SpendableT, SpendingT,
    TransparentAddressT,
};
use crate::db::contacts::list_contacts;
use crate::keys::{export_sk_bip38, import_sk_bip38};
use crate::network::Network;
use crate::types::{AccountInfo, OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo};
use crate::utils::ContextExt;
use crate::warp::TransparentSK;
use warp_macros::c_export;

#[c_export]
pub fn list_accounts(coin: &CoinDef, connection: &Connection) -> Result<AccountNameListT> {
    let mut s = connection.prepare(
        "SELECT id_account, name, birth, balance, hidden FROM accounts ORDER BY position",
    )?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, u64>(3)?,
            r.get::<_, bool>(4)?,
        ))
    })?;
    let mut accounts = vec![];
    for r in rows {
        let (id, name, birth, balance, hidden) = r?;
        accounts.push(AccountNameT {
            coin: coin.coin,
            id,
            name: Some(name),
            birth,
            balance,
            hidden,
        });
    }
    let accounts = AccountNameListT {
        items: Some(accounts),
    };
    Ok(accounts)
}

#[c_export]
pub fn list_account_transparent_addresses(
    connection: &Connection,
    account: u32,
) -> Result<Vec<TransparentAddressT>> {
    let mut s = connection.prepare(
        "SELECT t.external, t.addr_index, t.address, SUM(u.value)
        FROM t_addresses t
        LEFT JOIN utxos u
        ON t.account = u.account AND t.external = u.external
            AND t.addr_index = u.addr_index
        WHERE t.account = ?1 AND u.spent IS NULL
		GROUP BY t.address
        ORDER BY t.addr_index",
    )?;
    let rows = s.query_map([account], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<u64>>(3)?,
        ))
    })?;
    let mut addresses = vec![];
    for r in rows {
        let (external, addr_index, address, value) = r?;
        addresses.push(TransparentAddressT {
            account,
            external,
            addr_index,
            address: Some(address),
            amount: value.unwrap_or_default(),
        });
    }
    Ok(addresses)
}

#[derive(Clone, Debug)]
pub struct TransparentDerPath {
    pub account: u32,
    pub external: u32,
    pub addr_index: u32,
}

pub fn list_transparent_addresses(
    connection: &Connection,
) -> Result<Vec<(TransparentDerPath, String)>> {
    let mut s = connection.prepare(
        "SELECT account, external, addr_index, address FROM t_addresses ORDER BY addr_index",
    )?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut res = vec![];
    for r in rows {
        let (account, external, addr_index, address) = r?;
        let path = TransparentDerPath {
            account,
            external,
            addr_index,
        };
        res.push((path, address));
    }
    Ok(res)
}

pub fn get_account_info(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<AccountInfo> {
    let cindex = connection.query_row(
        "SELECT MAX(addr_index) FROM t_addresses WHERE account = ?1
        AND external = 1",
        [account],
        |r| r.get::<_, Option<u32>>(0),
    )?;

    let ai = connection
        .query_row(
            "SELECT a.name, a.position, a.seed, a.aindex, a.dindex, a.birth,
        t.xsk as txsk, t.sk as tsk, t.vk as tvk, t.address as taddr,
        s.sk as ssk, s.vk as svk, s.address as saddr,
        o.sk as osk, o.vk as ovk,
        a.saved
        FROM accounts a
        LEFT JOIN t_accounts t ON t.account = a.id_account
        LEFT JOIN s_accounts s ON s.account = a.id_account
        LEFT JOIN o_accounts o ON o.account = a.id_account
        WHERE id_account = ?1",
            [account],
            |r| {
                let name = r.get::<_, String>("name")?;
                let position = r.get::<_, u32>("position")?;
                let seed = r.get::<_, Option<String>>("seed")?;
                let aindex = r.get::<_, u32>("aindex")?;
                let dindex = r.get::<_, u32>("dindex")?;
                let birth = r.get::<_, u32>("birth")?;
                let saved = r.get::<_, Option<bool>>("saved")?;

                let taddr = r.get::<_, Option<String>>("taddr")?;
                let ti = match taddr {
                    None => None,
                    Some(taddr) => {
                        let txsk = r.get::<_, Option<Vec<u8>>>("txsk")?;
                        let xsk = txsk.map(|txsk| AccountPrivKey::from_bytes(&*txsk).unwrap());
                        let tsk = r.get::<_, Option<String>>("tsk")?;
                        let sk = tsk.map(|tsk| import_sk_bip38(&tsk).unwrap());
                        let tvk = r.get::<_, Option<Vec<u8>>>("tvk")?;
                        let vk = tvk.map(|tvk| {
                            AccountPubKey::deserialize(&tvk.try_into().unwrap()).unwrap()
                        });
                        let addr = TransparentAddress::decode(network, &taddr).unwrap();
                        let ti = TransparentAccountInfo {
                            index: dindex,
                            change_index: cindex,
                            xsk,
                            sk,
                            vk,
                            addr,
                        };
                        Some(ti)
                    }
                };

                let saddr = r.get::<_, Option<String>>("saddr")?;
                let si = match saddr {
                    None => None,
                    Some(saddr) => {
                        let sk = r.get::<_, Option<String>>("ssk")?.map(|sk| {
                            decode_extended_spending_key(
                                network.hrp_sapling_extended_spending_key(),
                                &sk,
                            )
                            .unwrap()
                        });
                        let vk = r.get::<_, String>("svk")?;
                        let vk = decode_extended_full_viewing_key(
                            network.hrp_sapling_extended_full_viewing_key(),
                            &vk,
                        )
                        .unwrap();
                        let vk = vk.to_diversifiable_full_viewing_key();
                        let addr =
                            decode_payment_address(network.hrp_sapling_payment_address(), &saddr)
                                .unwrap();
                        let si = SaplingAccountInfo { sk, vk, addr };
                        Some(si)
                    }
                };

                let ovk = r.get::<_, Option<Vec<u8>>>("ovk")?;
                let oi = match ovk {
                    None => None,
                    Some(vk) => {
                        let sk = r.get::<_, Option<Vec<u8>>>("osk")?.map(|sk| {
                            let sk = SpendingKey::from_bytes(sk.try_into().unwrap()).unwrap();
                            sk
                        });
                        let vk = FullViewingKey::from_bytes(&vk.try_into().unwrap()).unwrap();
                        let addr = vk.address_at(dindex, Scope::External);
                        let oi = OrchardAccountInfo { sk, vk, addr };
                        Some(oi)
                    }
                };

                let ai = AccountInfo {
                    account,
                    position,
                    name,
                    seed,
                    aindex,
                    dindex,
                    birth,
                    transparent: ti,
                    sapling: si,
                    orchard: oi,
                    saved: saved.unwrap_or_default(),
                };
                Ok(ai)
            },
        )
        .with_file_line(|| format!("No account {account}"))?;
    Ok(ai)
}

#[c_export]
pub fn change_account_dindex(
    network: &Network,
    connection: &Connection,
    account: u32,
    dindex: u32,
) -> Result<()> {
    let ai = get_account_info(network, connection, account)?;
    let ai = ai.clone_with_addr_index(network, dindex)?;
    update_account_addresses(network, connection, &ai)?;
    Ok(())
}

pub fn update_account_addresses(
    network: &Network,
    connection: &Connection,
    ai: &AccountInfo,
) -> Result<()> {
    if let Some(ti) = ai.transparent.as_ref() {
        let sk = ti.sk.as_ref().map(|sk| export_sk_bip38(sk));
        let address = ti.addr.encode(network);
        connection.execute(
            "UPDATE t_accounts SET sk = ?2, address = ?3
            WHERE account = ?1",
            params![ai.account, sk, address],
        )?;
    }
    if let Some(si) = ai.sapling.as_ref() {
        let address = si.addr.encode(network);
        connection.execute(
            "UPDATE s_accounts SET address = ?2 WHERE account = ?1",
            params![ai.account, address],
        )?;
    }
    connection.execute(
        "UPDATE accounts SET dindex = ?2 WHERE id_account = ?1",
        params![ai.account, ai.dindex],
    )?;
    Ok(())
}

pub fn list_account_tsk(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<Vec<TransparentSK>> {
    let mut s = connection.prepare("SELECT address, sk FROM t_addresses WHERE account = ?1")?;
    let rows = s.query_map([account], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut tsks = vec![];
    for r in rows {
        let (address, sk) = r?;
        let sk = import_sk_bip38(&sk)?;
        let ti = TransparentAccountInfo::from_secret_key(&sk, true);
        assert_eq!(ti.addr.encode(network), address);
        tsks.push(TransparentSK { address, sk });
    }
    Ok(tsks)
}

#[c_export]
pub fn get_balance(connection: &Connection, account: u32, height: u32) -> Result<BalanceT> {
    // includes spent but not confirmed
    // for display on the balance page
    let height = if height == 0 { u32::MAX } else { height };
    let transparent = connection
        .query_row(
            "SELECT SUM(value) FROM utxos
        WHERE account = ?1 AND height <= ?2 AND spent IS NULL",
            params![account, height],
            |r| r.get::<_, Option<u64>>(0),
        )?
        .unwrap_or_default();
    let sapling = connection
        .query_row(
            "SELECT SUM(value) FROM notes
        WHERE account = ?1 AND height <= ?2 AND orchard = 0
        AND spent IS NULL",
            params![account, height],
            |r| r.get::<_, Option<u64>>(0),
        )?
        .unwrap_or_default();
    let orchard = connection
        .query_row(
            "SELECT SUM(value) FROM notes
        WHERE account = ?1 AND height <= ?2 AND orchard = 1
        AND spent IS NULL",
            params![account, height],
            |r| r.get::<_, Option<u64>>(0),
        )?
        .unwrap_or_default();
    let b = BalanceT {
        transparent,
        sapling,
        orchard,
    };
    Ok(b)
}

#[c_export]
pub fn get_account_signing_capabilities(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<AccountSigningCapabilitiesT> {
    let ai = get_account_info(network, connection, account)?;
    let seed = ai.seed.is_some();
    // bit 0: has vk/addr
    // bit 1: has sk
    // bit 2: has diversifier/extended key
    let transparent: u8 = ai
        .transparent
        .as_ref()
        .map(|ti| {
            let mut tcaps = 1;
            if ti.sk.is_some() {
                tcaps |= 2;
            }
            if ti.vk.is_some() {
                tcaps |= 4;
            } // can derive
            tcaps
        })
        .unwrap_or_default();
    let sapling: u8 = ai
        .sapling
        .as_ref()
        // if there is a key, it is at least diversifiable + viewable
        .map(|si| if si.sk.is_some() { 7 } else { 5 })
        .unwrap_or_default();
    let orchard: u8 = ai
        .orchard
        .as_ref()
        .map(|oi| if oi.sk.is_some() { 7 } else { 5 })
        .unwrap_or_default();
    let account_caps = AccountSigningCapabilitiesT {
        seed,
        transparent,
        sapling,
        orchard,
    };
    Ok(account_caps)
}

#[c_export]
pub fn get_account_property(connection: &Connection, account: u32, name: &str) -> Result<Vec<u8>> {
    let value = connection
        .query_row(
            "SELECT value FROM props WHERE account = ?1 AND name = ?2",
            params![account, name],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    Ok(value.unwrap_or_default())
}

#[c_export]
pub fn set_account_property(
    connection: &Connection,
    account: u32,
    name: &str,
    value: &[u8],
) -> Result<()> {
    connection.execute(
        "INSERT INTO props(account, name, value)
        VALUES (?1, ?2, ?3) ON CONFLICT DO UPDATE
        SET value = excluded.value",
        params![account, name, value],
    )?;
    Ok(())
}

#[c_export]
pub fn get_spendings(
    network: &Network,
    connection: &Connection,
    account: u32,
    timestamp: u32,
) -> Result<Vec<SpendingT>> {
    let contacts = list_contacts(network, connection)?;
    let mut s = connection.prepare(
        "SELECT -SUM(value) as v, t.address FROM txs t
        WHERE account = ?1 AND timestamp >= ?2 AND value < 0
        AND t.address IS NOT NULL GROUP BY t.address ORDER BY v ASC LIMIT 5",
    )?;
    let rows = s.query_map(params![account, timestamp], |r| {
        Ok((r.get::<_, u64>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    let mut spendings = vec![];
    for r in rows {
        let (value, mut address) = r?;
        if let Some(a) = &address {
            let ra = RecipientAddress::decode(network, a).unwrap();
            for c in contacts.iter() {
                if recipient_contains(&c.address, &ra)? {
                    address = c.card.name.clone();
                }
            }
        }
        spendings.push(SpendingT {
            recipient: address,
            amount: value,
        });
    }
    Ok(spendings)
}

pub fn get_unconfirmed_spent(connection: &Connection, account: u32) -> Result<u64> {
    let spent = connection.query_row(
        "WITH n(value, account, spent, expiration) AS (
	SELECT value, account, spent, expiration FROM notes UNION ALL
	SELECT value, account, spent, expiration FROM utxos )
    SELECT SUM(value) FROM n WHERE account = ?1 AND expiration IS NOT NULL AND spent IS NULL",
        [account],
        |r| r.get::<_, Option<u64>>(0),
    )?;
    Ok(spent.unwrap_or_default())
}

pub fn get_unspent_before(connection: &Connection, account: u32, height: u32) -> Result<u64> {
    let spent = connection.query_row(
        "WITH n(value, account, height, spent, expiration) AS (
	SELECT value, account, height, spent, expiration FROM notes UNION ALL
	SELECT value, account, height, spent, expiration FROM utxos )
    SELECT SUM(value) FROM n WHERE account = ?1 AND height <= ?2
    AND expiration IS NULL AND spent IS NULL",
        [account, height],
        |r| r.get::<_, Option<u64>>(0),
    )?;
    Ok(spent.unwrap_or_default())
}

#[c_export]
pub fn get_spendable(connection: &Connection, account: u32, height: u32) -> Result<SpendableT> {
    let unconfirmed = get_unconfirmed_spent(connection, account)?;
    let total = get_unspent_before(connection, account, u32::MAX)?;
    let spendable = get_unspent_before(connection, account, height)?;
    let immature = total - spendable;
    let sp = SpendableT {
        total,
        unconfirmed,
        immature,
    };
    Ok(sp)
}

impl BalanceT {
    pub fn total(&self) -> u64 {
        self.transparent + self.sapling + self.orchard
    }
}

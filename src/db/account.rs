use anyhow::Result;
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use rusqlite::{params, Connection};
use zcash_client_backend::encoding::{
    decode_extended_full_viewing_key, decode_extended_spending_key, decode_payment_address,
    AddressCodec as _,
};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::consensus::NetworkConstants as _;
use zcash_primitives::legacy::TransparentAddress;

use crate::network::Network;
use crate::account::contacts::recipient_contains;
use crate::coin::COINS;
use crate::data::fb::{AccountNameT, AccountSigningCapabilitiesT, BalanceT, SpendingT};
use crate::db::contacts::list_contacts;
use crate::ffi::{map_result, map_result_bytes, CParam, CResult};
use crate::keys::import_sk_bip38;
use crate::types::{AccountInfo, OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo};
use crate::warp::TransparentSK;
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

#[c_export]
pub fn list_accounts(coin: u8, connection: &Connection) -> Result<Vec<AccountNameT>> {
    let mut s = connection.prepare(
        "SELECT id_account, key_type, name, birth, balance FROM accounts ORDER BY id_account",
    )?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u8>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, u32>(3)?,
            r.get::<_, u64>(4)?,
        ))
    })?;
    let mut accounts = vec![];
    for r in rows {
        let (id, key_type, name, birth, balance) = r?;
        accounts.push(AccountNameT {
            coin,
            id,
            key_type,
            name: Some(name),
            birth,
            balance,
        });
    }

    Ok(accounts)
}

pub fn list_transparent_addresses(connection: &Connection) -> Result<Vec<(u32, u32, String)>> {
    let mut s = connection.prepare("SELECT account, addr_index, address FROM t_subaccounts")?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut res = vec![];
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

pub fn get_account_info(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<AccountInfo> {
    let ai = connection.query_row(
        "SELECT a.name, a.fingerprint, a.seed, a.aindex, a.birth,
        t.addr_index as tidx, t.sk as tsk, t.address as taddr,
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
            let fingerprint = r.get::<_, Vec<u8>>("fingerprint")?;
            let seed = r.get::<_, Option<String>>("seed")?;
            let aindex = r.get::<_, u32>("aindex")?;
            let birth = r.get::<_, u32>("birth")?;
            let saved = r.get::<_, Option<bool>>("saved")?;

            let taddr = r.get::<_, Option<String>>("taddr")?;
            let ti = match taddr {
                None => None,
                Some(taddr) => {
                    let index = r.get::<_, Option<u32>>("tidx")?;
                    let tsk = r.get::<_, Option<String>>("tsk")?;
                    let sk = tsk.map(|tsk| import_sk_bip38(&tsk).unwrap());
                    let addr = TransparentAddress::decode(network, &taddr).unwrap();
                    let ti = TransparentAccountInfo { index, sk, addr };
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
                    let addr = vk.address_at(0u64, Scope::External);
                    let oi = OrchardAccountInfo { sk, vk, addr };
                    Some(oi)
                }
            };

            let ai = AccountInfo {
                account,
                name,
                fingerprint,
                seed,
                aindex,
                birth,
                transparent: ti,
                sapling: si,
                orchard: oi,
                saved: saved.unwrap_or_default(),
            };
            Ok(ai)
        },
    )?;
    Ok(ai)
}

pub fn list_account_tsk(connection: &Connection, account: u32) -> Result<Vec<TransparentSK>> {
    let mut s = connection.prepare("SELECT address, sk FROM t_subaccounts WHERE account = ?1")?;
    let rows = s.query_map([account], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut tsks = vec![];
    for r in rows {
        let (address, sk) = r?;
        let sk = import_sk_bip38(&sk)?;
        tsks.push(TransparentSK { address, sk });
    }
    Ok(tsks)
}

#[c_export]
pub fn get_balance(connection: &Connection, account: u32, height: u32) -> Result<BalanceT> {
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
    let transparent: u8 = ai
        .transparent
        .as_ref()
        .map(|ti| if ti.sk.is_some() { 3 } else { 1 })
        .unwrap_or_default();
    let sapling: u8 = ai
        .sapling
        .as_ref()
        .map(|si| if si.sk.is_some() { 3 } else { 1 })
        .unwrap_or_default();
    let orchard: u8 = ai
        .orchard
        .as_ref()
        .map(|oi| if oi.sk.is_some() { 3 } else { 1 })
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
    let value = connection.query_row(
        "SELECT value FROM props WHERE account = ?1 AND name = ?2",
        params![account, name],
        |r| r.get::<_, Vec<u8>>(0),
    )?;
    Ok(value)
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

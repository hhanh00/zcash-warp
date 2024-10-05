use anyhow::Result;
use bip39::{Mnemonic, Seed};
use rusqlite::{params, Connection, OptionalExtension};
use zcash_client_backend::{
    encoding::{
        decode_extended_full_viewing_key, decode_extended_spending_key,
        encode_extended_full_viewing_key, encode_extended_spending_key, encode_payment_address,
        AddressCodec,
    },
    keys::UnifiedFullViewingKey,
};
use zcash_primitives::{
    consensus::NetworkConstants as _,
    legacy::{keys::NonHardenedChildIndex, TransparentAddress},
};

use crate::{
    data::fb::{AccountSigningCapabilities, AccountSigningCapabilitiesT},
    keys::{export_sk_bip38, import_sk_bip38, to_extended_full_viewing_key, AccountKeys},
    network::Network,
    types::{OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo},
    utils::keys::find_address_index,
};

use crate::{
    coin::COINS,
    ffi::{map_result, CParam, CResult},
};
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

use super::account::get_account_info;

pub fn parse_seed_phrase(phrase: &str) -> Result<Seed> {
    let words = phrase.split_whitespace().collect::<Vec<_>>();
    let len = words.len();
    let (phrase, password) = if len % 3 == 1 {
        // extra word
        let phrase = words[0..len - 1].join(" ");
        let password = words[len - 1].to_string();
        (phrase, Some(password))
    } else {
        (phrase.to_string(), None)
    };

    let mnemonic = Mnemonic::from_phrase(&phrase, bip39::Language::English)?;
    let seed = Seed::new(&mnemonic, &password.unwrap_or_default());
    Ok(seed)
}

pub fn detect_key(network: &Network, key: &str, acc_index: u32) -> Result<AccountKeys> {
    let ak = if let Ok(_) = parse_seed_phrase(key) {
        AccountKeys::from_seed(network, key, acc_index)?
    } else if let Ok(ssk) =
        decode_extended_spending_key(network.hrp_sapling_extended_spending_key(), key)
    {
        let svk = ssk.to_diversifiable_full_viewing_key();
        let di = find_address_index(&svk, 0);
        AccountKeys {
            seed: None,
            aindex: 0,
            dindex: Some(di),
            txsk: None,
            tsk: None,
            tvk: None,
            taddr: None,
            ssk: Some(ssk.clone()),
            svk: Some(svk.clone()),
            osk: None,
            ovk: None,
        }
    } else if let Ok(svk) =
        decode_extended_full_viewing_key(network.hrp_sapling_extended_full_viewing_key(), key)
    {
        let svk = svk.to_diversifiable_full_viewing_key();
        let di = find_address_index(&svk, 0);
        AccountKeys {
            seed: None,
            aindex: 0,
            dindex: Some(di),
            txsk: None,
            tsk: None,
            tvk: None,
            taddr: None,
            ssk: None,
            svk: Some(svk.clone()),
            osk: None,
            ovk: None,
        }
    } else if let Ok(uvk) = UnifiedFullViewingKey::decode(network, key) {
        let tvk = uvk.transparent();
        let svk = uvk.sapling();
        let ovk = uvk.orchard();
        let sdi = svk.map(|svk| find_address_index(&svk, 0));
        let di = sdi.or(Some(0));
        let taddr = tvk.map(|tvk| TransparentAccountInfo::derive_address(tvk, di.unwrap()));
        AccountKeys {
            seed: None,
            aindex: 0,
            dindex: di,
            txsk: None,
            tsk: None,
            tvk: tvk.cloned(),
            taddr,
            ssk: None,
            svk: svk.cloned(),
            osk: None,
            ovk: ovk.cloned(),
        }
    } else if let Ok(tsk) = import_sk_bip38(key) {
        let ti = TransparentAccountInfo::from_secret_key(&tsk, true);
        // cannot derive more transparent addresses
        AccountKeys {
            seed: None,
            aindex: 0,
            dindex: None,
            txsk: None,
            tsk: ti.sk.clone(),
            tvk: None,
            taddr: Some(ti.addr),
            ssk: None,
            svk: None,
            osk: None,
            ovk: None,
        }
    } else if let Ok(taddr) = TransparentAddress::decode(network, key) {
        AccountKeys {
            seed: None,
            aindex: 0,
            dindex: None,
            txsk: None,
            tsk: None,
            tvk: None,
            taddr: Some(taddr),
            ssk: None,
            svk: None,
            osk: None,
            ovk: None,
        }
    } else {
        anyhow::bail!("Not a valid key");
    };
    Ok(ak)
}

#[c_export]
pub fn is_valid_key(network: &Network, key: &str) -> Result<bool> {
    let valid = detect_key(network, key, 0).is_ok();
    Ok(valid)
}

#[c_export]
pub fn create_new_account(
    network: &Network,
    connection: &Connection,
    name: &str,
    key: &str,
    acc_index: u32,
    birth: u32,
) -> Result<u32> {
    let ak = detect_key(network, &key, acc_index)?;
    let dindex = ak.dindex.unwrap_or_default();
    let account = create_account(
        connection,
        name,
        ak.seed.as_deref(),
        acc_index,
        dindex,
        birth,
    )?;
    if let Some(ti) = ak.to_transparent() {
        create_transparent_account(network, connection, account, &ti)?;
        create_transparent_address(network, connection, account, dindex, &ti)?;
        if dindex != 0 {
            create_transparent_address(network, connection, account, 0, &ti)?;
        }
    }
    if let Some(si) = ak.to_sapling() {
        create_sapling_account(network, connection, account, &si)?;
    }
    if let Some(oi) = ak.to_orchard() {
        create_orchard_account(network, connection, account, &oi)?;
    }
    Ok(account)
}

pub fn create_account(
    connection: &Connection,
    name: &str,
    seed: Option<&str>,
    acc_index: u32,
    addr_index: u32,
    birth: u32,
) -> Result<u32> {
    connection.execute(
        "INSERT INTO accounts(name, seed, aindex, dindex, birth, balance, saved)
        VALUES (?1, ?2, ?3, ?4, ?5, 0, FALSE)",
        params![name, seed, acc_index, addr_index, birth],
    )?;
    let account = connection.last_insert_rowid();
    Ok(account as u32)
}

pub fn create_sapling_account(
    network: &Network,
    connection: &Connection,
    account: u32,
    si: &SaplingAccountInfo,
) -> Result<()> {
    let sk = si
        .sk
        .as_ref()
        .map(|sk| encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), sk));
    let efvk = to_extended_full_viewing_key(&si.vk)?;
    let vk =
        encode_extended_full_viewing_key(network.hrp_sapling_extended_full_viewing_key(), &efvk);
    let addr = encode_payment_address(network.hrp_sapling_payment_address(), &si.addr);

    connection.execute(
        "INSERT INTO s_accounts(account, sk, vk, address)
        VALUES (?1, ?2, ?3, ?4)",
        params![account, sk, vk, addr],
    )?;
    Ok(())
}

pub fn create_transparent_account(
    network: &Network,
    connection: &Connection,
    account: u32,
    ti: &TransparentAccountInfo,
) -> Result<()> {
    let xsk = ti.xsk.as_ref().map(|xsk| xsk.to_bytes());
    let sk = ti.sk.as_ref().map(|sk| export_sk_bip38(&sk));
    let vk = ti.vk.as_ref().map(|vk| vk.serialize());
    let addr_index = ti.index.unwrap_or_default();
    let addr = ti.addr.encode(network);

    connection.execute(
        "INSERT INTO t_accounts(account, addr_index, xsk, sk, vk, address)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![account, addr_index, xsk, sk, vk, addr],
    )?;
    Ok(())
}

pub fn create_transparent_address(
    network: &Network,
    connection: &Connection,
    account: u32,
    addr_index: u32,
    ti: &TransparentAccountInfo,
) -> Result<()> {
    let address_index = NonHardenedChildIndex::from_index(addr_index).unwrap();
    let sk_from_xsk = ti.xsk.as_ref().map(|sk| {
        let sk = sk.derive_external_secret_key(address_index).unwrap();
        export_sk_bip38(&sk)
    });
    let sk_from_sk = ti.sk.as_ref().map(|sk| export_sk_bip38(sk));
    let sk = sk_from_xsk.or(sk_from_sk);
    let addr_from_vk = ti
        .vk
        .as_ref()
        .map(|tvk| TransparentAccountInfo::derive_address(tvk, addr_index).encode(network));
    let addr_from_addr = ti.addr.encode(network);
    let addr = addr_from_vk.or(Some(addr_from_addr));

    connection.execute(
        "INSERT INTO t_addresses(account, addr_index, sk, address)
        VALUES (?1, ?2, ?3, ?4)",
        params![account, addr_index, sk, addr],
    )?;
    Ok(())
}

pub fn create_orchard_account(
    _network: &Network,
    connection: &Connection,
    account: u32,
    oi: &OrchardAccountInfo,
) -> Result<()> {
    let sk = oi.sk.as_ref().map(|sk| sk.to_bytes());
    let fvk = &oi.vk.to_bytes();

    connection.execute(
        "INSERT INTO o_accounts(account, sk, vk)
        VALUES (?1, ?2, ?3)",
        params![account, sk, fvk],
    )?;
    Ok(())
}

pub fn get_account_by_name(connection: &Connection, name: &str) -> Result<Option<u32>> {
    let account = connection
        .query_row(
            "SELECT id_account FROM accounts WHERE name = ?1",
            [name],
            |r| r.get::<_, u32>(0),
        )
        .optional()?;
    Ok(account)
}

pub fn get_account_seed(connection: &Connection, account: u32) -> Result<(Seed, u32)> {
    let (phrase, aindex) = connection.query_row(
        "SELECT seed, aindex FROM accounts WHERE id_account = ?1",
        [account],
        |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, u32>(1)?)),
    )?;
    let phrase = phrase.ok_or(anyhow::anyhow!("No seed"))?;
    let seed = parse_seed_phrase(&phrase)?;
    Ok((seed, aindex))
}

#[c_export]
pub fn new_transparent_address(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<()> {
    let ai = get_account_info(network, connection, account)?;
    ai.transparent
        .as_ref()
        .map(|ti| {
            if ti.vk.is_none() {
                anyhow::bail!("Cannot derive additional addresses without an extended public key");
            }
            let addr_index = connection.query_row(
                "SELECT MAX(addr_index) FROM t_addresses WHERE account = ?1",
                [account],
                |r| r.get::<_, u32>(0),
            )? + 1;
            create_transparent_address(network, connection, account, addr_index, &ti)?;
            Ok::<_, anyhow::Error>(())
        })
        .transpose()?;
    Ok(())
}

pub fn trim_excess_transparent_addresses(connection: &Connection, account: u32) -> Result<()> {
    let max_addr_index = connection
        .query_row(
            "SELECT MAX(addr_index) FROM utxos WHERE account = ?1",
            [account],
            |r| r.get::<_, Option<u32>>(0),
        )?
        .unwrap_or_default();
    connection.execute(
        "DELETE FROM t_addresses WHERE account = ?1 AND addr_index > ?2",
        params![account, max_addr_index],
    )?;
    Ok(())
}

#[c_export]
pub fn edit_account_name(connection: &Connection, account: u32, name: &str) -> Result<()> {
    connection.execute(
        "UPDATE accounts SET name = ?2 where id_account = ?1",
        params![account, name],
    )?;
    Ok(())
}

#[c_export]
pub fn edit_account_birth(connection: &Connection, account: u32, birth: u32) -> Result<()> {
    connection.execute(
        "UPDATE accounts SET birth = ?2 where id_account = ?1",
        params![account, birth],
    )?;
    Ok(())
}

#[c_export]
pub fn delete_account(connection: &Connection, account: u32) -> Result<()> {
    connection.execute("DELETE FROM notes WHERE account = ?1", params![account])?;
    connection.execute("DELETE FROM utxos WHERE account = ?1", params![account])?;
    connection.execute("DELETE FROM witnesses WHERE account = ?1", params![account])?;
    connection.execute("DELETE FROM txs WHERE account = ?1", params![account])?;
    connection.execute("DELETE FROM txdetails WHERE account = ?1", params![account])?;
    connection.execute(
        "DELETE FROM accounts WHERE id_account = ?1",
        params![account],
    )?;
    connection.execute(
        "DELETE FROM t_accounts WHERE account = ?1",
        params![account],
    )?;
    connection.execute(
        "DELETE FROM t_addresses WHERE account = ?1",
        params![account],
    )?;
    connection.execute(
        "DELETE FROM s_accounts WHERE account = ?1",
        params![account],
    )?;
    connection.execute(
        "DELETE FROM o_accounts WHERE account = ?1",
        params![account],
    )?;
    connection.execute("DELETE FROM msgs WHERE account = ?1", params![account])?;
    connection.execute("DELETE FROM contacts WHERE account = ?1", params![account])?;
    connection.execute("DELETE FROM props WHERE account = ?1", params![account])?;
    Ok(())
}

#[c_export]
pub fn set_backup_reminder(connection: &Connection, account: u32, saved: bool) -> Result<()> {
    connection.execute(
        "UPDATE accounts SET saved = ?2 WHERE id_account = ?1",
        params![account, saved],
    )?;
    Ok(())
}

#[c_export]
pub fn downgrade_account(
    connection: &Connection,
    account: u32,
    capabilities: &AccountSigningCapabilitiesT,
) -> Result<()> {
    if capabilities.transparent == 0 && capabilities.sapling == 0 && capabilities.orchard == 0 {
        anyhow::bail!("Account needs at least one key");
    }

    if !capabilities.seed {
        connection.execute(
            "UPDATE accounts SET seed = NULL WHERE id_account = ?1",
            [account],
        )?;
    }
    if capabilities.transparent == 1 {
        connection.execute(
            "UPDATE t_accounts SET sk = NULL WHERE account = ?1",
            [account],
        )?;
        connection.execute(
            "UPDATE t_addresses SET sk = NULL WHERE account = ?1",
            [account],
        )?;
    } else if capabilities.transparent == 0 {
        connection.execute("DELETE FROM t_accounts WHERE account = ?1", [account])?;
        connection.execute("DELETE FROM t_addresses WHERE account = ?1", [account])?;
    }
    if capabilities.sapling == 1 {
        connection.execute(
            "UPDATE s_accounts SET sk = NULL WHERE account = ?1",
            [account],
        )?;
    } else if capabilities.sapling == 0 {
        connection.execute("DELETE FROM s_accounts WHERE account = ?1", [account])?;
    }
    if capabilities.orchard == 1 {
        connection.execute(
            "UPDATE o_accounts SET sk = NULL WHERE account = ?1",
            [account],
        )?;
    } else if capabilities.orchard == 0 {
        connection.execute("DELETE FROM o_accounts WHERE account = ?1", [account])?;
    }
    Ok(())
}

impl From<&AccountSigningCapabilitiesT> for u8 {
    fn from(value: &AccountSigningCapabilitiesT) -> Self {
        value.transparent
            | value.sapling << 2
            | value.orchard << 4
            | if value.seed { 1 << 6 } else { 0 }
    }
}

pub fn get_min_birth(connection: &Connection) -> Result<Option<u32>> {
    let birth = connection.query_row("SELECT MIN(birth) FROM accounts", [], |r| {
        r.get::<_, Option<u32>>(0)
    })?;
    Ok(birth)
}

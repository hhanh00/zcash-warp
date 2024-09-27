use anyhow::Result;
use bip39::{Mnemonic, Seed};
use orchard::keys::Scope;
use prost::bytes::BufMut;
use rusqlite::{params, Connection, OptionalExtension};
use sapling_crypto::zip32::{ExtendedFullViewingKey, ExtendedSpendingKey};
use secp256k1::{All, Secp256k1, SecretKey};
use zcash_client_backend::{
    encoding::{
        decode_extended_full_viewing_key, decode_extended_spending_key,
        encode_extended_full_viewing_key, encode_extended_spending_key, encode_payment_address,
        AddressCodec,
    },
    keys::UnifiedFullViewingKey,
};
use zcash_primitives::consensus::NetworkConstants as _;

use crate::{
    data::fb::{AccountSigningCapabilities, AccountSigningCapabilitiesT},
    keys::{derive_bip32, derive_orchard_zip32, derive_zip32, export_sk_bip38, import_sk_bip38},
    network::Network,
    types::{OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo},
};

use crate::{
    coin::COINS,
    ffi::{map_result, CParam, CResult},
};
use std::{
    ffi::{c_char, CStr},
    io::Write,
};
use warp_macros::c_export;

use super::account::get_account_signing_capabilities;

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

pub enum KeyType {
    Seed(String, Seed, u32),
    SaplingSK(ExtendedSpendingKey),
    SaplingVK(ExtendedFullViewingKey),
    UnifiedVK(UnifiedFullViewingKey),
    Transparent(SecretKey),
}

pub struct KeyFingerprint(pub Vec<u8>);

const T_SK: u8 = 1;
const T_VK: u8 = 2;
const S_SK: u8 = 4;
const S_VK: u8 = 8;
const O_SK: u8 = 16;
const O_VK: u8 = 32;
const SEED: u8 = 64;

impl From<&KeyType> for u8 {
    fn from(value: &KeyType) -> Self {
        match value {
            KeyType::Seed(_, _, _) => T_SK | T_VK | S_SK | S_VK | O_SK | O_VK | SEED,
            KeyType::SaplingSK(_) => S_SK | S_VK,
            KeyType::SaplingVK(_) => S_VK,
            KeyType::UnifiedVK(_) => S_VK | O_VK,
            KeyType::Transparent(_) => T_SK | T_VK,
        }
    }
}

impl KeyType {
    pub fn to_fingerprint(&self, network: &Network) -> Result<KeyFingerprint> {
        let mut fingerprint_buffer = vec![];
        match self {
            KeyType::Seed(_, seed, aindex) => {
                let si = derive_zip32(network, &seed, *aindex);
                fingerprint_buffer.put_u8(1);
                fingerprint_buffer.write_all(&si.addr.to_bytes()[..])?;
            }
            KeyType::SaplingSK(sk) => {
                let (_, address) = sk.default_address();
                fingerprint_buffer.put_u8(1);
                fingerprint_buffer.write_all(&address.to_bytes()[..])?;
            }
            KeyType::SaplingVK(vk) => {
                let (_, address) = vk.default_address();
                fingerprint_buffer.put_u8(1);
                fingerprint_buffer.write_all(&address.to_bytes()[..])?;
            }
            KeyType::UnifiedVK(vk) => {
                let Some(fvk) = vk.orchard() else {
                    anyhow::bail!("UVK must have an orchard receiver")
                };
                let address = fvk.address_at(0u64, Scope::External);
                fingerprint_buffer.put_u8(2);
                fingerprint_buffer.write_all(&address.to_raw_address_bytes()[..])?;
            }
            KeyType::Transparent(sk) => {
                let secp = Secp256k1::<All>::new();
                let pk = sk.public_key(&secp);
                fingerprint_buffer.put_u8(0);
                fingerprint_buffer.write_all(&pk.serialize()[..])?;
            }
        };
        Ok(KeyFingerprint(fingerprint_buffer))
    }
}

pub fn detect_key(network: &Network, key: &str, acc_index: u32) -> Result<KeyType> {
    if let Ok(seed) = parse_seed_phrase(key) {
        return Ok(KeyType::Seed(key.to_string(), seed, acc_index));
    }
    if let Ok(ssk) = decode_extended_spending_key(network.hrp_sapling_extended_spending_key(), key)
    {
        return Ok(KeyType::SaplingSK(ssk));
    }
    if let Ok(svk) =
        decode_extended_full_viewing_key(network.hrp_sapling_extended_full_viewing_key(), key)
    {
        return Ok(KeyType::SaplingVK(svk));
    }
    if let Ok(uvk) = UnifiedFullViewingKey::decode(network, key) {
        return Ok(KeyType::UnifiedVK(uvk));
    }
    if let Ok(tsk) = import_sk_bip38(key) {
        return Ok(KeyType::Transparent(tsk));
    }
    return Err(anyhow::anyhow!("Not a valid key"));
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
    let kt = detect_key(network, &key, acc_index)?;
    let fingerprint = kt.to_fingerprint(network)?;
    let account = match &kt {
        KeyType::Seed(seed_str, seed, acc_index) => {
            let si = derive_zip32(network, &seed, *acc_index);
            let account = create_account(
                connection,
                (&kt).into(),
                &fingerprint,
                name,
                Some(&seed_str),
                *acc_index,
                birth,
            )?;
            create_sapling_account(network, connection, account, &si)?;
            // This should have been acc_index / addr_index but ZecWallet Lite derives
            // with an incorrect path that we follow for compatibility reasons
            let ti = derive_bip32(network, &seed, *acc_index, 0, true);
            create_transparent_account(network, connection, account, 0, &ti)?;
            create_transparent_subaccount(network, connection, account, 0, &ti)?;
            let oi = derive_orchard_zip32(network, &seed, *acc_index);
            create_orchard_account(network, connection, account, &oi)?;
            account
        }
        KeyType::SaplingSK(sk) => {
            let account =
                create_account(connection, (&kt).into(), &fingerprint, name, None, 0, birth)?;
            let si = SaplingAccountInfo::from_sk(&sk);
            create_sapling_account(network, connection, account, &si)?;
            account
        }
        KeyType::SaplingVK(vk) => {
            let account =
                create_account(connection, (&kt).into(), &fingerprint, name, None, 0, birth)?;
            let si = SaplingAccountInfo::from_vk(&vk);
            create_sapling_account(network, connection, account, &si)?;
            account
        }
        KeyType::UnifiedVK(uvk) => {
            let account =
                create_account(connection, (&kt).into(), &fingerprint, name, None, 0, birth)?;
            let svk = uvk
                .sapling()
                .ok_or(anyhow::anyhow!("Missing sapling receiver"))?;
            let si = SaplingAccountInfo::from_dvk(&svk);
            create_sapling_account(network, connection, account, &si)?;
            uvk.orchard()
                .map(|ovk| {
                    let oi = OrchardAccountInfo::from_vk(ovk);
                    create_orchard_account(network, connection, account, &oi)
                })
                .transpose()?;
            account
        }
        KeyType::Transparent(tsk) => {
            let account =
                create_account(connection, (&kt).into(), &fingerprint, name, None, 0, birth)?;
            let ti = TransparentAccountInfo::from_secret_key(tsk, true);
            create_transparent_account(network, connection, account, 0, &ti)?;
            create_transparent_subaccount(network, connection, account, 0, &ti)?;
            account
        }
    };

    Ok(account)
}

pub fn create_account(
    connection: &Connection,
    key_type: u8,
    fingerprint: &KeyFingerprint,
    name: &str,
    seed: Option<&str>,
    acc_index: u32,
    birth: u32,
) -> Result<u32> {
    connection.execute(
        "INSERT INTO accounts(name, key_type, fingerprint, seed, aindex, birth, balance, saved)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, FALSE)",
        params![name, key_type, fingerprint.0, seed, acc_index, birth],
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
    let vk =
        encode_extended_full_viewing_key(network.hrp_sapling_extended_full_viewing_key(), &si.vk);
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
    addr_index: u32,
    ti: &TransparentAccountInfo,
) -> Result<()> {
    let sk = ti.sk.as_ref().map(|sk| export_sk_bip38(&sk));
    let addr = ti.addr.encode(network);

    connection.execute(
        "INSERT INTO t_accounts(account, addr_index, sk, address)
        VALUES (?1, ?2, ?3, ?4)",
        params![account, addr_index, sk, addr],
    )?;
    Ok(())
}

pub fn create_transparent_subaccount(
    network: &Network,
    connection: &Connection,
    account: u32,
    addr_index: u32,
    ti: &TransparentAccountInfo,
) -> Result<()> {
    let sk = ti.sk.as_ref().map(|sk| export_sk_bip38(sk));
    let addr = ti.addr.encode(network);

    connection.execute(
        "INSERT INTO t_subaccounts(account, addr_index, sk, address)
        VALUES (?1, ?2, ?3, ?4)",
        params![account, addr_index, sk, addr],
    )?;
    connection.execute(
        "UPDATE t_accounts SET addr_index = ?2, sk = ?3, address = ?4
        WHERE account = ?1",
        params![account, addr_index, &sk, &addr],
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
    let (seed, acc_index) = get_account_seed(connection, account)?;
    let addr_index = connection.query_row(
        "SELECT MAX(addr_index) FROM t_subaccounts WHERE account = ?1",
        [account],
        |r| r.get::<_, u32>(0),
    )? + 1;
    let ti = derive_bip32(network, &seed, acc_index, addr_index, true);
    create_transparent_subaccount(network, connection, account, addr_index, &ti)?;
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
        "DELETE FROM t_subaccounts WHERE account = ?1 AND addr_index > ?2",
        params![account, max_addr_index],
    )?;
    connection.execute(
        "INSERT INTO t_accounts 
        SELECT account, addr_index, sk, address FROM t_subaccounts
        WHERE account = ?1 AND addr_index = ?2
        ON CONFLICT (account) DO UPDATE SET addr_index = excluded.addr_index,
        sk = excluded.sk, address = excluded.address",
        params![account, max_addr_index])?; // update the account transparent address
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
        "DELETE FROM t_subaccounts WHERE account = ?1",
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

pub fn get_account_by_fingerprint(
    connection: &Connection,
    fingerprint: &[u8],
) -> Result<Option<u32>> {
    let account = connection
        .query_row(
            "SELECT id_account FROM accounts WHERE fingerprint = ?1",
            [fingerprint],
            |r| r.get::<_, u32>(0),
        )
        .optional()?;
    Ok(account)
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
    network: &Network,
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
            "UPDATE t_subaccounts SET sk = NULL WHERE account = ?1",
            [account],
        )?;
    } else if capabilities.transparent == 0 {
        connection.execute("DELETE FROM t_accounts WHERE account = ?1", [account])?;
        connection.execute("DELETE FROM t_subaccounts WHERE account = ?1", [account])?;
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
    let capabilities = &get_account_signing_capabilities(network, connection, account)?;
    let kt: u8 = capabilities.into();
    connection.execute(
        "UPDATE accounts SET key_type = ?2 WHERE id_account = ?1",
        params![account, kt],
    )?;
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

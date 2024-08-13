use anyhow::Result;
use bip39::{Mnemonic, Seed};
use rusqlite::{params, Connection};
use secp256k1::SecretKey;
use zcash_client_backend::{
    encoding::{decode_extended_full_viewing_key, decode_extended_spending_key, encode_extended_full_viewing_key, encode_extended_spending_key, encode_payment_address, AddressCodec},
    keys::UnifiedFullViewingKey,
};
use zcash_primitives::{
    consensus::{Network, Parameters},
    zip32::{ExtendedFullViewingKey, ExtendedSpendingKey},
};

use crate::{keys::{derive_bip32, derive_orchard_zip32, derive_zip32, export_sk_bip38, import_sk_bip38}, types::{OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo}};

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
    Seed(Seed, u32, u32),
    SaplingSK(ExtendedSpendingKey),
    SaplingVK(ExtendedFullViewingKey),
    UnifiedVK(UnifiedFullViewingKey),
    Transparent(SecretKey),
}

pub fn detect_key(
    network: &Network,
    key: &str,
    acc_index: u32,
    addr_index: u32,
) -> Result<KeyType> {
    if let Ok(seed) = parse_seed_phrase(key) {
        return Ok(KeyType::Seed(seed, acc_index, addr_index));
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

pub fn create_new_account(
    network: &Network,
    connection: &Connection,
    name: &str,
    key_str: &str,
    key: KeyType,
) -> Result<u32> {
    let account = match key {
        KeyType::Seed(seed, acc_index, _addr_index) => {
            let si = derive_zip32(network, &seed, acc_index);
            let account = create_sapling_account(network, connection, name, Some(key_str), acc_index, &si)?;
            // This should have been acc_index / addr_index but ZecWallet Lite derives
            // with an incorrect path that we follow for compatibility reasons
            let ti = derive_bip32(network, &seed, 0, acc_index);
            create_transparent_account(network, connection, account, &ti)?;
            let oi = derive_orchard_zip32(network, &seed, acc_index);
            create_orchard_account(network, connection, account, &oi)?;
            account
        }
        KeyType::SaplingSK(_) => todo!(),
        KeyType::SaplingVK(_) => todo!(),
        KeyType::UnifiedVK(_) => todo!(),
        KeyType::Transparent(_) => {
            anyhow::bail!("Transparent Private Keys are not supported. Use Sweep instead.")
        }
    };

    Ok(account)
}

pub fn create_sapling_account(
    network: &Network,
    connection: &Connection,
    name: &str,
    seed: Option<&str>,
    acc_index: u32,
    si: &SaplingAccountInfo,
) -> Result<u32> {
    let sk = si.sk.as_ref().map(|sk| encode_extended_spending_key(network.hrp_sapling_extended_spending_key(), sk));
    let vk = encode_extended_full_viewing_key(network.hrp_sapling_extended_full_viewing_key(), &si.vk);
    let addr = encode_payment_address(network.hrp_sapling_payment_address(), &si.addr);

    connection.execute("INSERT INTO accounts(name, seed, aindex, sk, ivk, address)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT DO NOTHING", params![name, seed, acc_index, sk, vk, addr])?;
    let account = connection.query_row("SELECT id_account FROM accounts WHERE ivk = ?1", [vk], |r| r.get::<_, u32>(0))?;
    Ok(account)
}

pub fn create_transparent_account(
    network: &Network,
    connection: &Connection,
    account: u32,
    ti: &TransparentAccountInfo,
) -> Result<()> {
    let sk = export_sk_bip38(&ti.sk);
    let addr = ti.addr.encode(network);

    connection.execute("INSERT INTO taddrs(account, sk, address, balance, height)
        VALUES (?1, ?2, ?3, 0, 0)", params![account, sk, addr])?;
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

    connection.execute("INSERT INTO orchard_addrs(account, sk, fvk)
        VALUES (?1, ?2, ?3)", params![account, sk, fvk])?;
    Ok(())
}

pub fn delete_account(connection: &Connection, account: u32) -> Result<()> {
    connection.execute(
        "DELETE FROM received_notes WHERE account = ?1",
        params![account],
    )?;
    connection.execute(
        "DELETE FROM transactions WHERE account = ?1",
        params![account],
    )?;
    connection.execute(
        "DELETE FROM diversifiers WHERE account = ?1",
        params![account],
    )?;
    connection.execute("DELETE FROM accounts2 WHERE account = ?1", params![account])?;
    connection.execute(
        "DELETE FROM accounts WHERE id_account = ?1",
        params![account],
    )?;
    connection.execute("DELETE FROM taddrs WHERE account = ?1", params![account])?;
    connection.execute(
        "DELETE FROM orchard_addrs WHERE account = ?1",
        params![account],
    )?;
    connection.execute("DELETE FROM messages WHERE account = ?1", params![account])?;
    connection.execute(
        "DELETE FROM hw_wallets WHERE account = ?1",
        params![account],
    )?;
    Ok(())
}

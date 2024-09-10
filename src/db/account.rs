use anyhow::Result;
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use rusqlite::{params, Connection};
use zcash_client_backend::encoding::{
    decode_extended_full_viewing_key, decode_extended_spending_key, decode_payment_address,
    AddressCodec as _,
};
use zcash_primitives::consensus::{Network, NetworkConstants as _};
use zcash_primitives::legacy::TransparentAddress;

use crate::data::fb::AccountNameT;
use crate::keys::import_sk_bip38;
use crate::types::{
    AccountInfo, Balance, OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo,
};

pub fn list_accounts(connection: &Connection) -> Result<Vec<AccountNameT>> {
    let mut s =
        connection.prepare("SELECT id_account, name, address, birth FROM accounts ORDER BY id_account")?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, u32>(3)?,
        ))
    })?;
    let mut accounts = vec![];
    for r in rows {
        let (id, name, address, birth) = r?;
        accounts.push(AccountNameT {
            id,
            name: Some(name),
            sapling_address: Some(address),
            birth,
        });
    }

    Ok(accounts)
}

pub fn get_account_info(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<AccountInfo> {
    let ai = connection.query_row(
        "SELECT a.name, a.seed, a.aindex, a.sk as ssk, a.vk as svk, a.address as saddr,
        a.birth,
        t.sk as tsk, t.address as taddr,
        o.sk as osk, o.vk as ovk,
        a.saved
        FROM accounts a
        LEFT JOIN t_accounts t ON t.account = a.id_account
        LEFT JOIN o_accounts o ON o.account = a.id_account
        WHERE id_account = ?1",
        [account],
        |r| {
            let taddr = r.get::<_, Option<String>>("taddr")?;
            let ti = match taddr {
                None => None,
                Some(taddr) => {
                    let tsk = r.get::<_, String>("tsk")?;
                    let sk = import_sk_bip38(&tsk).unwrap();
                    let addr = TransparentAddress::decode(network, &taddr).unwrap();
                    let ti = TransparentAccountInfo { sk, addr };
                    Some(ti)
                }
            };

            let sk = r.get::<_, Option<String>>("ssk")?.map(|sk| {
                decode_extended_spending_key(network.hrp_sapling_extended_spending_key(), &sk)
                    .unwrap()
            });
            let vk = r.get::<_, String>("svk")?;
            let vk = decode_extended_full_viewing_key(
                network.hrp_sapling_extended_full_viewing_key(),
                &vk,
            )
            .unwrap();
            let addr = r.get::<_, String>("saddr")?;
            let addr =
                decode_payment_address(network.hrp_sapling_payment_address(), &addr).unwrap();
            let name = r.get::<_, String>("name")?;
            let seed = r.get::<_, Option<String>>("seed")?;
            let aindex = r.get::<_, u32>("aindex")?;
            let birth = r.get::<_, u32>("birth")?;
            let saved = r.get::<_, Option<bool>>("saved")?;
            let si = SaplingAccountInfo { sk, vk, addr };

            let sk = r.get::<_, Option<Vec<u8>>>("osk")?.map(|sk| {
                let sk = SpendingKey::from_bytes(sk.try_into().unwrap()).unwrap();
                sk
            });
            let ovk = r.get::<_, Option<Vec<u8>>>("ovk")?;
            let oi = match ovk {
                None => None,
                Some(vk) => {
                    let vk = FullViewingKey::from_bytes(&vk.try_into().unwrap()).unwrap();
                    let addr = vk.address_at(0u64, Scope::External);
                    let oi = OrchardAccountInfo { sk, vk, addr };
                    Some(oi)
                }
            };

            let ai = AccountInfo {
                account,
                name,
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

pub fn get_balance(connection: &Connection, account: u32, height: u32) -> Result<Balance> {
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
    let b = Balance {
        transparent,
        sapling,
        orchard,
    };
    Ok(b)
}

pub fn get_account_property(connection: &Connection, account: u32, name: &str) -> Result<Vec<u8>> {
    let value = connection.query_row(
        "SELECT value FROM props WHERE account = ?1 AND name = ?2", 
        params![account, name], |r| r.get::<_, Vec<u8>>(0))?;
    Ok(value)
}

pub fn set_account_property(connection: &Connection, account: u32, name: &str, value: &[u8]) -> Result<()> {
    connection.execute(
        "INSERT INTO props(account, name, value)
        VALUES (?1, ?2, ?3) ON CONFLICT DO UPDATE
        SET value = excluded.value", params![account, name, value])?;
    Ok(())
}

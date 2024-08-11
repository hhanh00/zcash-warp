use anyhow::Result;
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use secp256k1::SecretKey;
use zcash_client_backend::encoding::{decode_extended_full_viewing_key, decode_extended_spending_key, decode_payment_address, AddressCodec as _};
use zcash_primitives::consensus::{Network, Parameters as _};
use zcash_primitives::legacy::TransparentAddress;

use crate::types::{AccountInfo, AccountName, OrchardAccountInfo, SaplingAccountInfo, TransparentAccountInfo};
use crate::Connection;

pub fn list_accounts(connection: &Connection) -> Result<Vec<AccountName>> {
    let mut s = connection.prepare("SELECT id_account, name FROM accounts ORDER BY id_account")?;
    let rows = s.query_map([], |r| {
        Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut accounts = vec![];
    for r in rows {
        let (account, name) = r?;
        accounts.push(AccountName { account, name });
    }

    Ok(accounts)
}

pub fn get_account_info(
    network: &Network,
    connection: &Connection,
    account: u32,
) -> Result<AccountInfo> {
    let ai = connection.query_row(
        "SELECT a.name, a.seed, a.aindex, a.sk as ssk, a.ivk as svk, a.address as saddr,
        t.sk as tsk, t.address as taddr, t.balance as tbal, t.height,
        o.sk as osk, o.fvk as ovk,
        a2.saved
        FROM accounts a
        LEFT JOIN taddrs t ON t.account = a.id_account
        LEFT JOIN orchard_addrs o ON o.account = a.id_account
        LEFT JOIN accounts2 a2 ON a2.account = a.id_account
        WHERE id_account = ?1",
        [account],
        |r| {
            let taddr = r.get::<_, Option<String>>("taddr")?;
            let ti = match taddr {
                None => None,
                Some(taddr) => {
                    let tsk = r.get::<_, String>("tsk")?;
                    let tsk = hex::decode(&tsk).unwrap();
                    let sk = SecretKey::from_slice(&tsk).unwrap();
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

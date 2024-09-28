use crate::{
    db::{
        account::get_account_info, account_manager::{create_transparent_subaccount, trim_excess_transparent_addresses},
        notes::store_utxo,
    },
    keys::derive_bip32,
    lwd::get_utxos,
    network::Network,
    Client,
};
use anyhow::Result;
use rusqlite::Connection;
use zcash_client_backend::encoding::AddressCodec as _;

use crate::types::AccountType;

use warp_macros::c_export;
use crate::{coin::COINS, ffi::{map_result, CResult}};

#[c_export]
pub async fn scan_transparent_addresses(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
    account: u32,
    gap_limit: u32,
) -> Result<()> {
    let ai = get_account_info(network, connection, account)?;
    let at = ai.account_type()?;
    let AccountType::Seed(seed) = at else {
        anyhow::bail!("No Seed")
    };
    let ti = ai.transparent.as_ref().unwrap();
    let mut addr_index = ti.index.unwrap() + 1;
    let mut gap = 0;
    while gap < gap_limit {
        let ti = derive_bip32(network, &seed, ai.aindex, 0, addr_index, true);
        create_transparent_subaccount(network, connection, account, addr_index, &ti)?;
        let address = ti.addr.encode(network);
        let utxos = get_utxos(client, account, addr_index, &address).await?;
        if utxos.is_empty() {
            gap += 1;
        } else {
            let db_tx = connection.transaction()?;
            for utxo in utxos.iter() {
                store_utxo(&db_tx, utxo)?;
            }
            db_tx.commit()?;
        }
        addr_index += 1;
    }
    trim_excess_transparent_addresses(connection, account)?;
    Ok(())
}

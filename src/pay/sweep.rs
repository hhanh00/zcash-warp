use crate::{
    db::{
        account::get_account_info,
        account_manager::{create_transparent_address, trim_excess_transparent_addresses},
        notes::store_utxo,
    },
    lwd::get_utxos,
    network::Network,
    types::TransparentAccountInfo,
    Client,
};
use anyhow::Result;
use rusqlite::Connection;
use zcash_client_backend::encoding::AddressCodec as _;

use crate::{
    coin::COINS,
    ffi::{map_result, CResult},
};
use warp_macros::c_export;

#[c_export]
pub async fn scan_transparent_addresses(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
    account: u32,
    gap_limit: u32,
) -> Result<()> {
    let ai = get_account_info(network, connection, account)?;
    let tvk = ai
        .transparent
        .as_ref()
        .and_then(|ti| ti.vk.as_ref())
        .ok_or(anyhow::anyhow!("No AccountPubKey"))?;
    let ti = ai.transparent.as_ref().unwrap();
    let mut addr_index = ai.dindex.unwrap() + 1;
    let mut gap = 0;
    while gap < gap_limit {
        let taddr = TransparentAccountInfo::derive_address(tvk, addr_index);
        let address = taddr.encode(network);
        tracing::info!("Checking {address}");
        let ti = ti.new_address(addr_index)?;
        create_transparent_address(network, connection, account, addr_index, &ti)?;
        let utxos = get_utxos(client, account, addr_index, &address).await?;
        if utxos.is_empty() {
            gap += 1;
        } else {
            let db_tx = connection.transaction()?;
            for utxo in utxos.iter() {
                store_utxo(&db_tx, utxo)?;
            }
            db_tx.commit()?;
            gap = 0;
        }
        addr_index += 1;
    }
    trim_excess_transparent_addresses(connection, account)?;
    Ok(())
}

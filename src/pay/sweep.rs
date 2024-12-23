use crate::{
    db::{
        account::get_account_info,
        account_manager::{store_transparent_address, trim_excess_transparent_addresses},
        notes::store_utxo,
    },
    keys::export_sk_bip38,
    lwd::get_utxos,
    network::Network,
    types::TransparentAccountInfo,
    Client,
};
use anyhow::Result;
use rusqlite::Connection;
use tracing::Level;
use zcash_client_backend::encoding::AddressCodec as _;

pub async fn scan_transparent_addresses(
    network: &Network,
    connection: &mut Connection,
    client: &mut Client,
    account: u32,
    external: u32,
    gap_limit: u32,
) -> Result<()> {
    let span = tracing::span!(Level::DEBUG, "scan_transparent_addresses");
    let _enter = span.enter();
    let ai = get_account_info(network, connection, account)?;
    let tvk = ai
        .transparent
        .as_ref()
        .and_then(|ti| ti.vk.as_ref())
        .ok_or(anyhow::anyhow!("No AccountPubKey"))?;
    let ti = ai.transparent.as_ref().unwrap();
    let mut addr_index = 0;
    let mut gap = 0;
    while gap < gap_limit {
        let sk = ti.xsk.as_ref().map(|xsk| {
            let sk = TransparentAccountInfo::derive_sk(xsk, external, addr_index);
            export_sk_bip38(&sk)
        });
        let taddr =
            TransparentAccountInfo::derive_address(tvk, external, addr_index).encode(network);
        tracing::event!(Level::INFO, "Checking {taddr}");
        store_transparent_address(
            connection,
            account,
            external,
            addr_index,
            sk,
            Some(taddr.clone()),
        )?;
        let utxos = get_utxos(client, account, external, addr_index, &taddr).await?;
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
    trim_excess_transparent_addresses(connection, account, external)?;
    Ok(())
}

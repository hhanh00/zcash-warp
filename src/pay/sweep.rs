use anyhow::Result;
use rusqlite::Connection;
use tonic::Request;
use zcash_client_backend::encoding::AddressCodec as _;
use zcash_primitives::consensus::Network;

use super::{Payment, PaymentBuilder, PaymentItem, UnsignedTransaction};
use crate::{
    coin::connect_lwd, db::chain::snap_to_checkpoint, keys::{Bip32KeyIterator, TSKStore}, lwd::rpc::{BlockId, BlockRange, GetAddressUtxosArg, TransparentAddressBlockFilter}, types::{AccountInfo, AccountType, PoolMask}, warp::{legacy::CommitmentTreeFrontier, UTXO}
};

pub async fn scan_utxo_by_address(
    url: String,
    account: u32,
    height: u32,
    address: String,
) -> Result<Vec<UTXO>> {
    let range = BlockRange {
        start: Some(BlockId {
            height: 1,
            hash: vec![],
        }),
        end: Some(BlockId {
            height: height as u64,
            hash: vec![],
        }),
        spam_filter_threshold: 0,
    };
    let mut client = connect_lwd(&url).await?;
    let mut txids = client
        .get_taddress_txids(Request::new(TransparentAddressBlockFilter {
            address: address.clone(),
            range: Some(range),
        }))
        .await?
        .into_inner();
    let rtx = txids.message().await?;
    if rtx.is_none() {
        return Ok(vec![]);
    }
    let mut utxos = vec![];
    let mut utxo_reps = client
        .get_address_utxos_stream(Request::new(GetAddressUtxosArg {
            addresses: vec![address.clone()],
            start_height: 1,
            max_entries: u32::MAX,
        }))
        .await?
        .into_inner();
    while let Some(utxo) = utxo_reps.message().await? {
        let utxo = UTXO {
            is_new: false,
            id: 0,
            account, // TODO Should we set account to 0 to indicate this not coming from us?
            addr_index: 0,
            height,
            timestamp: 0, // no need to retrieve block timestamp for a sweep
            txid: utxo.txid.try_into().unwrap(),
            vout: utxo.index as u32,
            address: utxo.address,
            value: utxo.value_zat as u64,
        };
        utxos.push(utxo);
    }
    Ok::<_, anyhow::Error>(utxos)
}

pub async fn scan_utxo_by_seed(
    network: &Network,
    url: &str,
    ai: AccountInfo,
    height: u32,
    addr_index: u32,
    compressed: bool,
    gap_limit: usize,
) -> Result<(Vec<UTXO>, TSKStore)> {
    let at = ai.account_type()?;
    let mut tsk_store = TSKStore::default();
    let mut utxos = vec![];
    if let AccountType::Seed(ref seed) = at {
        let mut tis = Bip32KeyIterator::new(network, seed, ai.aindex, addr_index, compressed);
        let mut gap = 0;
        loop {
            if gap >= gap_limit {
                break;
            }
            let ti = tis.next().unwrap();
            let address = ti.addr.encode(network);
            let mut funds =
                scan_utxo_by_address(url.to_string(), ai.account, height, address).await?;
            if !funds.is_empty() {
                tsk_store.0.insert(ti.addr.encode(network), ti.sk.clone().unwrap());
                utxos.append(&mut funds);
            } else {
                gap += 1;
            }
        }
    } else {
        anyhow::bail!("Account has no seed");
    }
    Ok((utxos, tsk_store))
}

pub fn prepare_sweep(
    network: &Network,
    connection: &Connection,
    account: u32,
    height: u32,
    utxos: &[UTXO],
    destination_address: &str,
    s: &CommitmentTreeFrontier,
    o: &CommitmentTreeFrontier,
) -> Result<UnsignedTransaction> {
    let amount = utxos.iter().map(|u| u.value).sum::<u64>();

    let p = Payment {
        recipients: vec![PaymentItem {
            address: destination_address.to_string(),
            amount,
            memo: None,
        }],
    };

    let height = snap_to_checkpoint(connection, height)?;
    let mut builder =
        PaymentBuilder::new(network, connection, account, height, p, PoolMask(1), &s, &o)?;
    builder.add_utxos(&utxos)?;
    builder.set_use_change(false)?;
    let mut utx = builder.prepare()?;
    let change = utx.change;
    assert!(change <= 0);
    utx.add_to_change(-change)?;
    let utx = builder.finalize(utx)?;

    println!("{:?}", utx);
    Ok(utx)
}

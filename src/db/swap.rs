use anyhow::Result;
use rusqlite::{params, Connection};

use crate::data::fb::{Swap, SwapListT, SwapT};

pub fn store_swap(connection: &Connection, account: u32, swap: &SwapT) -> Result<()> {
    let SwapT {
        provider,
        provider_id,
        timestamp,
        from_currency,
        from_amount,
        from_address,
        from_image,
        to_currency,
        to_amount,
        to_address,
        to_image,
        ..
    } = swap.clone();

    connection.execute(
        "INSERT INTO swaps(
        account,
        provider,
        provider_id,
        timestamp,
        from_currency,
        from_amount,
        from_address,
        from_image,
        to_currency,
        to_amount,
        to_address,
        to_image
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            account,
            provider.unwrap(),
            provider_id.unwrap(),
            timestamp,
            from_currency.unwrap(),
            from_amount.unwrap(),
            from_address.unwrap(),
            from_image.unwrap(),
            to_currency.unwrap(),
            to_amount.unwrap(),
            to_address.unwrap(),
            to_image.unwrap(),
        ],
    )?;
    Ok(())
}

pub fn list_swaps(connection: &Connection, account: u32) -> Result<SwapListT> {
    let mut s = connection.prepare(
        "SELECT 
        provider,
        provider_id,
        timestamp,
        from_currency,
        from_amount,
        from_address,
        from_image,
        to_currency,
        to_amount,
        to_address,
        to_image FROM swaps
        WHERE account = ?1",
    )?;
    let rows = s.query_map([account], |r| {
        let provider = r.get::<_, Option<String>>(0)?;
        let provider_id = r.get::<_, Option<String>>(1)?;
        let timestamp = r.get::<_, u32>(2)?;
        let from_currency = r.get::<_, Option<String>>(3)?;
        let from_amount = r.get::<_, Option<String>>(4)?;
        let from_address = r.get::<_, Option<String>>(5)?;
        let from_image = r.get::<_, Option<String>>(6)?;
        let to_currency = r.get::<_, Option<String>>(7)?;
        let to_amount = r.get::<_, Option<String>>(8)?;
        let to_address = r.get::<_, Option<String>>(9)?;
        let to_image = r.get::<_, Option<String>>(10)?;
        let swap = SwapT {
            provider,
            provider_id,
            timestamp,
            from_currency,
            from_amount,
            from_address,
            from_image,
            to_currency,
            to_amount,
            to_address,
            to_image,
        };
        Ok(swap)
    })?;
    let swaps = rows.collect::<Result<Vec<_>, _>>()?;
    let swaps = SwapListT { items: Some(swaps) };
    Ok(swaps)
}

pub fn clear_swap_history(connection: &Connection, account: u32) -> Result<()> {
    connection.execute("DELETE FROM swaps WHERE account = ?1", [account])?;
    Ok(())
}

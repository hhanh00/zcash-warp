use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension as _};

use crate::{data::fb::ShieldedMessageT, db::tx::get_message};

pub fn navigate_message_by_height(
    connection: &Connection,
    account: u32,
    height: u32,
    reverse: bool,
) -> Result<Option<ShieldedMessageT>> {
    let id = if !reverse {
        connection
            .query_row(
                "SELECT id_msg
            FROM msgs WHERE height = 
            (SELECT height FROM msgs WHERE account = ?1 AND height > ?2
            ORDER BY height ASC LIMIT 1)
            AND account = ?1",
                params![account, height],
                |r| r.get::<_, u32>(0),
            )
            .optional()?
    } else {
        connection
            .query_row(
                "SELECT id_msg
            FROM msgs WHERE height = 
            (SELECT height FROM msgs WHERE account = ?1 AND height < ?2
            ORDER BY height DESC LIMIT 1)
            AND account = ?1",
                params![account, height],
                |r| r.get::<_, u32>(0),
            )
            .optional()?
    };
    id.map(|id| get_message(connection, id)).transpose()
}

pub fn navigate_message_by_subject(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: &str,
    reverse: bool,
) -> Result<Option<ShieldedMessageT>> {
    let id = if !reverse {
        connection
            .query_row(
                "SELECT id_msg
            FROM msgs WHERE height = 
            (SELECT height FROM msgs WHERE account = ?1 AND height > ?2
            ORDER BY height ASC LIMIT 1)
            AND account = ?1 AND subject = ?3",
                params![account, height, subject],
                |r| r.get::<_, u32>(0),
            )
            .optional()?
    } else {
        connection
            .query_row(
                "SELECT id_msg
            FROM msgs WHERE height = 
            (SELECT height FROM msgs WHERE account = ?1 AND height < ?2
            ORDER BY height DESC LIMIT 1)
            AND account = ?1 AND subject = ?3",
                params![account, height, subject],
                |r| r.get::<_, u32>(0),
            )
            .optional()?
    };
    id.map(|id| get_message(connection, id)).transpose()
}

pub fn mark_all_read(connection: &Connection, account: u32, reverse: bool) -> Result<()> {
    connection.execute(
        "UPDATE msgs SET read = ?2 WHERE account = ?1",
        params![account, !reverse],
    )?;
    Ok(())
}

pub fn mark_read(connection: &Connection, id: u32, reverse: bool) -> Result<()> {
    connection.execute(
        "UPDATE msgs SET read = ?2 WHERE id = ?1",
        params![id, !reverse],
    )?;
    Ok(())
}

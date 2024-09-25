use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension as _, Row};

use crate::{
    data::fb::{ShieldedMessageT, UserMemoT}, fb_unwrap, txdetails::TransactionDetails
};

use crate::{
    coin::COINS,
    ffi::{map_result, map_result_bytes, CResult},
};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use warp_macros::c_export;

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

pub fn get_message(connection: &Connection, id: u32) -> Result<ShieldedMessageT> {
    let r = connection.query_row(
        "SELECT m.id_msg, m.account, m.height, m.timestamp, m.txid, m.nout, m.incoming, m.sender, 
        m.recipient, m.subject, m.body, m.read, t.id_tx FROM msgs m JOIN txs t
        ON m.txid = t.txid WHERE m.id_msg = ?1",
        [id],
        select_message,
    )?;
    let (
        id_msg,
        account,
        height,
        timestamp,
        txid,
        nout,
        incoming,
        sender,
        recipient,
        subject,
        body,
        read,
        id_tx,
    ) = r;
    let memo = UserMemoT {
        reply_to: false,
        sender,
        recipient,
        subject: Some(subject),
        body: Some(body),
    };

    let msg = ShieldedMessageT {
        id_msg,
        account,
        id_tx,
        txid: Some(txid),
        height,
        timestamp,
        incoming,
        nout,
        memo: Some(Box::new(memo)),
        read,
    };
    Ok(msg)
}

#[c_export]
pub fn list_messages(connection: &Connection, account: u32) -> Result<Vec<ShieldedMessageT>> {
    let mut s = connection.prepare(
        "SELECT m.id_msg, m.account, m.height, m.timestamp, m.txid, m.nout, m.incoming, m.sender, 
        m.recipient, m.subject, m.body, m.read, t.id_tx FROM msgs m JOIN txs t
        ON m.txid = t.txid WHERE m.account = ?1",
    )?;
    let rows = s.query_map([account], select_message)?;
    let mut msgs = vec![];
    for r in rows {
        let (
            id_msg,
            account,
            height,
            timestamp,
            txid,
            nout,
            incoming,
            sender,
            recipient,
            subject,
            body,
            read,
            id_tx,
        ) = r?;

        let memo = UserMemoT {
            reply_to: false,
            sender,
            recipient,
            subject: Some(subject),
            body: Some(body),
        };

        let msg = ShieldedMessageT {
            id_msg,
            account,
            id_tx,
            txid: Some(txid),
            height,
            timestamp,
            incoming,
            nout,
            memo: Some(Box::new(memo)),
            read,
        };
        msgs.push(msg);
    }
    Ok(msgs)
}

fn select_message(
    r: &Row,
) -> rusqlite::Result<(
    u32,
    u32,
    u32,
    u32,
    Vec<u8>,
    u32,
    bool,
    Option<String>,
    Option<String>,
    String,
    String,
    bool,
    u32,
)> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
    ))
}

pub fn store_message(
    connection: &Connection,
    account: u32,
    tx: &TransactionDetails,
    nout: u32,
    message: &ShieldedMessageT,
) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO msgs
        (account, height, timestamp, txid, nout, incoming,
        sender, recipient, subject, body, read)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, false)
        ON CONFLICT DO NOTHING",
    )?;
    let memo = fb_unwrap!(message.memo);
    s.execute(params![
        account,
        tx.height,
        tx.timestamp,
        tx.txid,
        nout,
        message.incoming,
        memo.sender,
        memo.recipient,
        memo.subject,
        memo.body
    ])?;
    Ok(())
}

#[c_export]
pub fn mark_all_read(connection: &Connection, account: u32, reverse: bool) -> Result<()> {
    connection.execute(
        "UPDATE msgs SET read = ?2 WHERE account = ?1",
        params![account, !reverse],
    )?;
    Ok(())
}

#[c_export]
pub fn mark_read(connection: &Connection, id: u32, reverse: bool) -> Result<()> {
    connection.execute(
        "UPDATE msgs SET read = ?2 WHERE id = ?1",
        params![id, !reverse],
    )?;
    Ok(())
}

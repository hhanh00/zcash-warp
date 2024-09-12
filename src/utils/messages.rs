use anyhow::Result;
use rusqlite::Connection;

use crate::{
    data::fb::ShieldedMessageT,
    db::messages::{navigate_message_by_height, navigate_message_by_subject},
};

use warp_macros::c_export;
use crate::{coin::COINS, ffi::{map_result_bytes, CResult}};
use flatbuffers::FlatBufferBuilder;
use std::ffi::{CStr, c_char};

pub fn navigate_message(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: Option<String>,
    reverse: bool,
) -> Result<Option<ShieldedMessageT>> {
    if let Some(subject) = subject {
        return navigate_message_by_subject(connection, account, height, &subject, reverse);
    }
    return navigate_message_by_height(connection, account, height, reverse);
}

#[c_export]
pub fn prev_message(
    connection: &Connection,
    account: u32,
    height: u32,
) -> Result<ShieldedMessageT> {
    navigate_message(connection, account, height, None, true).map(|m| m.unwrap_or_default())
}

#[c_export]
pub fn next_message(
    connection: &Connection,
    account: u32,
    height: u32,
) -> Result<ShieldedMessageT> {
    navigate_message(connection, account, height, None, false).map(|m| m.unwrap_or_default())
}

#[c_export]
pub fn prev_message_thread(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: &str,
) -> Result<ShieldedMessageT> {
    navigate_message(connection, account, height, Some(subject.to_string()), true).map(|m| m.unwrap_or_default())
}

#[c_export]
pub fn next_message_thread(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: &str,
) -> Result<ShieldedMessageT> {
    navigate_message(connection, account, height, Some(subject.to_string()), false).map(|m| m.unwrap_or_default())
}

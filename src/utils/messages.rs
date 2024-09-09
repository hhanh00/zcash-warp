use anyhow::Result;
use rusqlite::Connection;

use crate::{data::fb::ShieldedMessageT, db::messages::{navigate_message_by_height, navigate_message_by_subject}};

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

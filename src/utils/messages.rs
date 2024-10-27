use anyhow::Result;
use rusqlite::Connection;
use zcash_protocol::memo::Memo;

use crate::{
    data::fb::{ShieldedMessageT, UserMemoT},
    db::messages::{navigate_message_by_height, navigate_message_by_subject},
    fb_unwrap,
};

use std::str::FromStr as _;
use warp_macros::c_export;

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
    navigate_message(connection, account, height, Some(subject.to_string()), true)
        .map(|m| m.unwrap_or_default())
}

#[c_export]
pub fn next_message_thread(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: &str,
) -> Result<ShieldedMessageT> {
    navigate_message(
        connection,
        account,
        height,
        Some(subject.to_string()),
        false,
    )
    .map(|m| m.unwrap_or_default())
}

impl UserMemoT {
    pub fn from_text(sender: Option<&str>, recipient: &str, text: &str) -> Self {
        let memo_lines: Vec<_> = text.splitn(4, '\n').collect();
        let msg = if memo_lines.len() == 4 && memo_lines[0] == "\u{1F6E1}MSG" {
            UserMemoT {
                sender: if memo_lines[1].is_empty() {
                    sender.map(str::to_string)
                } else {
                    Some(memo_lines[1].to_string())
                },
                recipient: Some(recipient.to_string()),
                subject: Some(memo_lines[2].to_string()),
                body: Some(memo_lines[3].to_string()),
                reply_to: false,
            }
        } else {
            UserMemoT {
                sender: None,
                recipient: Some(recipient.to_string()),
                subject: Some(String::new()),
                body: Some(text.to_string()),
                reply_to: false,
            }
        };
        msg
    }

    pub fn to_memo(&self) -> Result<Memo> {
        let sender = if self.reply_to {
            self.sender.clone()
        } else {
            None
        };
        let sender = sender.unwrap_or_default();
        let memo_text = match &self.subject {
            Some(subject) if !subject.is_empty() || self.reply_to => {
                format!(
                    "\u{1F6E1}MSG\n{}\n{}\n{}",
                    sender,
                    subject,
                    fb_unwrap!(self.body)
                )
            }
            _ => self.body.clone().unwrap_or_default(),
        };
        Ok(Memo::from_str(&memo_text)?)
    }
}

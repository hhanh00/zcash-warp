use anyhow::Result;
use rusqlite::Connection;
use zcash_protocol::memo::Memo;

use crate::{
    data::{ShieldedMessageT, UserMemoT},
    db::messages::{navigate_message_by_height, navigate_message_by_subject},
};

use std::str::FromStr as _;

pub fn prev_message(
    connection: &Connection,
    account: u32,
    height: u32,
) -> Result<Option<ShieldedMessageT>> {
    navigate_message_by_height(connection, account, height, true)
}

pub fn next_message(
    connection: &Connection,
    account: u32,
    height: u32,
) -> Result<Option<ShieldedMessageT>> {
    navigate_message_by_height(connection, account, height, false)
}

pub fn prev_message_thread(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: &str,
) -> Result<Option<ShieldedMessageT>> {
    navigate_message_by_subject(connection, account, height, subject, true)
}

pub fn next_message_thread(
    connection: &Connection,
    account: u32,
    height: u32,
    subject: &str,
) -> Result<Option<ShieldedMessageT>> {
    navigate_message_by_subject(connection, account, height, subject, false)
}

impl UserMemoT {
    pub fn from_text(sender: Option<&str>, recipient: &str, text: &str) -> Self {
        let memo_lines: Vec<_> = text.splitn(4, '\n').collect();
        let msg = if memo_lines.len() == 4 && memo_lines[0] == "\u{1F6E1}MSG" {
            UserMemoT {
                sender: if memo_lines[1].is_empty() {
                    sender.map(|s| s.to_string()).unwrap_or_default()
                } else {
                    memo_lines[1].to_string()
                },
                recipient: recipient.to_string(),
                subject: memo_lines[2].to_string(),
                body: memo_lines[3].to_string(),
                reply_to: false,
            }
        } else {
            UserMemoT {
                sender: String::new(),
                recipient: recipient.to_string(),
                subject: String::new(),
                body: text.to_string(),
                reply_to: false,
            }
        };
        msg
    }

    pub fn to_memo(&self) -> Result<Memo> {
        let sender = if self.reply_to {
            self.sender.clone()
        } else {
            String::new()
        };
        let memo_text = if !self.subject.is_empty() || self.reply_to {
            format!("\u{1F6E1}MSG\n{}\n{}\n{}", sender, self.subject, self.body)
        } else {
            self.body.clone()
        };
        Ok(Memo::from_str(&memo_text)?)
    }
}

use anyhow::Result;
use rusqlite::{params, Connection};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::consensus::Network;

use warp_macros::c_export;
use crate::coin::COINS;
use crate::ffi::{map_result, map_result_bytes, CResult};
use crate::{data::fb::ContactCardT, types::Contact};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use std::ffi::{CStr, c_char};

pub fn store_contact(connection: &Connection, contact: &ContactCardT) -> Result<u32> {
    let id = connection.query_row(
        "INSERT INTO contacts(account, name, address, saved)
        VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO UPDATE
        SET saved = excluded.saved
        RETURNING id_contact",
        params![
            contact.account,
            contact.name,
            contact.address,
            contact.saved
        ],
        |r| r.get::<_, u32>(0),
    )?;
    Ok(id)
}

#[c_export]
pub fn list_contact_cards(connection: &Connection) -> Result<Vec<ContactCardT>> {
    let mut s =
        connection.prepare("SELECT id_contact, account, name, address, saved FROM contacts")?;
    let rows = s.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, bool>(4)?,
        ))
    })?;
    let mut cards = vec![];
    for r in rows {
        let (id, account, name, address, saved) = r?;
        let card = ContactCardT {
            id,
            account,
            name: Some(name),
            address: Some(address),
            saved,
        };
        cards.push(card);
    }
    Ok(cards)
}

pub fn list_contacts(network: &Network, connection: &Connection) -> Result<Vec<Contact>> {
    let cards = list_contact_cards(connection)?;
    let contacts = cards.iter().map(|card| {
        let recipient = RecipientAddress::decode(network, card.address.as_ref().unwrap()).unwrap();
        let contact = Contact {
            card: card.clone(),
            address: recipient,
        };
        contact
    }).collect::<Vec<_>>();
    Ok(contacts)
}

pub fn get_contact(network: &Network, connection: &Connection, id: u32) -> Result<Contact> {
    let card = get_contact_card(connection, id)?;
    let recipient = RecipientAddress::decode(network, card.address.as_ref().unwrap()).unwrap();
    let contact = Contact {
        card,
        address: recipient,
    };
    Ok(contact)
}

#[c_export]
pub fn get_contact_card(connection: &Connection, id: u32) -> Result<ContactCardT> {
    let mut s = connection.prepare(
        "SELECT account, name, address,
        saved FROM contacts WHERE id_contact = ?1",
    )?;
    let (account, name, address, saved) = s.query_row([id], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, bool>(3)?,
        ))
    })?;
    let card = ContactCardT {
        id,
        account,
        name: Some(name),
        address: Some(address),
        saved,
    };
    Ok(card)
}

#[c_export]
pub fn edit_contact_name(connection: &Connection, id: u32, name: &str) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET name = ?2 WHERE id_contact = ?1",
        params![id, name],
    )?;
    Ok(())
}

#[c_export]
pub fn edit_contact_address(connection: &Connection, id: u32, address: &str) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET address = ?2 WHERE id_contact = ?1",
        params![id, address],
    )?;
    Ok(())
}

#[c_export]
pub fn delete_contact(connection: &Connection, id: u32) -> Result<()> {
    connection.execute(
        "DELETE FROM contacts WHERE id_contact = ?1",
        [id],
    )?;
    Ok(())
}

pub fn get_unsaved_contacts(connection: &Connection, account: u32) -> Result<Vec<ContactCardT>> {
    let mut s = connection.prepare(
        "SELECT id_contact, name, address FROM contacts
        WHERE account = ?1 AND saved = FALSE")?;
    let rows = s.query_map([account], |r| Ok((
        r.get::<_, u32>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, String>(2)?,
    )))?;
    let mut cards = vec![];
    for r in rows {
        let (id, name, address) = r?;
        let card = ContactCardT {
            id,
            account,
            name: Some(name),
            address: Some(address),
            saved: false,
        };
        cards.push(card);
    }
    Ok(cards)
}

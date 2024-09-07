use anyhow::Result;
use rusqlite::{params, Connection};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::consensus::Network;

use crate::{data::fb::ContactCardT, types::Contact};

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

pub fn list_contacts(network: &Network, connection: &Connection) -> Result<Vec<Contact>> {
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
    let mut contacts = vec![];
    for r in rows {
        let (id, account, name, address, saved) = r?;
        let recipient = RecipientAddress::decode(network, &address).unwrap();
        let card = ContactCardT {
            id,
            account,
            name: Some(name),
            address: Some(address),
            saved,
        };
        let contact = Contact {
            card,
            address: recipient,
        };
        contacts.push(contact);
    }
    Ok(contacts)
}

pub fn get_contact(network: &Network, connection: &Connection, id: u32) -> Result<Contact> {
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
    let recipient = RecipientAddress::decode(network, &address).unwrap();
    let card = ContactCardT {
        id,
        account,
        name: Some(name),
        address: Some(address),
        saved,
    };
    let contact = Contact {
        card,
        address: recipient,
    };
    Ok(contact)
}

pub fn edit_contact_name(connection: &Connection, id: u32, name: &str) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET name = ?2 WHERE id_contact = ?1",
        params![id, name],
    )?;
    Ok(())
}

pub fn edit_contact_address(connection: &Connection, id: u32, address: &str) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET address = ?2 WHERE id_contact = ?1",
        params![id, address],
    )?;
    Ok(())
}

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

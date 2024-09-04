use anyhow::Result;
use rusqlite::{params, Connection};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::consensus::Network;

use crate::types::Contact;

pub fn store_contact(
    connection: &Connection,
    account: u32,
    name: &str,
    address: &str,
    dirty: bool,
) -> Result<u32> {
    let id = connection.query_row(
        "INSERT INTO contacts(account, name, address, dirty)
        VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO UPDATE
        SET dirty = excluded.dirty
        RETURNING id_contact",
        params![account, name, address, dirty],
        |r| r.get::<_, u32>(0),
    )?;
    Ok(id)
}

pub fn list_contacts(network: &Network, connection: &Connection) -> Result<Vec<Contact>> {
    let mut s =
        connection.prepare("SELECT id_contact, account, name, address, dirty FROM contacts")?;
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
        let (id, account, name, address, dirty) = r?;
        let address = RecipientAddress::decode(network, &address).unwrap();
        let contact = Contact {
            id,
            account,
            name,
            address,
            dirty,
        };
        contacts.push(contact);
    }
    Ok(contacts)
}

pub fn get_contact(network: &Network, connection: &Connection, id: u32) -> Result<Contact> {
    let mut s = connection.prepare(
        "SELECT account, name, address,
        dirty FROM contacts WHERE id_contact = ?1",
    )?;
    let (account, name, address, dirty) = s.query_row([id], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, bool>(3)?,
        ))
    })?;
    let address = RecipientAddress::decode(network, &address).unwrap();
    let contact = Contact {
        id,
        account,
        name,
        address,
        dirty,
    };
    Ok(contact)
}

pub fn delete_contact(connection: &Connection, id_contact: u32, tpe: u8) -> Result<()> {
    connection.execute(
        "DELETE FROM contacts WHERE id_contact = ?1",
        params![id_contact],
    )?;
    Ok(())
}

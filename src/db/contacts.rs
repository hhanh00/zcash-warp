use crate::network::Network;
use crate::utils::ua::split_address;
use anyhow::Result;
use rusqlite::{params, Connection};
use zcash_keys::address::Address as RecipientAddress;

use crate::{data::ContactCardT, types::Contact};

pub fn store_contact(
    network: &Network,
    connection: &Connection,
    contact: &ContactCardT,
) -> Result<u32> {
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
    upsert_contact_receivers(network, connection, id, &contact.address)?;
    Ok(id)
}

pub fn list_contact_cards(connection: &Connection) -> Result<Vec<ContactCardT>> {
    let mut s = connection
        .prepare("SELECT id_contact, account, name, address, saved FROM contacts ORDER BY name")?;
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
            name,
            address,
            saved,
        };
        cards.push(card);
    }
    Ok(cards)
}

pub fn list_contacts(network: &Network, connection: &Connection) -> Result<Vec<Contact>> {
    let cards = list_contact_cards(connection)?;
    let contacts = cards
        .iter()
        .map(|card| {
            let recipient = RecipientAddress::decode(network, &card.address).unwrap();
            let contact = Contact {
                card: card.clone(),
                address: recipient,
            };
            contact
        })
        .collect::<Vec<_>>();
    Ok(contacts)
}

pub fn get_contact(network: &Network, connection: &Connection, id: u32) -> Result<Contact> {
    let card = get_contact_card(connection, id)?;
    let recipient = RecipientAddress::decode(network, &card.address).unwrap();
    let contact = Contact {
        card,
        address: recipient,
    };
    Ok(contact)
}

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
        name,
        address,
        saved,
    };
    Ok(card)
}

pub fn edit_contact_name(connection: &Connection, id: u32, name: &str) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET name = ?2 WHERE id_contact = ?1",
        params![id, name],
    )?;
    Ok(())
}

pub fn address_to_bytes(network: &Network, address: &str) -> Result<Vec<u8>> {
    if address.is_empty() {
        return Ok(vec![]);
    }
    let (t, s, o, _) = split_address(network, address)?;
    if let Some(t) = t {
        Ok(t.script().0.to_vec())
    } else if let Some(s) = s {
        Ok(s.to_bytes().to_vec())
    } else if let Some(o) = o {
        Ok(o.to_raw_address_bytes().to_vec())
    } else {
        Err(anyhow::anyhow!("No Receiver"))
    }
}

pub fn edit_contact_address(
    network: &Network,
    connection: &Connection,
    id: u32,
    address: &str,
) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET address = ?2 WHERE id_contact = ?1",
        params![id, address],
    )?;
    upsert_contact_receivers(network, connection, id, address)?;
    Ok(())
}

pub fn upsert_contact_receivers(
    network: &Network,
    connection: &Connection,
    id: u32,
    address: &str,
) -> Result<()> {
    let account = connection.query_row(
        "SELECT account FROM contacts WHERE id_contact = ?1",
        [id],
        |r| r.get::<_, u32>(0),
    )?;

    let (t, s, o, _) = split_address(network, address)?;
    connection.execute("DELETE FROM contact_receivers WHERE contact = ?1", [id])?;
    if let Some(t) = t {
        connection.execute(
            "INSERT INTO contact_receivers
            (account, contact, pool, address)
            VALUES (?1, ?2, ?3, ?4)",
            params![account, id, 0, t.script().0.to_vec()],
        )?;
    }
    if let Some(s) = s {
        connection.execute(
            "INSERT INTO contact_receivers
            (account, contact, pool, address)
            VALUES (?1, ?2, ?3, ?4)",
            params![account, id, 1, s.to_bytes().to_vec()],
        )?;
    }
    if let Some(o) = o {
        connection.execute(
            "INSERT INTO contact_receivers
            (account, contact, pool, address)
            VALUES (?1, ?2, ?3, ?4)",
            params![account, id, 2, o.to_raw_address_bytes().to_vec()],
        )?;
    }

    Ok(())
}

pub fn delete_contact(connection: &Connection, id: u32) -> Result<()> {
    connection.execute("DELETE FROM contacts WHERE id_contact = ?1", [id])?;
    Ok(())
}

pub fn get_unsaved_contacts(connection: &Connection, account: u32) -> Result<Vec<ContactCardT>> {
    let mut s = connection.prepare(
        "SELECT id_contact, name, address FROM contacts
        WHERE account = ?1 AND saved = FALSE",
    )?;
    let rows = s.query_map([account], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut cards = vec![];
    for r in rows {
        let (id, name, address) = r?;
        let card = ContactCardT {
            id,
            account,
            name,
            address,
            saved: false,
        };
        cards.push(card);
    }
    Ok(cards)
}

pub fn on_contacts_saved(connection: &Connection, account: u32) -> Result<()> {
    connection.execute(
        "UPDATE contacts SET saved = TRUE WHERE account = ?1",
        [account],
    )?;
    Ok(())
}

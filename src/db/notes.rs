use crate::{
    data::fb::{IdNoteT, InputTransparentT, ShieldedNoteT},
    types::CheckpointHeight,
    warp::{
        sync::{PlainNote, ReceivedNote, ReceivedTx, TxValueUpdate},
        BlockHeader, OutPoint, Witness, UTXO,
    },
    Hash,
};
use anyhow::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

use crate::{
    coin::COINS,
    ffi::{map_result, map_result_bytes, CResult},
};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use warp_macros::c_export;

use super::tx::{add_tx_value, store_tx};

pub fn get_note_by_nf(connection: &Connection, nullifier: &Hash) -> Result<Option<PlainNote>> {
    let r = connection
        .query_row(
            "SELECT address, value, rcm, rho FROM notes WHERE nf = ?1",
            [nullifier],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(address, value, rcm, rho)| {
            Ok::<_, Error>(PlainNote {
                address: address.try_into().unwrap(),
                value,
                rcm: rcm.try_into().unwrap(),
                rho: rho.map(|rho| rho.try_into().unwrap()),
            })
        })
        .transpose()?;
    Ok(r)
}

fn select_note(row: &Row) -> Result<ReceivedNote, rusqlite::Error> {
    let (
        id_note,
        account,
        position,
        height,
        vout,
        address,
        value,
        rcm,
        nf,
        rho,
        spent,
        txid,
        timestamp,
        tx_value,
        witness,
    ) = (
        row.get::<_, u32>(0)?,
        row.get::<_, u32>(1)?,
        row.get::<_, u32>(2)?,
        row.get::<_, u32>(3)?,
        row.get::<_, u32>(4)?,
        row.get::<_, [u8; 43]>(5)?,
        row.get::<_, u64>(6)?,
        row.get::<_, Hash>(7)?,
        row.get::<_, Hash>(8)?,
        row.get::<_, Option<Hash>>(9)?,
        row.get::<_, Option<u32>>(10)?,
        row.get::<_, Hash>(11)?,
        row.get::<_, u32>(12)?,
        row.get::<_, i64>(13)?,
        row.get::<_, Vec<u8>>(14)?,
    );
    let note = ReceivedNote {
        is_new: false,
        id: id_note,
        account,
        position,
        height,
        address,
        value,
        rcm,
        nf,
        rho,
        vout,
        tx: ReceivedTx {
            id: 0,
            account,
            height,
            txid,
            timestamp,
            value: tx_value,
            ivtx: 0, // not persisted
        },
        spent,
        witness: bincode::deserialize_from(&*witness).unwrap(),
    };
    Ok(note)
}

pub fn list_all_received_notes(
    connection: &Connection,
    height: CheckpointHeight,
    orchard: bool,
) -> Result<Vec<ReceivedNote>> {
    let height: u32 = height.into();
    let mut s = connection.prepare(
        "SELECT n.id_note, n.account, n.position, n.height, n.output_index, n.address,
        n.value, n.rcm, n.nf, n.rho, n.spent, t.txid, t.timestamp, t.value, w.witness
        FROM notes n, txs t, witnesses w WHERE n.tx = t.id_tx AND w.note = n.id_note AND w.height = ?1
        AND orchard = ?2 AND (spent IS NULL OR spent > ?1 OR spent = 0) AND NOT excluded
        ORDER BY n.value DESC")?;
    let rows = s.query_map(params![height, orchard], select_note)?;
    let notes = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(notes)
}

pub fn list_received_notes(
    connection: &Connection,
    account: u32,
    height: CheckpointHeight,
    orchard: bool,
) -> Result<Vec<ReceivedNote>> {
    let height: u32 = height.into();
    let mut s = connection.prepare(
        "SELECT n.id_note, n.account, n.position, n.height, n.output_index, n.address,
        n.value, n.rcm, n.nf, n.rho, n.spent, t.txid, t.timestamp, t.value, w.witness
        FROM notes n, txs t, witnesses w WHERE n.tx = t.id_tx AND w.note = n.id_note AND w.height = ?1
        AND orchard = ?2 AND (spent IS NULL OR spent > ?1 AND n.account = ?3) AND NOT excluded
        ORDER BY n.value DESC")?;
    let rows = s.query_map(params![height, orchard, account], select_note)?;
    let notes = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(notes)
}

pub fn mark_shielded_spent(connection: &Transaction, tx_value: &TxValueUpdate<Hash>) -> Result<()> {
    let mut s = connection.prepare("UPDATE notes SET spent = ?2 WHERE nf = ?1")?;
    s.execute(params![tx_value.id_spent.unwrap(), tx_value.height])?;
    Ok(())
}

pub fn mark_notes_unconfirmed_spent(connection: &Connection, id_notes: &[IdNoteT]) -> Result<()> {
    let mut upd_transparent =
        connection.prepare("UPDATE utxos SET spent = 0 WHERE id_utxo = ?1")?;
    let mut upd_shielded = connection.prepare("UPDATE notes SET spent = 0 WHERE id_note = ?1")?;
    for note in id_notes {
        match note.pool {
            0 => {
                upd_transparent.execute([note.id])?;
            }
            1 | 2 => {
                upd_shielded.execute([note.id])?;
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

pub fn mark_transparent_spent(
    connection: &Transaction,
    tx_value: &TxValueUpdate<OutPoint>,
) -> Result<()> {
    let OutPoint { txid, vout } = tx_value.id_spent.as_ref().unwrap();
    let mut s = connection.prepare("UPDATE utxos SET spent = ?3 WHERE txid = ?1 AND vout = ?2")?;
    s.execute(params![txid, vout, tx_value.height])?;
    Ok(())
}

pub fn store_received_note(
    connection: &Transaction,
    height: u32,
    notes: &[ReceivedNote],
) -> Result<()> {
    let mut s_note = connection.prepare_cached(
        "INSERT INTO notes
    (account, position, height, tx, output_index, address, value, rcm, nf, rho, spent, orchard, excluded)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, FALSE)",
    )?;
    for n in notes {
        let orchard = n.rho.is_some();
        if n.is_new {
            store_tx(connection, &n.tx)?;
            add_tx_value(
                connection,
                &TxValueUpdate::<()> {
                    id_tx: 0,
                    account: n.account,
                    height: n.height,
                    txid: n.tx.txid,
                    timestamp: n.tx.timestamp,
                    value: n.tx.value,
                    id_spent: None,
                },
            )?;
            let id_tx = connection.query_row(
                "SELECT id_tx FROM txs WHERE txid = ?1",
                [n.tx.txid],
                |r| r.get::<_, u32>(0),
            )?;
            s_note.execute(params![
                n.account, n.position, n.height, id_tx, n.vout, n.address, n.value, n.rcm, n.nf,
                n.rho, n.spent, orchard,
            ])?;
        }
        let id_note = connection.query_row(
            "SELECT id_note FROM notes
            WHERE position = ?1 AND orchard = ?2",
            params![n.position, orchard],
            |r| r.get::<_, u32>(0),
        )?;
        store_witness(connection, n.account, id_note, height, &n.witness)?;
    }

    Ok(())
}

pub fn store_witness(
    connection: &Transaction,
    account: u32,
    id_note: u32,
    height: u32,
    witness: &Witness,
) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO witnesses
        (account, note, height, witness) VALUES (?1, ?2, ?3, ?4)",
    )?;
    s.execute(params![
        account,
        id_note,
        height,
        bincode::serialize(witness).unwrap()
    ])?;
    Ok(())
}

fn select_utxo(r: &Row) -> Result<UTXO, rusqlite::Error> {
    let (id_utxo, account, addr_index, height, timestamp, txid, vout, address, value) = (
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get::<_, Vec<u8>>(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
    );

    let utxo = UTXO {
        is_new: false,
        id: id_utxo,
        account,
        addr_index,
        height,
        timestamp,
        txid: txid.try_into().unwrap(),
        vout,
        address,
        value,
    };
    Ok(utxo)
}

pub fn list_all_utxos(connection: &Connection, height: CheckpointHeight) -> Result<Vec<UTXO>> {
    let height: u32 = height.into();
    // include the unconfirmed spents
    let mut s = connection.prepare(
        "SELECT u.id_utxo, u.account, u.addr_index, u.height, u.timestamp, u.txid, u.vout, s.address,
        u.value FROM utxos u
        JOIN t_accounts t ON u.account = t.account
        JOIN t_addresses s ON s.account = t.account AND s.addr_index = u.addr_index
        WHERE u.height <= ?1 AND (u.spent IS NULL OR u.spent > ?1 OR u.spent = 0)
        ORDER BY u.height DESC"
    )?;
    let rows = s.query_map([height], select_utxo)?;
    let utxos = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(utxos)
}

pub fn list_utxos(
    connection: &Connection,
    account: u32,
    height: CheckpointHeight,
) -> Result<Vec<UTXO>> {
    let height: u32 = height.into();
    // exclude unconfirmed spents
    let mut s = connection.prepare(
        &("SELECT u.id_utxo, u.account, u.addr_index, u.height, u.timestamp, u.txid, u.vout, s.address,
        u.value FROM utxos u
        JOIN t_accounts t ON u.account = t.account
        JOIN t_addresses s ON s.account = t.account AND s.addr_index = u.addr_index
        WHERE u.height <= ?1 AND (u.spent IS NULL OR u.spent > ?1)
        AND u.account = ?2 ORDER BY u.height DESC"),
    )?;
    let rows = s.query_map(params![height, account], select_utxo)?;
    let utxos = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(utxos)
}

pub fn store_utxo(connection: &Transaction, utxo: &UTXO) -> Result<()> {
    if utxo.is_new {
        let mut s = connection.prepare_cached(
            "INSERT INTO utxos
            (account, height, timestamp, txid, vout, addr_index, value, spent)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT DO NOTHING",
        )?;
        s.execute(params![
            utxo.account,
            utxo.height,
            utxo.timestamp,
            utxo.txid,
            utxo.vout,
            utxo.addr_index,
            utxo.value,
            None::<u32>
        ])?;
        let tx_value = TxValueUpdate::<OutPoint> {
            id_tx: 0,
            account: utxo.account,
            txid: utxo.txid,
            value: utxo.value as i64,
            height: utxo.height,
            timestamp: utxo.timestamp,
            id_spent: None,
        };
        add_tx_value(connection, &tx_value)?;
    }
    Ok(())
}

pub fn update_tx_timestamp<'a, I: IntoIterator<Item = &'a Option<BlockHeader>>>(
    connection: &Transaction,
    headers: I,
) -> Result<()> {
    let mut s = connection.prepare_cached("UPDATE txs SET timestamp = ?2 WHERE height = ?1")?;
    for bh in headers {
        if let Some(bh) = bh {
            s.execute(params![bh.height, bh.timestamp])?;
        }
    }
    Ok(())
}

pub fn update_account_balances(connection: &Transaction, height: u32) -> Result<()> {
    connection.execute(
        "UPDATE accounts SET balance = coins.balance FROM 
        (WITH coins AS
        (SELECT account, value, spent FROM notes UNION 
        SELECT account, value, spent FROM utxos)
        SELECT account, SUM(value) AS balance FROM coins 
        WHERE spent IS NULL OR spent > ?1 GROUP BY account)
        AS coins WHERE coins.account = accounts.id_account",
        [height],
    )?;
    Ok(())
}

#[c_export]
pub fn get_unspent_notes(
    connection: &Connection,
    account: u32,
    bc_height: u32,
) -> Result<Vec<ShieldedNoteT>> {
    let mut s = connection.prepare(
        "SELECT n.id_note, n.height, t.timestamp, n.value, n.orchard, n.excluded
        FROM notes n JOIN txs t ON n.tx = t.id_tx
        WHERE n.account = ?1 AND (spent IS NULL OR spent > ?2 OR spent = 0)
        ORDER BY n.height DESC",
    )?;
    let rows = s.query_map(params![account, bc_height], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, u64>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, bool>(5)?,
        ))
    })?;
    let mut notes = vec![];
    for r in rows {
        let (id, height, timestamp, value, orchard, excluded) = r?;
        let note = ShieldedNoteT {
            id_note: id,
            height,
            confirmations: bc_height - height + 1,
            timestamp,
            value,
            orchard,
            excluded,
        };
        notes.push(note);
    }
    Ok(notes)
}

#[c_export]
pub fn get_unspent_utxos(
    connection: &Connection,
    account: u32,
    bc_height: u32,
) -> Result<Vec<InputTransparentT>> {
    let utxos = list_utxos(connection, account, CheckpointHeight(bc_height))?;
    let utxos = utxos
        .into_iter()
        .map(|u| InputTransparentT {
            txid: Some(u.txid.to_vec()),
            vout: u.vout,
            address: Some(u.address),
            value: u.value,
        })
        .collect::<Vec<_>>();
    Ok(utxos)
}

#[c_export]
pub fn exclude_note(connection: &Connection, id: u32, reverse: bool) -> Result<()> {
    connection.execute(
        "UPDATE notes SET excluded = ?2 WHERE id_note = ?1",
        params![id, !reverse],
    )?;
    Ok(())
}

#[c_export]
pub fn reverse_note_exclusion(connection: &Connection, account: u32) -> Result<()> {
    connection.execute(
        "UPDATE notes SET excluded = NOT excluded WHERE account = ?1",
        [account],
    )?;
    Ok(())
}

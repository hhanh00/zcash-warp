use crate::{
    data::fb::{IdNoteT, InputTransparentT, ShieldedNoteT},
    types::CheckpointHeight,
    utils::ContextExt,
    warp::{
        sync::{IdSpent, PlainNote, ReceivedNote, ReceivedTx, TxValueUpdate},
        BlockHeader, OutPoint, Witness, STXO, UTXO,
    },
    Hash,
};
use anyhow::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

use warp_macros::c_export;

use super::tx::{add_tx_value, store_tx};

pub fn get_note_by_nf(
    connection: &Connection,
    account: u32,
    nullifier: &Hash,
) -> Result<Option<PlainNote>> {
    let r = connection
        .query_row(
            "SELECT id_note, address, value, rcm, rho FROM notes WHERE nf = ?1 AND account = ?2",
            params![nullifier, account],
            |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, Option<Vec<u8>>>(4)?,
                ))
            },
        )
        .optional()?
        .map(|(id_note, address, value, rcm, rho)| {
            Ok::<_, Error>(PlainNote {
                id: id_note,
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

// used by synchronization
// must returned all the nodes including the ones spent but unconfirmed
pub fn list_all_received_notes(
    connection: &Connection,
    height: CheckpointHeight,
    orchard: bool,
) -> Result<Vec<ReceivedNote>> {
    let height: u32 = height.into();
    let mut s = connection.prepare(
        "SELECT n.id_note, n.account, n.position, n.height, n.output_index, n.address,
        n.value, n.rcm, n.nf, n.rho, n.spent, t.txid, t.timestamp, t.value, w.witness
        FROM notes n, txs t, witnesses w WHERE
        n.tx = t.id_tx AND n.account = t.account
        AND w.account = n.account AND w.note = n.id_note AND w.height = ?1
        AND orchard = ?2 AND spent IS NULL
        ORDER BY n.value DESC",
    )?;
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
        FROM notes n, txs t, witnesses w
        WHERE n.tx = t.id_tx AND n.account = t.account
        AND w.note = n.id_note AND w.account = n.account AND w.height = ?1
        AND orchard = ?2 AND spent IS NULL AND n.account = ?3 AND NOT excluded
        AND n.height <= ?1 AND n.expiration IS NULL
        ORDER BY n.value DESC",
    )?;
    let rows = s.query_map(params![height, orchard, account], select_note)?;
    let notes = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(notes)
}

pub fn mark_shielded_spent(connection: &Transaction, id_spent: &IdSpent<Hash>) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO note_spends(id_note, account, height, id_tx)
        SELECT n.id_note, ?1, ?2, t.id_tx FROM notes n
        JOIN txs t
        WHERE n.nf = ?3 AND n.account = ?1
        AND t.txid = ?4 AND t.account = ?1
        RETURNING id_note",
    )?;
    let id_note = s
        .query_row(
            params![
                id_spent.account,
                id_spent.height,
                &id_spent.note_ref,
                &id_spent.txid
            ],
            |r| r.get::<_, u32>(0),
        )
        .with_file_line(|| format!("{}", hex::encode(&id_spent.note_ref)))?;
    let mut s = connection
        .prepare_cached("UPDATE notes SET spent = ?2, expiration = NULL WHERE id_note = ?1")?;
    s.execute(params![id_note, id_spent.height])?;
    Ok(())
}

pub fn mark_transparent_spent(
    connection: &Transaction,
    id_spent: &IdSpent<OutPoint>,
) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO utxo_spends(id_utxo, account, height, id_tx)
        SELECT u.id_utxo, ?1, ?2, t.id_tx FROM utxos u
        JOIN txs t
        WHERE u.txid = ?3 AND u.vout = ?4 AND u.account = ?1
        AND t.txid = ?5 AND t.account = ?1
        ON CONFLICT DO UPDATE SET account = excluded.account
        RETURNING id_utxo",
    )?;
    let id_utxo = s.query_row(
        params![
            id_spent.account,
            id_spent.height,
            id_spent.note_ref.txid,
            id_spent.note_ref.vout,
            &id_spent.txid
        ],
        |r| r.get::<_, u32>(0),
    )?;
    let mut s = connection
        .prepare_cached("UPDATE utxos SET spent = ?2, expiration = NULL WHERE id_utxo = ?1")?;
    s.execute(params![id_utxo, id_spent.height])?;
    Ok(())
}

pub fn mark_notes_unconfirmed_spent(
    connection: &Connection,
    id_notes: &[IdNoteT],
    expiration: u32,
) -> Result<()> {
    let mut upd_transparent =
        connection.prepare("UPDATE utxos SET expiration = ?2 WHERE id_utxo = ?1")?;
    let mut upd_shielded =
        connection.prepare("UPDATE notes SET expiration = ?2 WHERE id_note = ?1")?;
    for note in id_notes {
        match note.pool {
            0 => {
                upd_transparent.execute([note.id, expiration])?;
            }
            1 | 2 => {
                upd_shielded.execute([note.id, expiration])?;
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

pub fn recover_expired_spends(connection: &Connection, height: u32) -> Result<()> {
    connection.execute(
        "UPDATE notes SET expiration = NULL WHERE expiration < ?1",
        [height],
    )?;
    connection.execute(
        "UPDATE utxos SET expiration = NULL WHERE expiration < ?1",
        [height],
    )?;
    connection.execute("DELETE FROM txs WHERE expiration < ?1", [height])?;
    connection.execute(
        "UPDATE utxos SET expiration = NULL WHERE expiration < ?1",
        [height],
    )?;
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
            let id_tx = store_tx(connection, &n.tx)?;
            add_tx_value(
                connection,
                &TxValueUpdate {
                    id_tx: 0,
                    account: n.account,
                    height: n.height,
                    txid: n.tx.txid,
                    timestamp: n.tx.timestamp,
                    value: n.tx.value,
                },
            )?;
            s_note.execute(params![
                n.account, n.position, n.height, id_tx, n.vout, n.address, n.value, n.rcm, n.nf,
                n.rho, n.spent, orchard,
            ])?;
        }
        let id_note = connection.query_row(
            "SELECT id_note FROM notes
            WHERE account = ?1 AND position = ?2 AND orchard = ?3",
            params![n.account, n.position, orchard],
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
    let (id_utxo, account, external, addr_index, height, timestamp, txid, vout, address, value) = (
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get::<_, Vec<u8>>(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
    );

    let utxo = UTXO {
        is_new: false,
        id: id_utxo,
        account,
        external,
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

// include unconfirmed spent
pub fn list_all_utxos(connection: &Connection) -> Result<Vec<UTXO>> {
    // include the unconfirmed spents
    let mut s = connection.prepare(
        "SELECT u.id_utxo, u.account, u.external, u.addr_index, u.height, u.timestamp, u.txid, u.vout, s.address,
        u.value FROM utxos u
        JOIN t_accounts t ON u.account = t.account
        JOIN t_addresses s ON s.account = t.account
            AND s.external = u.external
            AND s.addr_index = u.addr_index
        WHERE u.spent IS NULL
        ORDER BY u.height DESC"
    )?;
    let rows = s.query_map([], select_utxo)?;
    let utxos = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(utxos)
}

// List the unconfirmed and spent tx outputs
pub fn list_pending_stxos(connection: &Connection, account: u32) -> Result<Vec<STXO>> {
    let mut s = connection.prepare(
        &("SELECT u.txid, u.vout, u.value, s.address FROM utxos u
        JOIN t_accounts t ON u.account = t.account
        JOIN t_addresses s ON t.account = s.account
            AND u.external = s.external
            AND u.addr_index = s.addr_index
        WHERE u.spent IS NULL
        AND u.expiration IS NOT NULL
        AND u.account = ?1"),
    )?;
    let rows = s.query_map([account], |r| {
        let txid = r.get::<_, Vec<u8>>(0)?;
        let vout = r.get::<_, u32>(1)?;
        let value = r.get::<_, u64>(2)?;
        let address = r.get::<_, String>(3)?;
        Ok(STXO {
            account,
            txid: txid.try_into().unwrap(),
            vout,
            value,
            address,
        })
    })?;
    let stxos = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(stxos)
}

pub fn list_utxos(
    connection: &Connection,
    account: u32,
    height: CheckpointHeight,
) -> Result<Vec<UTXO>> {
    let height: u32 = height.into();
    // exclude unconfirmed spents
    let mut s = connection.prepare(
        &("SELECT u.id_utxo, u.account, u.external, u.addr_index, u.height, u.external, u.txid, u.vout, s.address,
        u.value FROM utxos u
        JOIN t_accounts t ON u.account = t.account
        JOIN t_addresses s ON t.account = s.account
            AND u.external = s.external
            AND u.addr_index = s.addr_index
        WHERE u.height <= ?1 AND (u.spent IS NULL OR u.spent > ?1)
        AND u.expiration IS NULL
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
            (account, height, timestamp, txid, vout, external, addr_index, value, spent)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT DO NOTHING",
        )?;
        s.execute(params![
            utxo.account,
            utxo.height,
            utxo.timestamp,
            utxo.txid,
            utxo.vout,
            utxo.external,
            utxo.addr_index,
            utxo.value,
            None::<u32>
        ])?;
        let tx_value = TxValueUpdate {
            id_tx: 0,
            account: utxo.account,
            txid: utxo.txid,
            value: utxo.value as i64,
            height: utxo.height,
            timestamp: utxo.timestamp,
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

pub fn update_account_balances(connection: &Transaction) -> Result<()> {
    connection.execute(
        "UPDATE accounts SET balance = balances.balance FROM
        (WITH 
            coins AS (SELECT account, value, spent FROM notes UNION ALL
                SELECT account, value, spent FROM utxos),
            unspent AS (SELECT account, SUM(value) AS balance , spent FROM coins WHERE spent IS NULL GROUP BY account)
		SELECT id_account, COALESCE(u.balance, 0) AS balance FROM accounts a
		LEFT JOIN unspent u ON a.id_account = u.account) AS balances
        WHERE balances.id_account = accounts.id_account",
        [],
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
        WHERE n.account = ?1 AND (spent IS NULL OR spent > ?2) AND n.expiration IS NULL
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

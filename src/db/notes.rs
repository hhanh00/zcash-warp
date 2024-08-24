use crate::{
    warp::{
        sync::{PlainNote, ReceivedNote, ReceivedTx, TxValueUpdate},
        BlockHeader, OutPoint, Witness, UTXO,
    },
    Hash,
};
use anyhow::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zcash_primitives::consensus::{Network, NetworkUpgrade, Parameters};

pub fn get_note_by_nf(connection: &Connection, nullifier: &Hash) -> Result<Option<PlainNote>> {
    let r = connection
        .query_row(
            "SELECT diversifier, value, rcm, rho FROM notes WHERE nf = ?1",
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
        .map(|(diversifier, value, rcm, rho)| {
            Ok::<_, Error>(PlainNote {
                diversifier: diversifier.try_into().unwrap(),
                value,
                rcm: rcm.try_into().unwrap(),
                rho: rho.map(|rho| rho.try_into().unwrap()),
            })
        })
        .transpose()?;
    Ok(r)
}

pub fn list_received_notes(
    connection: &Connection,
    height: u32,
    orchard: bool,
) -> Result<Vec<ReceivedNote>> {
    let mut s = connection.prepare(
        "SELECT n.id_note, n.account, n.position, n.height, n.output_index, n.diversifier,
        n.value, n.rcm, n.nf, n.rho, n.spent, t.txid, t.timestamp, t.value, w.witness
        FROM notes n, txs t, witnesses w WHERE n.tx = t.id_tx AND w.note = n.id_note AND w.height = ?1
        AND orchard = ?2 AND (spent IS NULL OR spent > ?1)
        ORDER BY n.value DESC",
    )?;
    let rows = s.query_map(params![height, orchard], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, u32>(3)?,
            r.get::<_, u32>(4)?,
            r.get::<_, [u8; 11]>(5)?,
            r.get::<_, u64>(6)?,
            r.get::<_, Hash>(7)?,
            r.get::<_, Hash>(8)?,
            r.get::<_, Option<Hash>>(9)?,
            r.get::<_, Option<u32>>(10)?,
            r.get::<_, Hash>(11)?,
            r.get::<_, u32>(12)?,
            r.get::<_, i64>(13)?,
            r.get::<_, Vec<u8>>(14)?,
        ))
    })?;
    let mut notes = vec![];
    for r in rows {
        let (
            id_note,
            account,
            position,
            height,
            vout,
            diversifier,
            value,
            rcm,
            nf,
            rho,
            spent,
            txid,
            timestamp,
            tx_value,
            witness,
        ) = r?;
        let note = ReceivedNote {
            is_new: false,
            id: id_note,
            account,
            position,
            height,
            diversifier,
            value,
            rcm,
            nf,
            rho,
            vout,
            tx: ReceivedTx {
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
        notes.push(note);
    }
    Ok(notes)
}

pub fn store_tx(connection: &Transaction, tx: &ReceivedTx) -> Result<()> {
    let mut s_tx = connection.prepare_cached(
        "INSERT INTO txs
        (account, txid, height, timestamp, value)
        VAlUES (?1, ?2, ?3, ?4, 0)
        ON CONFLICT DO NOTHING",
    )?;
    s_tx.execute(params![tx.account, tx.txid, tx.height, tx.timestamp,])?;
    Ok(())
}

pub fn add_tx_value<IDSpent: std::fmt::Debug>(
    connection: &Transaction,
    tx_value: &TxValueUpdate<IDSpent>,
) -> Result<()> {
    let mut s_tx =
        connection.prepare_cached("UPDATE txs SET value = value + ?2 WHERE txid = ?1")?;
    s_tx.execute(params![tx_value.txid, tx_value.value])?;
    Ok(())
}

pub fn mark_shielded_spent(connection: &Transaction, tx_value: &TxValueUpdate<Hash>) -> Result<()> {
    let mut s = connection.prepare("UPDATE notes SET spent = ?2 WHERE nf = ?1")?;
    s.execute(params![tx_value.id_spent.unwrap(), tx_value.height])?;
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
    (account, position, height, tx, output_index, diversifier, value, rcm, nf, rho, spent, orchard)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                n.account,
                n.position,
                n.height,
                id_tx,
                n.vout,
                n.diversifier,
                n.value,
                n.rcm,
                n.nf,
                n.rho,
                n.spent,
                orchard,
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

pub fn get_block_header(connection: &Connection, height: u32) -> Result<BlockHeader> {
    let (hash, prev_hash, timestamp) = connection.query_row(
        "SELECT hash, prev_hash, timestamp FROM blcks WHERE height = ?1",
        [height],
        |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, u32>(2)?,
            ))
        },
    )?;
    Ok(BlockHeader {
        height,
        hash: hash.try_into().unwrap(),
        prev_hash: prev_hash.try_into().unwrap(),
        timestamp,
    })
}

pub fn store_block(connection: &Transaction, bh: &BlockHeader) -> Result<()> {
    let mut s = connection.prepare_cached(
        "INSERT INTO blcks
        (height, hash, prev_hash, timestamp) VALUES (?1, ?2, ?3, ?4)",
    )?;
    s.execute(params![bh.height, bh.hash, bh.prev_hash, bh.timestamp,])?;
    Ok(())
}

pub fn list_utxos(connection: &Connection, height: u32) -> Result<Vec<UTXO>> {
    let mut s = connection.prepare(
        "SELECT u.id_utxo, u.account, u.height, u.txid, u.vout, t.address,
        u.value FROM utxos u, taddrs t WHERE u.height <= ?1 AND (u.spent IS NULL OR u.spent > ?1)
        AND u.account = t.account",
    )?;
    let rows = s.query_map([height], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, Vec<u8>>(3)?,
            r.get::<_, u32>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, u64>(6)?,
        ))
    })?;
    let mut utxos = vec![];
    for r in rows {
        let (id_utxo, account, height, txid, vout, address, value) = r?;
        let utxo = UTXO {
            is_new: false,
            id: id_utxo,
            account,
            height,
            txid: txid.try_into().unwrap(),
            vout,
            address,
            value,
        };
        utxos.push(utxo);
    }

    Ok(utxos)
}

pub fn store_utxo(
    connection: &Transaction,
    utxo: &UTXO,
    // tx: &ReceivedTx,
    // outpoint: &OutPoint,
    // value: u64,
) -> Result<()> {
    if utxo.is_new {
        let mut s = connection.prepare_cached(
            "INSERT INTO utxos
        (account, height, txid, vout, value, spent)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        s.execute(params![
            utxo.account,
            utxo.height,
            utxo.txid,
            utxo.vout,
            utxo.value,
            None::<u32>
        ])?;
        let tx_value = TxValueUpdate::<OutPoint> {
            id_tx: 0,
            account: utxo.account,
            txid: utxo.txid,
            value: utxo.value as i64,
            height: utxo.height,
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

pub fn get_sync_height(connection: &Connection) -> Result<Option<u32>> {
    let height = connection.query_row("SELECT MAX(height) FROM blcks", [], |r| {
        r.get::<_, Option<u32>>(0)
    })?;
    Ok(height)
}

pub fn truncate_scan(connection: &Connection) -> Result<()> {
    connection.execute("DELETE FROM blcks", [])?;
    connection.execute("DELETE FROM txs", [])?;
    connection.execute("DELETE FROM notes", [])?;
    connection.execute("DELETE FROM witnesses", [])?;

    Ok(())
}

pub fn reset_scan(network: &Network, connection: &Connection, height: Option<u32>) -> Result<u32> {
    let activation: u32 = network
        .activation_height(NetworkUpgrade::Sapling)
        .unwrap()
        .into();
    let height = height.unwrap_or(activation + 1) - 1;

    connection.execute("DELETE FROM blcks WHERE height >= ?1", [height])?;
    connection.execute("DELETE FROM txs WHERE height >= ?1", [height])?;
    connection.execute("DELETE FROM notes WHERE height >= ?1", [height])?;
    connection.execute("DELETE FROM witnesses WHERE height >= ?1", [height])?;
    connection.execute("UPDATE notes SET spent = NULL WHERE spent >= ?1", [height])?;

    Ok(height)
}

pub fn rewind_checkpoint(connection: &Connection) -> Result<()> {
    if let Some(checkpoint) = get_sync_height(connection)? {
        rewind(connection, checkpoint - 1)?;
    }
    Ok(())
}

pub fn rewind(connection: &Connection, height: u32) -> Result<()> {
    connection.execute("DELETE FROM blcks WHERE height >= ?1", [height])?;
    connection.execute("DELETE FROM txs WHERE height >= ?1", [height])?;
    connection.execute("DELETE FROM notes WHERE height >= ?1", [height])?;
    connection.execute("DELETE FROM witnesses WHERE height >= ?1", [height])?;
    connection.execute("UPDATE notes SET spent = NULL WHERE spent >= ?1", [height])?;
    Ok(())
}

pub fn get_txid(connection: &Connection, id: u32) -> Result<Vec<u8>> {
    let txid = connection.query_row("SELECT txid FROM txs WHERE id_tx = ?1", [id], |r| {
        r.get::<_, Vec<u8>>(0)
    })?;
    Ok(txid)
}

pub fn store_tx_details(connection: &Connection, id: u32, data: &[u8]) -> Result<()> {
    connection.execute(
        "INSERT INTO txdetails(id_tx, data)
        VALUES (?1, ?2) ON CONFLICT DO NOTHING",
        params![id, data],
    )?;
    Ok(())
}

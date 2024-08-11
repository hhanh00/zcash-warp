use anyhow::Result;
use rusqlite::params;
use crate::{warp::sync::ReceivedNote, Connection};

pub fn store_received_note(connection: &Connection, notes: &[ReceivedNote]) -> Result<()> {
    connection.execute("CREATE TABLE IF NOT EXISTS notes(
        id_note INTEGER PRIMARY KEY,
        account INTEGER NOT NULL,
        position INTEGER NOT NULL,
        height INTEGER NOT NULL,
        output_index INTEGER NOT NULL,
        diversifier BLOB NOT NULL,
        value INTEGER NOT NULL,
        rcm BLOB NOT NULL,
        nf BLOB NOT NULL UNIQUE,
        rho BLOB,
        spent INTEGER)", [])?;
    let mut s = connection.prepare("INSERT INTO notes
    (account, position, height, output_index, diversifier, value, rcm, nf, rho, spent)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)")?;
    for n in notes {
        s.execute(params![n.account, n.position, n.height, n.vout, n.diversifier,
            n.value, n.rcm, n.nf, n.rho, n.spent])?;
    }

    Ok(())
}

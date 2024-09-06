use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _};

pub fn encrypt_db(connection: &Connection, password: &str, new_db_path: &str) -> Result<()> {
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS encrypted_db KEY ?2"),
        [new_db_path, password],
    )?;
    connection.query_row("SELECT sqlcipher_export('encrypted_db')", [], |_row| Ok(()))?;
    connection.execute("DETACH DATABASE encrypted_db", [])?;
    Ok(())
}

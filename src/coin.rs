use anyhow::Result;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use std::{path::Path, time::Duration};
use tonic::transport::{Certificate, ClientTlsConfig};

use zcash_primitives::consensus::Network;

use crate::{cli::CONFIG, lwd::rpc::compact_tx_streamer_client::CompactTxStreamerClient, Client};

type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Clone, Debug)]
pub struct CoinDef {
    pub network: Network,
    pub url: String,
    pub warp: String,
    pub pool: Option<Pool<SqliteConnectionManager>>,
    pub db_password: Option<String>,
}

impl CoinDef {
    pub fn from_network(network: Network) -> Self {
        Self {
            network,
            url: "".to_string(),
            warp: "".to_string(),
            pool: None,
            db_password: None,
        }
    }

    pub fn set_db_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let manager = r2d2_sqlite::SqliteConnectionManager::file(path);
        let pool = r2d2::Pool::new(manager)?;
        self.pool = Some(pool);
        Ok(())
    }

    pub fn set_password(&mut self, password: &str) {
        self.db_password = Some(password.to_string());
    }

    pub fn set_url(&mut self, url: &str) {
        self.url = url.to_string();
    }

    pub fn set_warp(&mut self, warp: &str) {
        self.warp = warp.to_string();
    }

    pub fn connection(&self) -> Result<Connection> {
        let pool = self.pool.as_ref().expect("No db path set");
        let connection = pool.get().unwrap();
        if let Some(ref password) = self.db_password {
            let _ = connection
                .query_row(&format!("PRAGMA key = '{}'", password), [], |_| Ok(()))
                .optional();
        }
        let _ = connection.busy_timeout(Duration::from_secs(60));
        let c = connection
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
                row.get::<_, u32>(0)
            })
            .optional()?;
        if c.is_none() {
            anyhow::bail!("Could not open db (invalid password?)")
        }
        Ok(connection)
    }

    pub async fn connect_lwd(&self) -> Result<Client> {
        connect_lwd(&self.url).await
    }
}

pub async fn connect_lwd(url: &str) -> Result<Client> {
    let mut channel = tonic::transport::Channel::from_shared(url.to_string())?;
    if url.starts_with("https") {
        let pem = include_bytes!("ca.pem");
        let ca = Certificate::from_pem(pem);
        let tls = ClientTlsConfig::new().ca_certificate(ca);
        channel = channel.tls_config(tls)?;
    }
    let client = CompactTxStreamerClient::connect(channel).await?;
    Ok(client)
}

pub fn init_coin() -> Result<()> {
    let mut zec = COINS[0].lock();
    zec.set_db_path(&CONFIG.db_path)?;
    zec.set_url(&CONFIG.lwd_url);
    zec.set_warp(&CONFIG.warp_url);
    Ok(())
}

lazy_static! {
    pub static ref COINS: [Mutex<CoinDef>; 1] = [
        Mutex::new(CoinDef::from_network(Network::MainNetwork)),
        // Mutex::new(CoinDef::from_network(Network::YCashMainNetwork)),
    ];
}

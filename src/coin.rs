use anyhow::Result;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use std::time::Duration;
use tonic::transport::{Certificate, ClientTlsConfig};

use crate::network::Network;

use crate::{data::fb::ConfigT, lwd::rpc::compact_tx_streamer_client::CompactTxStreamerClient, Client};

type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Clone, Debug)]
pub struct CoinDef {
    pub coin: u8,
    pub network: Network,
    pub pool: Option<Pool<SqliteConnectionManager>>,
    pub db_password: Option<String>,
    pub config: ConfigT,
}

impl CoinDef {
    pub fn from_network(coin: u8, network: Network) -> Self {
        Self {
            coin,
            network,
            pool: None,
            db_password: None,
            config: ConfigT::default(),
        }
    }

    pub fn set_config(&mut self, config: &ConfigT) -> Result<()> {
        self.config.merge(config);
        if let Some(path) = self.config.db_path.as_ref() {
            tracing::info!("Setting pool");
            let manager = r2d2_sqlite::SqliteConnectionManager::file(path);
            let pool = r2d2::Pool::new(manager)?;
            self.pool = Some(pool);
        }
        Ok(())
    }

    pub fn set_password(&mut self, password: &str) {
        self.db_password = Some(password.to_string());
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
        connect_lwd(self.config.lwd_url.as_ref().unwrap()).await
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

pub fn cli_init_coin(config: &ConfigT) -> Result<()> {
    let mut zec = COINS[0].lock();
    zec.set_config(config)?;
    Ok(())
}

lazy_static! {
    pub static ref COINS: [Mutex<CoinDef>; 1] = [
        Mutex::new(CoinDef::from_network(0, Network::Main)),
        // Mutex::new(CoinDef::from_network(Network::YCashMainNetwork)),
    ];
}

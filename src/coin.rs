use anyhow::Result;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint};
use tower::discover::Change;

use crate::network::Network;

use crate::{
    data::fb::ConfigT, lwd::rpc::compact_tx_streamer_client::CompactTxStreamerClient, Client,
};

type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Clone, Debug)]
pub struct CoinDef {
    pub coin: u8,
    pub network: Network,
    pub pool: Option<Pool<SqliteConnectionManager>>,
    pub db_password: Option<String>,
    pub channel: Option<Channel>,
    pub config: ConfigT,
    pub runtime: TokioRuntime, // this runtime needs to live for the whole duration of the app
}

impl Drop for CoinDef {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.0.take() {
            if let Ok(runtime) = Arc::try_unwrap(runtime) {
                runtime.shutdown_background();
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct TokioRuntime(Option<Arc<Runtime>>);

impl CoinDef {
    pub fn from_network(coin: u8, network: Network) -> Self {
        Self {
            coin,
            network,
            pool: None,
            db_password: None,
            channel: None,
            config: ConfigT::default(),
            runtime: TokioRuntime(Some(Arc::new(Runtime::new().unwrap()))),
        }
    }

    pub fn set_config(&mut self, config: &ConfigT) -> Result<()> {
        self.config.merge(config);
        if let Some(servers) = self.config.servers.as_ref() {
            let pem = include_bytes!("ca.pem");
            let ca = Certificate::from_pem(pem);
            let tls = ClientTlsConfig::new().ca_certificate(ca);
            let endpoints = servers
                .iter()
                .map(|s| {
                    let ep = Endpoint::from_str(&s).unwrap();
                    ep.tls_config(tls.clone()).unwrap()
                })
                .collect::<Vec<_>>();
            tracing::info!("servers {:?}", endpoints);
            let (channel, tx) = Channel::balance_channel_with_executor(16, self.runtime.clone());
            endpoints.into_iter().for_each(|endpoint| {
                tx.try_send(Change::Insert(endpoint.uri().clone(), endpoint))
                    .unwrap();
            });
            self.channel = Some(channel);
        }
        Ok(())
    }

    pub fn set_path_password(&mut self, path: &str, password: &str) -> Result<()> {
        self.db_password = Some(password.to_string());
        tracing::info!("Setting pool");
        let manager = r2d2_sqlite::SqliteConnectionManager::file(path);
        let pool = r2d2::Pool::new(manager)?;
        self.pool = Some(pool);
        Ok(())
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

    pub fn connect_lwd(&self) -> Result<Client> {
        let channel = self
            .channel
            .as_ref()
            .ok_or(anyhow::anyhow!("No connection channel"))?;
        let client = CompactTxStreamerClient::new(channel.clone());
        Ok(client)
    }
}

impl<F> hyper::rt::Executor<F> for TokioRuntime
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, future: F) {
        self.0.as_ref().unwrap().spawn(future);
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
    tracing::info!("{url}");
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
        #[cfg(feature = "regtest")]
        Mutex::new(CoinDef::from_network(0, Network::Regtest(crate::network::_regtest()))),
        #[cfg(not(feature = "regtest"))]
        Mutex::new(CoinDef::from_network(0, Network::Main)),
        // Mutex::new(CoinDef::from_network(Network::YCashMainNetwork)),
    ];
}

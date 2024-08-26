use lwd::rpc::compact_tx_streamer_client::CompactTxStreamerClient;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use tonic::transport::Channel;

#[path = "./generated/data_generated.rs"]
mod data;

pub mod account;
mod coin;
pub mod db;
mod keys;
pub mod lwd;
pub mod pay;
pub mod txdetails;
pub mod messages;
pub mod types;
mod utils;
pub mod warp;
pub mod cli;

pub mod tests;

pub type Client = CompactTxStreamerClient<Channel>;
pub type PooledSQLConnection = PooledConnection<SqliteConnectionManager>;
pub type Hash = [u8; 32];

// pub use coin::{CoinDef, COINS};
// pub use keys::{generate_random_mnemonic_phrase, TSKStore};
pub use cli::cli_main;

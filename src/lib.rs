use lwd::rpc::compact_tx_streamer_client::CompactTxStreamerClient;
use tonic::transport::Channel;

#[path="./generated/data_generated.rs"]
mod data;

mod utils;
mod coin;
mod keys;
pub mod types;
pub mod lwd;
pub mod db;
pub mod warp;
pub mod pay;

pub type Client = CompactTxStreamerClient<Channel>;
pub type Hash = [u8; 32];

pub use coin::{CoinDef, COINS};
pub use keys::generate_random_mnemonic_phrase;

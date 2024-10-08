use crate::{
    coin::CoinDef,
    data::fb::{Config, ConfigT},
    Hash,
};
use anyhow::Result;
use tracing_subscriber::{fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};

use crate::{
    coin::COINS,
    ffi::{map_result, CParam, CResult},
};
use warp_macros::c_export;

pub mod chain;
pub mod data_split;
pub mod db;
pub mod keys;
pub mod messages;
pub mod pay;
pub mod tx;
pub mod ua;
pub mod uri;
pub mod zip_db;

pub fn init_tracing() {
    let s = tracing_subscriber::registry();
    let _ = s
        .with(fmt::layer().with_ansi(false).compact())
        .with(EnvFilter::from_default_env())
        .try_init();
    tracing::info!("Tracing initialized");
}

#[no_mangle]
pub extern "C" fn c_setup() {
    init_tracing();
}

#[macro_export]
macro_rules! fb_unwrap {
    ($v: expr) => {
        $v.as_ref().unwrap()
    };
}

#[macro_export]
macro_rules! fb_vec_to_bytes {
    ($vs: ident, $T: ident) => {{
        let mut builder = FlatBufferBuilder::new();
        let mut os = vec![];
        for v in $vs.iter() {
            let o = v.pack(&mut builder);
            builder.push(o);
            os.push(o);
        }
        builder.start_vector::<WIPOffset<$T>>($vs.len());
        for o in os {
            builder.push(o);
        }
        let o = builder.end_vector::<WIPOffset<$T>>($vs.len());
        builder.finish(o, None);
        let data = builder.finished_data();
        Ok::<_, anyhow::Error>(data.to_vec())
    }};
}

pub fn to_txid_str(txid: &Hash) -> String {
    let mut txid = txid.clone();
    txid.reverse();
    hex::encode(&txid)
}

#[c_export]
pub fn configure(coin: &CoinDef, config: &ConfigT) -> Result<()> {
    tracing::info!("{:?}", config);
    let mut coin_def = COINS[coin.coin as usize].lock();
    coin_def.set_config(config)?;
    Ok(())
}

impl ConfigT {
    pub fn merge(&mut self, other: &ConfigT) {
        if other.lwd_url.is_some() {
            self.lwd_url = other.lwd_url.clone();
        }
        if other.warp_url.is_some() {
            self.warp_url = other.warp_url.clone();
        }
        if other.confirmations > 0 {
            self.confirmations = other.confirmations;
        }
        if other.warp_end_height > 0 {
            self.warp_end_height = other.warp_end_height;
        }
        if other.regtest {
            self.regtest = other.regtest;
        }
    }
}

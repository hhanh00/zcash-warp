use crate::{
    coin::CoinDef,
    data::fb::{Config, ConfigT},
    Hash,
};
use anyhow::Result;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
    EnvFilter, Layer, Registry,
};

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

type BoxedLayer<S> = Box<dyn Layer<S> + Send + Sync + 'static>;

fn default_layer<S>() -> BoxedLayer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fmt::layer()
        .with_ansi(false)
        .with_span_events(FmtSpan::ACTIVE)
        .compact()
        .boxed()
}

fn env_layer<S>() -> BoxedLayer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    EnvFilter::from_default_env().boxed()
}

#[cfg(target_os = "android")]
fn android_layer<S>() -> Option<BoxedLayer<S>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let android_layer = paranoid_android::layer(env!("CARGO_PKG_NAME"))
        .with_filter(tracing_subscriber::filter::LevelFilter::INFO);
    Some(android_layer.boxed())
}

#[cfg(not(target_os = "android"))]
fn android_layer<S>() -> Option<BoxedLayer<S>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    None
}

#[cfg(target_os = "ios")]
fn ios_layer<S>() -> Option<BoxedLayer<S>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let layer = tracing_oslog::OsLogger::new("moe.absolucy.test", "default");
    Some(layer.boxed())
}

#[cfg(not(target_os = "ios"))]
fn ios_layer<S>() -> Option<BoxedLayer<S>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    None
}

pub fn init_tracing() {
    let _ = Registry::default()
        .with(default_layer())
        .with(env_layer())
        .with(android_layer())
        .with(ios_layer())
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
pub async fn configure(coin: &CoinDef, config: &ConfigT) -> Result<()> {
    tracing::info!("{:?}", config);
    let mut coin_def = COINS[coin.coin as usize].lock();
    coin_def.set_config(config)?;
    Ok(())
}

impl ConfigT {
    pub fn merge(&mut self, other: &ConfigT) {
        if other.servers.is_some() {
            self.servers = other.servers.clone();
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

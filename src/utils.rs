use crate::{cli::init_config, coin::{init_coin, COINS}, data::fb::AppConfig, ffi::{CParam, CResult}, Hash};
use tracing_subscriber::{fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};

pub mod db;
pub mod ua;
pub mod uri;
pub mod chain;
pub mod messages;
pub mod zip_db;
pub mod data_split;
pub mod pay;
pub mod tx;

pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(false).compact())
        .with(EnvFilter::from_default_env())
        .init();
}

#[no_mangle]
pub extern "C" fn c_setup() {
    init_tracing();
    init_config();
    init_coin().unwrap();
}

#[macro_export]
macro_rules! fb_to_bytes {
    ($v: ident) => {{
        let mut builder = FlatBufferBuilder::new();
        let backup_bytes = $v.pack(&mut builder);
        builder.finish(backup_bytes, None);
        Ok::<_, anyhow::Error>(builder.finished_data().to_vec())
    }};
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

#[no_mangle]
pub extern "C" fn c_configure(coin: u8, config: CParam) -> CResult<u8> {
    let config_bytes = config.value;
    let config_len = config.len as usize;
    let config = unsafe {
        Vec::from_raw_parts(config_bytes, config_len, config_len)
    };
    let config = flatbuffers::root::<AppConfig>(&config).unwrap();
    let config = config.unpack();
    tracing::info!("{:?}", config);
    let mut coin_def = COINS[coin as usize].lock();
    if let Some(db) = config.db { 
        coin_def.set_db_path(&db).unwrap();
    }
    if let Some(url) = config.url { 
        coin_def.set_url(&url);
    }
    if let Some(warp) = config.warp { 
        coin_def.set_warp(&warp);
    }
    CResult::new(0)
}

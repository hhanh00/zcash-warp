use tracing_subscriber::{fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};
use zcash_warp::cli_main;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(false).compact())
        .with(EnvFilter::from_default_env())
        .init();

    cli_main()?;
    Ok(())
}

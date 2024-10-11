use zcash_warp::{cli::init_config, cli_main, utils::init_tracing};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = init_config();
    cli_main(&config)?;
    Ok(())
}

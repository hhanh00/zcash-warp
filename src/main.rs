use zcash_warp::{cli::init_config, cli_main, utils::init_tracing};

fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = init_config();
    cli_main(&config)?;
    Ok(())
}

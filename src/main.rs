use zcash_warp::{cli_main, utils::init_tracing};

fn main() -> anyhow::Result<()> {
    init_tracing();

    cli_main()?;
    Ok(())
}

use zcash_warp::cli_main;

fn main() -> anyhow::Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    cli_main()?;
    Ok(())
}

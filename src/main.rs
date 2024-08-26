use zcash_warp::cli_main;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // account_tests()?;
    cli_main().await?;
    // let _tx = test_payment().await?;
    Ok(())
}

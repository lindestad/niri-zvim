use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args();
    let _program = args.next();
    match (args.next().as_deref(), args.next()) {
        (None, None) => {}
        (Some("-V" | "--version"), None) => {
            println!("niri-zvimd {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        (Some("-h" | "--help"), None) => {
            println!("usage: niri-zvimd");
            return Ok(());
        }
        _ => anyhow::bail!("usage: niri-zvimd"),
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();
    niri_zvim::run_daemon().await
}

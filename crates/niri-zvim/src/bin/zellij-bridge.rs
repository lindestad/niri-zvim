use niri_zvim::run_forwarded_bridge;

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => run_forwarded_bridge(),
        ["-V" | "--version"] => {
            println!("niri-zvim-zellij-bridge {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["-h" | "--help"] => {
            println!("usage: niri-zvim-zellij-bridge");
            Ok(())
        }
        _ => anyhow::bail!("usage: niri-zvim-zellij-bridge"),
    }
}

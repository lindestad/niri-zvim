use std::{path::Path, process::Command};

use anyhow::Context;
use niri_zvim::{BindingInstall, install_hjkl_bindings, restore_hjkl_bindings};

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["install-hjkl", "--config", config, "--state-dir", state] => {
            validate(Path::new(config))?;
            match install_hjkl_bindings(Path::new(config), Path::new(state))? {
                BindingInstall::AlreadyConfigured => {
                    println!("Mod+H/J/K/L already run niri-zvim; no files changed");
                }
                BindingInstall::AlreadyManaged { backup } => {
                    println!(
                        "Mod+H/J/K/L are already managed; backup: {}",
                        backup.display()
                    );
                }
                BindingInstall::Installed { files, backup } => {
                    if let Err(error) = validate(Path::new(config)) {
                        let _ = restore_hjkl_bindings(Path::new(state));
                        return Err(error).context("restored the original bindings");
                    }
                    for file in files {
                        println!("updated {}", file.display());
                    }
                    println!("binding backup: {}", backup.display());
                }
            }
            Ok(())
        }
        ["restore-hjkl", "--state-dir", state] => {
            let restored = restore_hjkl_bindings(Path::new(state))?;
            if restored.backup.is_none() {
                println!("no installer-managed Niri bindings found");
                return Ok(());
            }
            for file in restored.restored {
                println!("restored {}", file.display());
            }
            for file in &restored.preserved {
                eprintln!(
                    "preserved {} because it changed after installation",
                    file.display()
                );
            }
            if !restored.preserved.is_empty() {
                let backup = restored.backup.expect("checked above");
                anyhow::bail!(
                    "some Niri bindings were not restored automatically; originals remain in {}",
                    backup.display()
                );
            }
            Ok(())
        }
        ["-V" | "--version"] => {
            println!("niri-zvim-configure {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["-h" | "--help"] => {
            println!("{}", usage());
            Ok(())
        }
        _ => anyhow::bail!(usage()),
    }
}

fn validate(config: &Path) -> anyhow::Result<()> {
    let output = Command::new("niri")
        .args(["validate", "--config"])
        .arg(config)
        .output()
        .context("could not run niri validate")?;
    if output.status.success() {
        return Ok(());
    }
    let mut message = String::from_utf8_lossy(&output.stdout).into_owned();
    message.push_str(&String::from_utf8_lossy(&output.stderr));
    anyhow::bail!("niri rejected {}: {}", config.display(), message.trim())
}

fn usage() -> &'static str {
    "usage: niri-zvim-configure <install-hjkl --config PATH --state-dir PATH|restore-hjkl --state-dir PATH>"
}

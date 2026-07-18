use std::{ffi::OsString, path::PathBuf};

use anyhow::Context;

pub fn socket_path() -> anyhow::Result<PathBuf> {
    resolve_socket_path(
        std::env::var_os("NIRI_ZVIM_SOCKET"),
        std::env::var_os("XDG_RUNTIME_DIR"),
    )
}

fn resolve_socket_path(
    explicit: Option<OsString>,
    runtime: Option<OsString>,
) -> anyhow::Result<PathBuf> {
    let path = explicit
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            runtime
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(|path| path.join("niri-zvim.sock"))
        })
        .context("XDG_RUNTIME_DIR or NIRI_ZVIM_SOCKET is required")?;
    anyhow::ensure!(path.is_absolute(), "socket path must be absolute");
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_socket_path_wins() {
        assert_eq!(
            resolve_socket_path(Some("/custom/niri.sock".into()), Some("/run/user/1".into()))
                .unwrap(),
            PathBuf::from("/custom/niri.sock")
        );
    }

    #[test]
    fn runtime_directory_contains_the_default_socket() {
        assert_eq!(
            resolve_socket_path(None, Some("/run/user/1".into())).unwrap(),
            PathBuf::from("/run/user/1/niri-zvim.sock")
        );
    }

    #[test]
    fn socket_path_requires_an_absolute_private_location() {
        assert!(resolve_socket_path(None, None).is_err());
        assert!(resolve_socket_path(Some("relative.sock".into()), None).is_err());
    }
}

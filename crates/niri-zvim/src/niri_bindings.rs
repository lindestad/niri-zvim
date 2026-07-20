use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

const MANIFEST_VERSION: u8 = 1;
const MANIFEST_NAME: &str = "niri-bindings.json";
const DIRECTIONS: [(&str, &str); 4] = [
    ("Mod+H", "left"),
    ("Mod+J", "down"),
    ("Mod+K", "up"),
    ("Mod+L", "right"),
];

#[derive(Debug, PartialEq, Eq)]
pub enum BindingInstall {
    AlreadyConfigured,
    AlreadyManaged {
        backup: PathBuf,
    },
    Installed {
        files: Vec<PathBuf>,
        backup: PathBuf,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct BindingRestore {
    pub restored: Vec<PathBuf>,
    pub preserved: Vec<PathBuf>,
    pub backup: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BindingManifest {
    version: u8,
    config: PathBuf,
    backup: PathBuf,
    files: Vec<ManagedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManagedFile {
    logical_path: PathBuf,
    target_path: PathBuf,
    original: PathBuf,
    installed: PathBuf,
}

#[derive(Debug)]
struct SourceFile {
    logical_path: PathBuf,
    target_path: PathBuf,
    contents: String,
}

#[derive(Debug, Clone, Copy)]
struct BindingSpan {
    body_start: usize,
    body_end: usize,
    direction: &'static str,
}

pub fn install_hjkl_bindings(config: &Path, state_dir: &Path) -> anyhow::Result<BindingInstall> {
    let manifest_path = state_dir.join(MANIFEST_NAME);
    if manifest_path.exists() {
        let manifest = read_manifest(&manifest_path)?;
        return Ok(BindingInstall::AlreadyManaged {
            backup: manifest.backup,
        });
    }

    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("HOME is required to resolve Niri includes")?;
    let mut sources = discover_sources(config, &home)?;
    let mut found: BTreeMap<&'static str, usize> = DIRECTIONS
        .iter()
        .map(|(_, direction)| (*direction, 0))
        .collect();
    let mut changed = Vec::new();

    for source in &mut sources {
        let spans = find_binding_spans(&source.contents)?;
        let mut replacements = Vec::new();
        for span in spans {
            *found.entry(span.direction).or_default() += 1;
            let expected = format!("spawn \"niri-zvim\" \"{}\";", span.direction);
            if source.contents[span.body_start..span.body_end].trim() != expected {
                replacements.push((span.body_start, span.body_end, format!(" {expected} ")));
            }
        }
        if !replacements.is_empty() {
            replacements.sort_by_key(|(start, _, _)| *start);
            for (start, end, replacement) in replacements.into_iter().rev() {
                source.contents.replace_range(start..end, &replacement);
            }
            changed.push(source.logical_path.clone());
        }
    }

    let missing: Vec<_> = DIRECTIONS
        .iter()
        .filter_map(|(binding, direction)| (found[direction] == 0).then_some(*binding))
        .collect();
    if !missing.is_empty() {
        bail!(
            "could not find {} in the Niri config or its imports; no files were changed",
            missing.join(", ")
        );
    }
    if changed.is_empty() {
        return Ok(BindingInstall::AlreadyConfigured);
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs();
    let backup = state_dir
        .join("backups")
        .join(format!("niri-bindings-{stamp}-{}", std::process::id()));
    fs::create_dir_all(&backup)
        .with_context(|| format!("could not create backup directory {}", backup.display()))?;

    let mut managed = Vec::new();
    for (index, source) in sources
        .iter()
        .filter(|source| changed.contains(&source.logical_path))
        .enumerate()
    {
        let original = backup.join(format!("{index}.original.kdl"));
        let installed = backup.join(format!("{index}.installed.kdl"));
        let original_contents = fs::read(&source.target_path).with_context(|| {
            format!("could not read {} for backup", source.target_path.display())
        })?;
        fs::write(&original, original_contents)
            .with_context(|| format!("could not write backup {}", original.display()))?;
        fs::write(&installed, source.contents.as_bytes())
            .with_context(|| format!("could not write snapshot {}", installed.display()))?;
        managed.push(ManagedFile {
            logical_path: source.logical_path.clone(),
            target_path: source.target_path.clone(),
            original,
            installed,
        });
    }

    let mut written: Vec<&ManagedFile> = Vec::new();
    for file in &managed {
        let contents = fs::read(&file.installed)
            .with_context(|| format!("could not read {}", file.installed.display()))?;
        if let Err(error) = atomic_write(&file.target_path, &contents) {
            for previous in written.into_iter().rev() {
                if let Ok(original) = fs::read(&previous.original) {
                    let _ = atomic_write(&previous.target_path, &original);
                }
            }
            return Err(error)
                .with_context(|| format!("could not update {}", file.logical_path.display()));
        }
        written.push(file);
    }

    let manifest = BindingManifest {
        version: MANIFEST_VERSION,
        config: config.to_path_buf(),
        backup: backup.clone(),
        files: managed,
    };
    fs::create_dir_all(state_dir)
        .with_context(|| format!("could not create state directory {}", state_dir.display()))?;
    if let Err(error) = write_manifest(&manifest_path, &manifest) {
        for file in &manifest.files {
            if let Ok(original) = fs::read(&file.original) {
                let _ = atomic_write(&file.target_path, &original);
            }
        }
        return Err(error);
    }

    Ok(BindingInstall::Installed {
        files: changed,
        backup,
    })
}

pub fn restore_hjkl_bindings(state_dir: &Path) -> anyhow::Result<BindingRestore> {
    let manifest_path = state_dir.join(MANIFEST_NAME);
    if !manifest_path.exists() {
        return Ok(BindingRestore {
            restored: Vec::new(),
            preserved: Vec::new(),
            backup: None,
        });
    }
    let manifest = read_manifest(&manifest_path)?;
    let mut restored = Vec::new();
    let mut preserved = Vec::new();

    for file in &manifest.files {
        let current_target = write_target(&file.logical_path)?;
        let current = fs::read(&current_target).with_context(|| {
            format!(
                "could not read current Niri config {}",
                current_target.display()
            )
        })?;
        let installed = fs::read(&file.installed)
            .with_context(|| format!("could not read {}", file.installed.display()))?;
        if current_target != file.target_path || current != installed {
            preserved.push(file.logical_path.clone());
            continue;
        }
        let original = fs::read(&file.original)
            .with_context(|| format!("could not read backup {}", file.original.display()))?;
        atomic_write(&file.target_path, &original)
            .with_context(|| format!("could not restore {}", file.logical_path.display()))?;
        restored.push(file.logical_path.clone());
    }

    if preserved.is_empty() {
        fs::remove_file(&manifest_path)
            .with_context(|| format!("could not remove {}", manifest_path.display()))?;
    }

    Ok(BindingRestore {
        restored,
        preserved,
        backup: Some(manifest.backup),
    })
}

pub fn binding_manifest_path(state_dir: &Path) -> PathBuf {
    state_dir.join(MANIFEST_NAME)
}

fn read_manifest(path: &Path) -> anyhow::Result<BindingManifest> {
    let contents = fs::read(path)
        .with_context(|| format!("could not read binding manifest {}", path.display()))?;
    let manifest: BindingManifest = serde_json::from_slice(&contents)
        .with_context(|| format!("could not parse binding manifest {}", path.display()))?;
    if manifest.version != MANIFEST_VERSION {
        bail!(
            "binding manifest {} has version {}, expected {}",
            path.display(),
            manifest.version,
            MANIFEST_VERSION
        );
    }
    Ok(manifest)
}

fn write_manifest(path: &Path, manifest: &BindingManifest) -> anyhow::Result<()> {
    let mut contents = serde_json::to_vec_pretty(manifest)?;
    contents.push(b'\n');
    atomic_write(path, &contents)
        .with_context(|| format!("could not write binding manifest {}", path.display()))
}

fn discover_sources(config: &Path, home: &Path) -> anyhow::Result<Vec<SourceFile>> {
    let mut sources = Vec::new();
    let mut visited = HashSet::new();
    discover_source(config, home, 0, &mut visited, &mut sources)?;
    Ok(sources)
}

fn discover_source(
    logical_path: &Path,
    home: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    sources: &mut Vec<SourceFile>,
) -> anyhow::Result<()> {
    if depth > 32 {
        bail!("Niri includes exceed the supported depth of 32");
    }
    let target_path = write_target(logical_path)?;
    if !visited.insert(target_path.clone()) {
        return Ok(());
    }
    let contents = fs::read_to_string(logical_path)
        .with_context(|| format!("could not read Niri config {}", logical_path.display()))?;
    let includes = find_includes(&contents)?;
    sources.push(SourceFile {
        logical_path: logical_path.to_path_buf(),
        target_path,
        contents,
    });

    for include in includes {
        let path = expand_include(logical_path, home, &include.path);
        if !path.exists() {
            if include.optional {
                continue;
            }
            bail!(
                "required Niri include {} from {} is missing",
                path.display(),
                logical_path.display()
            );
        }
        discover_source(&path, home, depth + 1, visited, sources)?;
    }
    Ok(())
}

#[derive(Debug)]
struct Include {
    path: String,
    optional: bool,
}

fn find_includes(contents: &str) -> anyhow::Result<Vec<Include>> {
    let mask = syntax_mask(contents)?;
    let mut includes = Vec::new();
    let mut depth = 0usize;
    let mut offset = 0usize;
    for line in mask.split_inclusive(|byte| *byte == b'\n') {
        let line_end = offset + line.len();
        if depth == 0 {
            let start = offset
                + line
                    .iter()
                    .position(|byte| !byte.is_ascii_whitespace())
                    .unwrap_or(line.len());
            if token_at(&mask, start, b"include") {
                let raw = &contents[start..line_end];
                let path = first_kdl_string(raw)
                    .with_context(|| format!("include at byte {start} has no string path"))?;
                let compact: String = raw
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect();
                includes.push(Include {
                    path,
                    optional: compact.contains("optional=true"),
                });
            }
        }
        update_depth(line, &mut depth)?;
        offset = line_end;
    }
    Ok(includes)
}

fn expand_include(source: &Path, home: &Path, include: &str) -> PathBuf {
    if include == "~" {
        return home.to_path_buf();
    }
    if let Some(relative) = include.strip_prefix("~/") {
        return home.join(relative);
    }
    let include = Path::new(include);
    if include.is_absolute() {
        include.to_path_buf()
    } else {
        source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(include)
    }
}

fn find_binding_spans(contents: &str) -> anyhow::Result<Vec<BindingSpan>> {
    let mask = syntax_mask(contents)?;
    let blocks = find_binds_blocks(&mask)?;
    let mut spans = Vec::new();
    for (open, close) in blocks {
        spans.extend(find_bindings_in_block(&mask, open + 1, close)?);
    }
    Ok(spans)
}

fn find_binds_blocks(mask: &[u8]) -> anyhow::Result<Vec<(usize, usize)>> {
    let mut blocks = Vec::new();
    let mut depth = 0usize;
    let mut offset = 0usize;
    for line in mask.split_inclusive(|byte| *byte == b'\n') {
        let line_end = offset + line.len();
        if depth == 0 {
            let relative = line.iter().position(|byte| !byte.is_ascii_whitespace());
            if let Some(relative) = relative {
                let start = offset + relative;
                if token_at(mask, start, b"binds") {
                    let open = mask[start..line_end]
                        .iter()
                        .position(|byte| *byte == b'{')
                        .map(|index| start + index)
                        .context("a top-level binds node must open its block on the same line")?;
                    blocks.push((open, matching_brace(mask, open)?));
                }
            }
        }
        update_depth(line, &mut depth)?;
        offset = line_end;
    }
    Ok(blocks)
}

fn find_bindings_in_block(
    mask: &[u8],
    content_start: usize,
    content_end: usize,
) -> anyhow::Result<Vec<BindingSpan>> {
    let mut spans = Vec::new();
    let mut depth = 0usize;
    let mut offset = content_start;
    for line in mask[content_start..content_end].split_inclusive(|byte| *byte == b'\n') {
        let line_end = offset + line.len();
        if depth == 0
            && let Some(relative) = line.iter().position(|byte| !byte.is_ascii_whitespace())
        {
            let start = offset + relative;
            if !mask[start..line_end].starts_with(b"/-") {
                for (binding, direction) in DIRECTIONS {
                    if token_at_case_insensitive(mask, start, binding.as_bytes()) {
                        let open = mask[start..line_end]
                            .iter()
                            .position(|byte| *byte == b'{')
                            .map(|index| start + index)
                            .with_context(|| {
                                format!("{binding} must open its action block on the same line")
                            })?;
                        let close = matching_brace(mask, open)?;
                        spans.push(BindingSpan {
                            body_start: open + 1,
                            body_end: close,
                            direction,
                        });
                    }
                }
            }
        }
        update_depth(line, &mut depth)?;
        offset = line_end;
    }
    Ok(spans)
}

fn token_at(contents: &[u8], start: usize, token: &[u8]) -> bool {
    contents.get(start..start + token.len()) == Some(token)
        && contents
            .get(start + token.len())
            .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'{' | b';'))
}

fn token_at_case_insensitive(contents: &[u8], start: usize, token: &[u8]) -> bool {
    contents
        .get(start..start + token.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(token))
        && contents
            .get(start + token.len())
            .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'{' | b';'))
}

fn matching_brace(mask: &[u8], open: usize) -> anyhow::Result<usize> {
    let mut depth = 0usize;
    for (relative, byte) in mask[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1).context("unmatched closing brace")?;
                if depth == 0 {
                    return Ok(open + relative);
                }
            }
            _ => {}
        }
    }
    bail!("unclosed action block")
}

fn update_depth(bytes: &[u8], depth: &mut usize) -> anyhow::Result<()> {
    for byte in bytes {
        match byte {
            b'{' => *depth += 1,
            b'}' => *depth = depth.checked_sub(1).context("unmatched closing brace")?,
            _ => {}
        }
    }
    Ok(())
}

fn first_kdl_string(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' if !escaped => {
                        return serde_json::from_str(&input[start..=index]).ok();
                    }
                    b'\\' if !escaped => escaped = true,
                    _ => escaped = false,
                }
                index += 1;
            }
            return None;
        }
        if bytes[index] == b'r' {
            let mut quote = index + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if bytes.get(quote) == Some(&b'"') {
                let hashes = quote - index - 1;
                let body_start = quote + 1;
                let mut end = body_start;
                while end < bytes.len() {
                    if bytes[end] == b'"'
                        && bytes.get(end + 1..end + 1 + hashes) == Some(&vec![b'#'; hashes][..])
                    {
                        return Some(input[body_start..end].to_owned());
                    }
                    end += 1;
                }
                return None;
            }
        }
        index += 1;
    }
    None
}

fn syntax_mask(input: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut mask = bytes.to_vec();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let end = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |relative| index + relative);
            mask[index..end].fill(b' ');
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            let start = index;
            index += 2;
            let mut depth = 1usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            if depth != 0 {
                bail!("unclosed block comment in Niri config");
            }
            for byte in &mut mask[start..index] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            continue;
        }
        if bytes[index] == b'r' {
            let mut quote = index + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if bytes.get(quote) == Some(&b'"') {
                let start = index;
                let hashes = quote - index - 1;
                index = quote + 1;
                loop {
                    if index >= bytes.len() {
                        bail!("unclosed raw string in Niri config");
                    }
                    if bytes[index] == b'"'
                        && bytes.get(index + 1..index + 1 + hashes) == Some(&vec![b'#'; hashes][..])
                    {
                        index += 1 + hashes;
                        break;
                    }
                    index += 1;
                }
                for byte in &mut mask[start..index] {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
                continue;
            }
        }
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                if bytes[index] == b'"' && !escaped {
                    index += 1;
                    break;
                }
                escaped = bytes[index] == b'\\' && !escaped;
                index += 1;
            }
            if index > bytes.len() || bytes.get(index.saturating_sub(1)) != Some(&b'"') {
                bail!("unclosed string in Niri config");
            }
            for byte in &mut mask[start..index] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            continue;
        }
        index += 1;
    }
    Ok(mask)
}

fn write_target(path: &Path) -> anyhow::Result<PathBuf> {
    fs::canonicalize(path)
        .with_context(|| format!("could not resolve Niri config path {}", path.display()))
}

fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)?;
    let mode = fs::metadata(path)
        .map(|metadata| metadata.permissions().mode())
        .unwrap_or(0o600);
    let temporary = parent.join(format!(
        ".niri-zvim-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| -> anyhow::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn bindings(left: &str, down: &str, up: &str, right: &str) -> String {
        format!(
            "binds {{\n    Mod+H {{ {left}; }}\n    Mod+J {{ {down}; }}\n    Mod+K {{ {up}; }}\n    Mod+L {{ {right}; }}\n}}\n"
        )
    }

    #[test]
    fn edits_imported_bindings_without_replacing_symlinks_and_restores_them() {
        let temp = tempfile::tempdir().unwrap();
        let active = temp.path().join("active");
        let managed = temp.path().join("managed");
        let state = temp.path().join("state");
        fs::create_dir_all(&active).unwrap();
        fs::create_dir_all(&managed).unwrap();
        let config_target = managed.join("config.kdl");
        let bindings_target = managed.join("keys.kdl");
        fs::write(&config_target, "include \"keys.kdl\"\n").unwrap();
        let original = bindings(
            "focus-column-left",
            "focus-window-down",
            "focus-window-up",
            "focus-column-right",
        );
        fs::write(&bindings_target, &original).unwrap();
        symlink(&config_target, active.join("config.kdl")).unwrap();
        symlink(&bindings_target, active.join("keys.kdl")).unwrap();

        let installed = install_hjkl_bindings(&active.join("config.kdl"), &state).unwrap();
        assert!(matches!(installed, BindingInstall::Installed { .. }));
        assert!(active.join("config.kdl").is_symlink());
        assert!(active.join("keys.kdl").is_symlink());
        assert_eq!(
            fs::read_to_string(&config_target).unwrap(),
            "include \"keys.kdl\"\n"
        );
        let edited = fs::read_to_string(&bindings_target).unwrap();
        for (_, direction) in DIRECTIONS {
            assert!(edited.contains(&format!("spawn \"niri-zvim\" \"{direction}\";")));
        }

        let restored = restore_hjkl_bindings(&state).unwrap();
        assert_eq!(restored.restored, vec![active.join("keys.kdl")]);
        assert!(restored.preserved.is_empty());
        assert_eq!(fs::read_to_string(&bindings_target).unwrap(), original);
        assert!(active.join("keys.kdl").is_symlink());
    }

    #[test]
    fn recursively_finds_split_bindings_and_preserves_properties() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.kdl");
        let first = temp.path().join("first.kdl");
        let second = temp.path().join("second.kdl");
        fs::write(
            &config,
            "include \"first.kdl\"\ninclude optional=true \"missing.kdl\"\n",
        )
        .unwrap();
        fs::write(
            &first,
            "include \"second.kdl\"\nbinds {\n Mod+H repeat=false { focus-column-left; }\n Mod+L { focus-column-right; }\n}\n",
        )
        .unwrap();
        fs::write(
            &second,
            "binds {\n Mod+J { focus-window-down; }\n Mod+K { focus-window-up; }\n}\n",
        )
        .unwrap();

        install_hjkl_bindings(&config, &temp.path().join("state")).unwrap();
        let first = fs::read_to_string(first).unwrap();
        assert!(first.contains("Mod+H repeat=false { spawn \"niri-zvim\" \"left\"; }"));
        assert!(first.contains("Mod+L { spawn \"niri-zvim\" \"right\"; }"));
        let second = fs::read_to_string(second).unwrap();
        assert!(second.contains("Mod+J { spawn \"niri-zvim\" \"down\"; }"));
        assert!(second.contains("Mod+K { spawn \"niri-zvim\" \"up\"; }"));
    }

    #[test]
    fn missing_bindings_leave_every_file_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.kdl");
        let original = "binds {\n Mod+H { focus-column-left; }\n}\n";
        fs::write(&config, original).unwrap();
        let error = install_hjkl_bindings(&config, &temp.path().join("state")).unwrap_err();
        assert!(error.to_string().contains("Mod+J, Mod+K, Mod+L"));
        assert_eq!(fs::read_to_string(config).unwrap(), original);
    }

    #[test]
    fn restore_refuses_to_overwrite_later_user_changes() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.kdl");
        fs::write(
            &config,
            bindings(
                "focus-column-left",
                "focus-window-down",
                "focus-window-up",
                "focus-column-right",
            ),
        )
        .unwrap();
        let state = temp.path().join("state");
        install_hjkl_bindings(&config, &state).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&config)
            .unwrap()
            .write_all(b"// later user change\n")
            .unwrap();

        let restored = restore_hjkl_bindings(&state).unwrap();
        assert!(restored.restored.is_empty());
        assert_eq!(restored.preserved, vec![config]);
        assert!(binding_manifest_path(&state).exists());
    }

    #[test]
    fn braces_in_strings_and_comments_do_not_confuse_binding_ranges() {
        let text = r#"
// binds { Mod+H { ignored; } }
binds {
    Mod+H hotkey-overlay-title="{ left }" { focus-column-left; }
    Mod+J { spawn-sh "echo }"; }
    Mod+K { /* } */ focus-window-up; }
    Mod+L { focus-column-right; }
}
"#;
        let spans = find_binding_spans(text).unwrap();
        assert_eq!(spans.len(), 4);
    }
}

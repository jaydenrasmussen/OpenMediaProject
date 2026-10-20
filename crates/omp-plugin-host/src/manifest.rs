//! `plugin.toml`: what a plugin is and what it may do.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::Permissions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Indexer,
    Notification,
    List,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: Kind,
    /// File name of the component, next to the manifest.
    pub wasm: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Folder the manifest was read from.
    #[serde(skip)]
    pub dir: PathBuf,
}

impl Manifest {
    pub fn parse(text: &str, dir: &Path) -> Result<Self> {
        let mut m: Manifest = toml::from_str(text).context("invalid plugin.toml")?;
        if m.id.is_empty()
            || !m
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            bail!("plugin id must be lowercase letters, digits and dashes");
        }
        // The component is loaded from the plugin's own folder, nowhere else.
        if m.wasm.contains(['/', '\\']) || m.wasm.starts_with('.') {
            bail!("wasm must be a file name in the plugin's folder");
        }
        m.dir = dir.to_path_buf();
        Ok(m)
    }

    pub fn wasm_path(&self) -> PathBuf {
        self.dir.join(&self.wasm)
    }

    pub fn permissions(&self) -> Permissions {
        Permissions {
            allowed_hosts: self.allowed_hosts.clone(),
        }
    }
}

/// Reads every `<dir>/<plugin>/plugin.toml`. A broken plugin is reported and
/// skipped; it does not stop the others loading.
pub fn discover(dir: &Path) -> (Vec<Manifest>, Vec<String>) {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (found, problems);
    };
    let mut folders: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        let file = folder.join("plugin.toml");
        if !file.is_file() {
            continue;
        }
        let result = std::fs::read_to_string(&file)
            .map_err(anyhow::Error::from)
            .and_then(|t| Manifest::parse(&t, &folder))
            .and_then(|m| {
                if m.wasm_path().is_file() {
                    Ok(m)
                } else {
                    bail!("{} is missing", m.wasm)
                }
            });
        match result {
            Ok(m) if found.iter().any(|f: &Manifest| f.id == m.id) => {
                problems.push(format!(
                    "{}: duplicate plugin id {}",
                    folder.display(),
                    m.id
                ));
            }
            Ok(m) => found.push(m),
            Err(e) => problems.push(format!("{}: {e:#}", folder.display())),
        }
    }
    (found, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
        id = "webhook"
        name = "Webhook"
        version = "0.1.0"
        kind = "notification"
        wasm = "webhook.wasm"
        allowed_hosts = ["*"]
    "#;

    #[test]
    fn parses_a_manifest() {
        let m = Manifest::parse(GOOD, Path::new("/p/webhook")).unwrap();
        assert_eq!((m.id.as_str(), m.kind), ("webhook", Kind::Notification));
        assert_eq!(m.wasm_path(), Path::new("/p/webhook/webhook.wasm"));
        assert!(m.permissions().allows("anything"));
    }

    #[test]
    fn rejects_unsafe_values() {
        for bad in [
            GOOD.replace("webhook.wasm", "../x.wasm"),
            GOOD.replace("webhook.wasm", "/etc/x.wasm"),
            GOOD.replace("\"webhook\"", "\"Web Hook\""),
            GOOD.replace("notification", "driver"),
        ] {
            assert!(Manifest::parse(&bad, Path::new("/p")).is_err(), "{bad}");
        }
    }

    #[test]
    fn discovery_skips_broken_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("a");
        std::fs::create_dir(&ok).unwrap();
        std::fs::write(ok.join("plugin.toml"), GOOD).unwrap();
        std::fs::write(ok.join("webhook.wasm"), b"x").unwrap();
        let no_wasm = dir.path().join("b");
        std::fs::create_dir(&no_wasm).unwrap();
        std::fs::write(
            no_wasm.join("plugin.toml"),
            GOOD.replace("webhook", "other"),
        )
        .unwrap();
        let junk = dir.path().join("c");
        std::fs::create_dir(&junk).unwrap();
        std::fs::write(junk.join("plugin.toml"), "nonsense").unwrap();

        let (found, problems) = discover(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(discover(&dir.path().join("missing")).0.len(), 0);
    }
}

//! Display choices made with the keys, written back to the config file.
//!
//! `f`, `c`, `w`, `s` and `v` change how the lyrics look, and a choice made
//! once should still be there on the next run rather than having to be made
//! again every time. The config file is where those settings already live, so
//! that is where the choice goes: one place to look, one place to edit, and a
//! key pressed is the same as having typed the line by hand.
//!
//! The file is the user's, so only the one key is touched. Everything else —
//! comments, blank lines, the order of keys, a trailing comment on the line
//! being changed — is left exactly as it was. A file that is not valid TOML is
//! not rewritten at all; the run that loaded it would have failed already, but
//! a file edited by hand mid-run must not be flattened by a keypress.
//!
//! Like the per-song offsets, disk is the truth on every write: the file is
//! read, changed and written back each time, so two `lyrics` running at once
//! cannot undo each other's choices.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Value};

/// Where the keys' choices are saved. A store with no path still works; the
/// choices last for the session and are forgotten on exit.
#[derive(Debug, Clone)]
pub struct Prefs {
    path: Option<PathBuf>,
}

impl Prefs {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Nowhere to save; choices last only for this session.
    pub fn in_memory() -> Self {
        Self::new(None)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Set one top-level key in the config file, creating the file (and its
    /// directory) when there is none yet.
    pub fn set(&self, key: &str, value: impl Into<Value>) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        // Write to what a symlink points at, not over the link: a config kept
        // in a dotfiles repo and linked into place should stay linked.
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());

        let before = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };
        let mut doc: DocumentMut = before
            .parse()
            .with_context(|| format!("{} is not valid TOML", path.display()))?;

        let mut value = value.into();
        match doc.get_mut(key).and_then(|item| item.as_value_mut()) {
            // Keep the spacing around the `=` and any comment after the value.
            Some(old) => {
                *value.decor_mut() = old.decor().clone();
                *old = value;
            }
            None => doc[key] = toml_edit::value(value),
        }

        let after = doc.to_string();
        if after == before {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        // Write-then-rename, so a process killed mid-write cannot leave a
        // truncated config that then fails to load.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, after).with_context(|| format!("cannot write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            anyhow::Error::new(e).context(format!("cannot write {}", path.display()))
        })
    }
}

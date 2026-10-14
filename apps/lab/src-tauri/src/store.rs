//! Read-or-default / write-atomically JSON persistence.
//!
//! Atomic because a half-written settings file is worse than a missing one:
//! `read_json` falls back to `Default` on any parse failure, so a truncated
//! write would silently reset the user's preferences.

use std::fs;
use std::io;
use std::path::Path;

use serde::{de::DeserializeOwned, Serialize};

/// Load `T` from `path`, falling back to `Default` if the file is missing or
/// unparseable.
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

/// Write `value` to `path` via a temp file + rename.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

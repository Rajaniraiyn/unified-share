use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use getrandom::fill;
use serde::{Deserialize, Serialize};

use crate::SCHEMA_VERSION;

const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp_unix: u64,
    pub route: String,
    pub target: String,
    pub item_count: usize,
    pub outcome: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
struct HistoryDocument {
    schema_version: u8,
    entries: Vec<HistoryEntry>,
}

pub fn record(route: &str, target: &str, item_count: usize, outcome: &str, message: &str) {
    if let Err(error) = try_record(HistoryEntry {
        id: random_id(),
        timestamp_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        route: route.to_owned(),
        target: target.to_owned(),
        item_count,
        outcome: outcome.to_owned(),
        message: message.to_owned(),
    }) {
        eprintln!("Could not update transfer history: {error:#}");
    }
}

pub fn print(limit: usize, json: bool) -> Result<()> {
    let mut entries = read_entries()?;
    entries.reverse();
    entries.truncate(limit);
    if json {
        println!(
            "{}",
            serde_json::to_string(&HistoryDocument {
                schema_version: SCHEMA_VERSION,
                entries,
            })?
        );
    } else if entries.is_empty() {
        println!("No transfer history yet");
    } else {
        for entry in entries {
            println!(
                "{}\t{}\t{}\t{} item(s)\t{}",
                entry.timestamp_unix, entry.outcome, entry.target, entry.item_count, entry.message
            );
        }
    }
    Ok(())
}

pub fn clear() -> Result<()> {
    let path = history_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("could not clear {}", path.display()))?;
    }
    Ok(())
}

fn try_record(entry: HistoryEntry) -> Result<()> {
    let path = history_path();
    let parent = path.parent().expect("history path has a parent");
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    serde_json::to_writer(&mut file, &entry)?;
    file.write_all(b"\n")?;
    drop(file);

    let entries = read_entries()?;
    if entries.len() > MAX_ENTRIES {
        rewrite_entries(&entries[entries.len() - MAX_ENTRIES..])?;
    }
    Ok(())
}

fn read_entries() -> Result<Vec<HistoryEntry>> {
    let path = history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path)?;
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Ok(entry) = serde_json::from_str(&line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn rewrite_entries(entries: &[HistoryEntry]) -> Result<()> {
    let path = history_path();
    let temporary = path.with_extension("jsonl.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    for entry in entries {
        serde_json::to_writer(&mut file, entry)?;
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn history_path() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/state")
        })
        .join("unified-share/history.jsonl")
}

fn random_id() -> String {
    let mut bytes = [0_u8; 12];
    fill(&mut bytes).expect("OS randomness unavailable");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_ids_are_96_bit_hex() {
        let id = random_id();
        assert_eq!(id.len(), 24);
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}

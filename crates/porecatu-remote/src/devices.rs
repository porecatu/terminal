// SPDX-License-Identifier: GPL-3.0-or-later

//! List of paired devices, persisted in `remote_devices.json` (ADR-0106 §5).
//!
//! The file sits in the data directory, next to `session.json`, and holds
//! **public** keys only. The rules are the ones of `pinned.json`:
//!
//! - **Fails closed.** A file that cannot be read as a valid list is moved
//!   aside to `remote_devices.json.corrompido-<stamp>` (with `.1`, `.2`, ...
//!   when that name is taken: never overwriting an earlier one) and the list
//!   starts empty, never partial.
//! - **Never overwrites a newer schema.** The file is left byte for byte
//!   untouched, the list is empty and read-only: [`DeviceStore::save`] does
//!   nothing.
//! - **Atomic write**: `<name>.tmp` in the same directory, `fsync`, `rename`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

use porecatu_protocol::encoding::{decode_base64url, encode_base64url};
use serde::{Deserialize, Serialize};

use crate::api::{DeviceInfo, DevicePlatform};
use crate::time::{file_stamp, rfc3339_utc};

/// Schema version this binary writes and reads without migration.
const SCHEMA_VERSION: u32 = 1;

/// File name of the list, in the data directory.
const FILE_NAME: &str = "remote_devices.json";

/// What [`DeviceStore::load`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadNote {
    /// No file (or one that could not be read at all): an empty list.
    Missing,
    /// A valid list.
    Loaded,
    /// Invalid file: renamed to `path` and kept there; the list starts empty.
    Quarantined { path: PathBuf },
    /// `schema_version` newer than this binary supports: the file is
    /// untouched, the list is empty and read-only.
    NewerSchema { found: u32 },
}

/// One device as it is written on disk.
#[derive(Debug, Serialize, Deserialize)]
struct RecordV1 {
    /// Public static X25519 key, unpadded base64url.
    key: String,
    name: String,
    /// `android`, `ios` or `other`.
    platform: String,
    paired_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_seen: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileV1 {
    schema_version: u32,
    #[serde(default)]
    devices: Vec<RecordV1>,
}

fn platform_to_text(platform: DevicePlatform) -> &'static str {
    match platform {
        DevicePlatform::Android => "android",
        DevicePlatform::Ios => "ios",
        DevicePlatform::Other => "other",
    }
}

fn platform_from_text(text: &str) -> Option<DevicePlatform> {
    match text {
        "android" => Some(DevicePlatform::Android),
        "ios" => Some(DevicePlatform::Ios),
        "other" => Some(DevicePlatform::Other),
        _ => None,
    }
}

fn record_to_info(record: &RecordV1) -> Option<DeviceInfo> {
    let key = <[u8; 32]>::try_from(decode_base64url(&record.key).ok()?.as_slice()).ok()?;
    Some(DeviceInfo {
        key,
        name: record.name.clone(),
        platform: platform_from_text(&record.platform)?,
        paired_at: record.paired_at.clone(),
        last_seen: record.last_seen.clone(),
    })
}

fn info_to_record(info: &DeviceInfo) -> RecordV1 {
    RecordV1 {
        key: encode_base64url(&info.key),
        name: info.name.clone(),
        platform: platform_to_text(info.platform).to_owned(),
        paired_at: info.paired_at.clone(),
        last_seen: info.last_seen.clone(),
    }
}

/// The paired devices and the file they live in.
#[derive(Debug)]
pub(crate) struct DeviceStore {
    path: PathBuf,
    devices: Vec<DeviceInfo>,
    read_only: bool,
}

impl DeviceStore {
    /// Reads `remote_devices.json` in `dir`. Never fails: whatever cannot be
    /// used becomes an empty list and says so in the [`LoadNote`].
    pub(crate) fn load(dir: &Path) -> (DeviceStore, LoadNote) {
        Self::load_at(dir, SystemTime::now())
    }

    fn load_at(dir: &Path, now: SystemTime) -> (DeviceStore, LoadNote) {
        let path = dir.join(FILE_NAME);
        let mut store = DeviceStore {
            path: path.clone(),
            devices: Vec::new(),
            read_only: false,
        };

        let Ok(bytes) = fs::read(&path) else {
            return (store, LoadNote::Missing);
        };
        let Ok(raw) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            let note = quarantine(&path, now);
            return (store, note);
        };

        // A missing, non-numeric or zero version is invalid, not "old". A
        // newer one is checked before anything else is parsed, so a file this
        // binary does not understand is never judged invalid.
        let version = raw
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if version == 0 {
            let note = quarantine(&path, now);
            return (store, note);
        }
        if version > u64::from(SCHEMA_VERSION) {
            store.read_only = true;
            return (
                store,
                LoadNote::NewerSchema {
                    found: u32::try_from(version).unwrap_or(u32::MAX),
                },
            );
        }

        let Ok(file) = serde_json::from_value::<FileV1>(raw) else {
            let note = quarantine(&path, now);
            return (store, note);
        };
        let mut devices: Vec<DeviceInfo> = Vec::with_capacity(file.devices.len());
        for record in &file.devices {
            let Some(info) = record_to_info(record) else {
                let note = quarantine(&path, now);
                return (store, note);
            };
            // A repeated key keeps its first record.
            if !devices.iter().any(|known| known.key == info.key) {
                devices.push(info);
            }
        }
        store.devices = devices;
        (store, LoadNote::Loaded)
    }

    /// The devices, in the order they were first paired.
    pub(crate) fn list(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }

    pub(crate) fn contains(&self, key: &[u8; 32]) -> bool {
        self.devices.iter().any(|device| &device.key == key)
    }

    /// Adds a device, or **replaces** name, platform and pairing date of the
    /// one with the same key (a new pairing of the same phone); `last_seen`
    /// is kept and the list never gets a duplicate.
    pub(crate) fn upsert(
        &mut self,
        key: [u8; 32],
        name: String,
        platform: DevicePlatform,
        now: SystemTime,
    ) {
        let paired_at = rfc3339_utc(now);
        match self.devices.iter_mut().find(|device| device.key == key) {
            Some(device) => {
                device.name = name;
                device.platform = platform;
                device.paired_at = paired_at;
            }
            None => self.devices.push(DeviceInfo {
                key,
                name,
                platform,
                paired_at,
                last_seen: None,
            }),
        }
    }

    /// Removes a device; `true` when it was there.
    pub(crate) fn remove(&mut self, key: &[u8; 32]) -> bool {
        let before = self.devices.len();
        self.devices.retain(|device| &device.key != key);
        self.devices.len() != before
    }

    pub(crate) fn clear(&mut self) {
        self.devices.clear();
    }

    /// Records a session of a known device; an unknown key changes nothing.
    pub(crate) fn touch_last_seen(&mut self, key: &[u8; 32], now: SystemTime) {
        if let Some(device) = self.devices.iter_mut().find(|device| &device.key == key) {
            device.last_seen = Some(rfc3339_utc(now));
        }
    }

    /// Whether the file is newer than this binary and must not be written.
    #[cfg(test)]
    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Writes the list atomically. Read-only (newer schema): does nothing and
    /// reports success, the file stays as it was.
    pub(crate) fn save(&self) -> io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        let file = FileV1 {
            schema_version: SCHEMA_VERSION,
            devices: self.devices.iter().map(info_to_record).collect(),
        };
        write_json_atomic(&self.path, &file)
    }
}

/// Moves an invalid file aside. If even that fails (the file vanished between
/// the read and now) there is nothing to preserve: `Missing`.
fn quarantine(path: &Path, now: SystemTime) -> LoadNote {
    match rename_to_first_free_corrupt(path, now) {
        Ok(corrupt) => LoadNote::Quarantined { path: corrupt },
        Err(_) => LoadNote::Missing,
    }
}

/// Renames `path` to `<name>.corrompido-<stamp>`, or with `.1`, `.2`, ...
/// appended when that name exists. Never overwrites.
fn rename_to_first_free_corrupt(path: &Path, now: SystemTime) -> io::Result<PathBuf> {
    let mut base = path.file_name().map(ToOwned::to_owned).unwrap_or_default();
    base.push(format!(".corrompido-{}", file_stamp(now)));

    let mut attempt = 0u32;
    loop {
        let mut name = base.clone();
        if attempt > 0 {
            name.push(format!(".{attempt}"));
        }
        let target = path.with_file_name(name);
        if !target.exists() {
            fs::rename(path, &target)?;
            return Ok(target);
        }
        attempt += 1;
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    use std::io::Write;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    let mut tmp_name = path.file_name().map(ToOwned::to_owned).unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp_path = path.with_file_name(tmp_name);

    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        std::time::UNIX_EPOCH + Duration::from_secs(secs)
    }

    // 2026-11-15T14:03:00Z
    const PAIRED: u64 = 1_794_751_380;

    fn key(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn file_path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let (store, note) = DeviceStore::load(dir.path());
        assert_eq!(note, LoadNote::Missing);
        assert!(store.list().is_empty());
        assert!(!store.is_read_only());
    }

    #[test]
    fn a_list_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = DeviceStore::load(dir.path());
        store.upsert(
            key(1),
            "Pixel 9".into(),
            DevicePlatform::Android,
            at(PAIRED),
        );
        store.upsert(
            key(2),
            "iPhone".into(),
            DevicePlatform::Ios,
            at(PAIRED + 60),
        );
        store.touch_last_seen(&key(1), at(PAIRED + 3600));
        store.save().unwrap();

        let (loaded, note) = DeviceStore::load(dir.path());
        assert_eq!(note, LoadNote::Loaded);
        assert_eq!(loaded.list(), store.list());
        let list = loaded.list();
        assert_eq!(list[0].name, "Pixel 9");
        assert_eq!(list[0].paired_at, "2026-11-15T14:03:00Z");
        assert_eq!(list[0].last_seen.as_deref(), Some("2026-11-15T15:03:00Z"));
        assert_eq!(list[1].last_seen, None);
        assert!(!dir.path().join("remote_devices.json.tmp").exists());
    }

    #[test]
    fn the_file_has_the_documented_shape() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = DeviceStore::load(dir.path());
        store.upsert(key(7), "Pixel 9".into(), DevicePlatform::Other, at(PAIRED));
        store.save().unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(file_path(dir.path())).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 1);
        let device = &value["devices"][0];
        assert_eq!(device["key"], encode_base64url(&key(7)));
        assert_eq!(device["name"], "Pixel 9");
        assert_eq!(device["platform"], "other");
        assert_eq!(device["paired_at"], "2026-11-15T14:03:00Z");
        // Never seen: the field is absent.
        assert!(device.get("last_seen").is_none());
    }

    #[test]
    fn a_null_last_seen_reads_as_never_seen() {
        let dir = tempfile::tempdir().unwrap();
        let json = format!(
            r#"{{"schema_version":1,"devices":[{{"key":"{}","name":"A","platform":"ios","paired_at":"2026-11-15T14:03:00Z","last_seen":null}}]}}"#,
            encode_base64url(&key(3))
        );
        fs::write(file_path(dir.path()), json).unwrap();
        let (store, note) = DeviceStore::load(dir.path());
        assert_eq!(note, LoadNote::Loaded);
        assert_eq!(store.list()[0].last_seen, None);
        assert_eq!(store.list()[0].platform, DevicePlatform::Ios);
    }

    #[test]
    fn pairing_the_same_key_again_replaces_and_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = DeviceStore::load(dir.path());
        store.upsert(key(1), "Old".into(), DevicePlatform::Android, at(PAIRED));
        store.touch_last_seen(&key(1), at(PAIRED + 10));
        store.upsert(key(1), "New".into(), DevicePlatform::Ios, at(PAIRED + 100));

        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "New");
        assert_eq!(list[0].platform, DevicePlatform::Ios);
        assert_eq!(list[0].paired_at, "2026-11-15T14:04:40Z");
        assert!(store.contains(&key(1)));
    }

    #[test]
    fn remove_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = DeviceStore::load(dir.path());
        store.upsert(key(1), "A".into(), DevicePlatform::Other, at(PAIRED));
        store.upsert(key(2), "B".into(), DevicePlatform::Other, at(PAIRED));

        assert!(store.remove(&key(1)));
        assert!(!store.remove(&key(1)));
        assert!(!store.contains(&key(1)));
        assert!(store.contains(&key(2)));

        store.clear();
        assert!(store.list().is_empty());
        store.save().unwrap();
        let (loaded, note) = DeviceStore::load(dir.path());
        assert_eq!(note, LoadNote::Loaded);
        assert!(loaded.list().is_empty());
    }

    #[test]
    fn last_seen_updates_and_an_unknown_key_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = DeviceStore::load(dir.path());
        store.upsert(key(1), "A".into(), DevicePlatform::Other, at(PAIRED));
        store.touch_last_seen(&key(1), at(PAIRED + 5));
        assert_eq!(
            store.list()[0].last_seen.as_deref(),
            Some("2026-11-15T14:03:05Z")
        );
        store.touch_last_seen(&key(1), at(PAIRED + 9));
        assert_eq!(
            store.list()[0].last_seen.as_deref(),
            Some("2026-11-15T14:03:09Z")
        );

        let before = store.list();
        store.touch_last_seen(&key(9), at(PAIRED + 99));
        assert_eq!(store.list(), before);
    }

    fn assert_quarantined(content: &str) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(file_path(dir.path()), content).unwrap();
        let (store, note) = DeviceStore::load_at(dir.path(), at(PAIRED));
        let LoadNote::Quarantined { path } = note else {
            panic!("expected quarantine for {content:?}, got {note:?}");
        };
        assert_eq!(
            path,
            dir.path()
                .join("remote_devices.json.corrompido-20261115-140300")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert!(!file_path(dir.path()).exists());
        assert!(store.list().is_empty());
        assert!(!store.is_read_only());
    }

    #[test]
    fn invalid_files_go_to_quarantine() {
        assert_quarantined("not json at all");
        assert_quarantined(r#"{"devices":[]}"#);
        assert_quarantined(r#"{"schema_version":0,"devices":[]}"#);
        assert_quarantined(r#"{"schema_version":"1","devices":[]}"#);
        // Key that does not decode to 32 bytes.
        assert_quarantined(
            r#"{"schema_version":1,"devices":[{"key":"AAAA","name":"A","platform":"ios","paired_at":"x"}]}"#,
        );
        assert_quarantined(
            r#"{"schema_version":1,"devices":[{"key":"!!!","name":"A","platform":"ios","paired_at":"x"}]}"#,
        );
        // Unknown platform.
        let good_key = encode_base64url(&key(1));
        assert_quarantined(&format!(
            r#"{{"schema_version":1,"devices":[{{"key":"{good_key}","name":"A","platform":"symbian","paired_at":"x"}}]}}"#
        ));
        // Missing required field.
        assert_quarantined(&format!(
            r#"{{"schema_version":1,"devices":[{{"key":"{good_key}","platform":"ios","paired_at":"x"}}]}}"#
        ));
    }

    #[test]
    fn a_second_quarantine_never_overwrites_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut moved = Vec::new();
        for content in ["first bad", "second bad", "third bad"] {
            fs::write(file_path(dir.path()), content).unwrap();
            let (_, note) = DeviceStore::load_at(dir.path(), at(PAIRED));
            let LoadNote::Quarantined { path } = note else {
                panic!("expected quarantine, got {note:?}");
            };
            moved.push((path, content));
        }

        let names: Vec<_> = moved
            .iter()
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "remote_devices.json.corrompido-20261115-140300",
                "remote_devices.json.corrompido-20261115-140300.1",
                "remote_devices.json.corrompido-20261115-140300.2",
            ]
        );
        for (path, content) in moved {
            assert_eq!(fs::read_to_string(path).unwrap(), content);
        }
    }

    #[test]
    fn a_newer_schema_is_read_only_and_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"{"schema_version":2,"devices":[{"anything":true}]}"#;
        fs::write(file_path(dir.path()), content).unwrap();

        let (mut store, note) = DeviceStore::load(dir.path());
        assert_eq!(note, LoadNote::NewerSchema { found: 2 });
        assert!(store.is_read_only());
        assert!(store.list().is_empty());

        store.upsert(key(1), "A".into(), DevicePlatform::Other, at(PAIRED));
        store.save().unwrap();
        assert_eq!(fs::read_to_string(file_path(dir.path())).unwrap(), content);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn save_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        let (mut store, _) = DeviceStore::load(&nested);
        store.upsert(key(1), "A".into(), DevicePlatform::Other, at(PAIRED));
        store.save().unwrap();
        assert!(file_path(&nested).exists());
    }

    #[test]
    fn only_public_keys_and_plain_fields_are_written() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = DeviceStore::load(dir.path());
        store.upsert(key(1), "A".into(), DevicePlatform::Other, at(PAIRED));
        store.save().unwrap();
        let text = fs::read_to_string(file_path(dir.path())).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let fields: Vec<_> = value["devices"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(fields, ["key", "name", "paired_at", "platform"]);
    }
}

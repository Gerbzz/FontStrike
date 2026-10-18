//! Lossless, crash-safe UFO persistence.
//!
//! Saving never edits the live UFO in place:
//!
//! 1. the model is written to `<name>.fontstrike-staging`;
//! 2. data `norad` does not model (unknown root files, unknown files inside
//!    layer folders, unknown `fontinfo.plist` / `layerinfo.plist` keys) is
//!    copied or merged into the stage verbatim;
//! 3. the stage is reloaded and compared with the in-memory model, and every
//!    file of the original UFO must either be reproduced or be data the model
//!    owns (glyph files, known plists);
//! 4. a journal file `<name>.fontstrike-journal` records the intent to commit;
//! 5. the live UFO is renamed to `<name>.fontstrike-backup`, the stage takes
//!    its place, and the backup and journal are removed.
//!
//! An interruption at any step leaves either the old UFO, the new UFO, or a
//! state that [`recover`] can finish deterministically.

use crate::{Error, Result};
use norad::{Font, FormatVersion};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub(crate) type Snapshot = BTreeMap<PathBuf, String>;

const ROOT_MANAGED: &[&str] = &[
    "metainfo.plist",
    "fontinfo.plist",
    "groups.plist",
    "kerning.plist",
    "features.fea",
    "lib.plist",
    "layercontents.plist",
];
const LAYER_MANAGED: &[&str] = &["contents.plist", "layerinfo.plist"];
const LAYERINFO_KNOWN: &[&str] = &["color", "lib"];

/// Data that `norad` does not model and FontStrike carries across saves.
#[derive(Clone, Debug, Default)]
pub(crate) struct Preserved {
    /// Paths (files and directories) copied verbatim from the live UFO.
    pub verbatim: Vec<PathBuf>,
    /// Paths the model owns; they may legitimately disappear on save.
    pub managed: BTreeSet<PathBuf>,
    pub fontinfo_extra: plist::Dictionary,
    pub layerinfo_extra: BTreeMap<PathBuf, plist::Dictionary>,
}

/// What is on disk for an open project.
#[derive(Clone, Debug, Default)]
pub(crate) struct DiskState {
    pub snapshot: Snapshot,
    pub preserved: Preserved,
}

pub(crate) fn sibling(root: &Path, suffix: &str) -> PathBuf {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "font.ufo".into());
    root.with_file_name(format!("{name}{suffix}"))
}
pub(crate) fn staging_path(root: &Path) -> PathBuf {
    sibling(root, ".fontstrike-staging")
}
pub(crate) fn backup_path(root: &Path) -> PathBuf {
    sibling(root, ".fontstrike-backup")
}
pub(crate) fn journal_path(root: &Path) -> PathBuf {
    sibling(root, ".fontstrike-journal")
}
/// Directory holding the most recent autosave of unsaved edits.
pub fn autosave_path(root: &Path) -> PathBuf {
    sibling(root, ".fontstrike-autosave")
}
fn autosave_tmp_path(root: &Path) -> PathBuf {
    sibling(root, ".fontstrike-autosave-tmp")
}
fn autosave_backup_path(root: &Path) -> PathBuf {
    sibling(root, ".fontstrike-autosave-backup")
}

pub(crate) fn autosave_to_restore(root: &Path) -> Option<PathBuf> {
    let current = autosave_path(root);
    if current.is_dir() {
        Some(current)
    } else {
        let backup = autosave_backup_path(root);
        backup.is_dir().then_some(backup)
    }
}

fn io_err(context: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("{context}: {e}"))
}

/// SHA-256 of every file plus every directory entry under `root`.
pub(crate) fn snapshot_tree(root: &Path) -> Result<Snapshot> {
    let mut snapshot = BTreeMap::new();
    if !root.exists() {
        return Ok(snapshot);
    }
    if !root.is_dir() {
        return Err(Error::new("UFO source is not a directory"));
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|e| io_err("read UFO folder", e))? {
            let entry = entry.map_err(|e| io_err("read UFO folder", e))?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|e| Error::new(e.to_string()))?
                .to_path_buf();
            let kind = fs::symlink_metadata(&path)
                .map_err(|e| io_err("inspect UFO file", e))?
                .file_type();
            if kind.is_symlink() {
                return Err(Error::new(format!(
                    "UFO contains a symlink, which FontStrike does not follow: {}",
                    relative.display()
                )));
            }
            if kind.is_dir() {
                snapshot.insert(relative, "directory".into());
                pending.push(path);
            } else if kind.is_file() {
                let mut file = fs::File::open(&path).map_err(|e| io_err("read UFO file", e))?;
                let mut hash = Sha256::new();
                io::copy(&mut file, &mut hash).map_err(|e| io_err("read UFO file", e))?;
                snapshot.insert(relative, format!("{:x}", hash.finalize()));
            } else {
                return Err(Error::new(format!(
                    "unsupported file type in UFO: {}",
                    relative.display()
                )));
            }
        }
    }
    Ok(snapshot)
}

fn read_dict(path: &Path) -> Result<Option<plist::Dictionary>> {
    if !path.exists() {
        return Ok(None);
    }
    let value = plist::Value::from_file(path)
        .map_err(|e| io_err(&format!("read {}", path.display()), e))?;
    match value {
        plist::Value::Dictionary(d) => Ok(Some(d)),
        _ => Err(Error::new(format!(
            "{} is not a dictionary",
            path.display()
        ))),
    }
}

/// Classifies every file of a freshly loaded UFO into model-owned and
/// verbatim-preserved data and returns human-readable notes.
pub(crate) fn analyse(
    root: &Path,
    font: &Font,
    snapshot: &Snapshot,
) -> Result<(Preserved, Vec<String>)> {
    let mut preserved = Preserved::default();
    let mut notes = Vec::new();
    let layer_dirs: BTreeMap<PathBuf, &norad::Layer> = font
        .layers
        .iter()
        .map(|l| (l.path().to_path_buf(), l))
        .collect();
    let mut glif_paths: BTreeSet<PathBuf> = BTreeSet::new();
    for layer in font.layers.iter() {
        for glyph in layer.iter() {
            if let Some(p) = layer.get_path(glyph.name()) {
                glif_paths.insert(layer.path().join(p));
            }
        }
    }
    for path in snapshot.keys() {
        let mut parts = path.components();
        let first = parts
            .next()
            .map(|c| PathBuf::from(c.as_os_str()))
            .unwrap_or_default();
        let depth = path.components().count();
        let root_managed = depth == 1 && ROOT_MANAGED.iter().any(|m| Path::new(m) == path);
        let store = first == Path::new("data") || first == Path::new("images");
        let managed = if root_managed || store {
            true
        } else if layer_dirs.contains_key(&first) {
            if depth == 1 {
                true
            } else if depth == 2 {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                LAYER_MANAGED.iter().any(|m| Some(m.to_string()) == name)
                    || glif_paths.contains(path)
            } else {
                false
            }
        } else {
            false
        };
        if managed {
            preserved.managed.insert(path.clone());
        } else {
            preserved.verbatim.push(path.clone());
        }
    }
    let top_unknown: Vec<String> = preserved
        .verbatim
        .iter()
        .filter(|p| {
            p.components().count() == 1
                || layer_dirs
                    .contains_key(&PathBuf::from(p.components().next().unwrap().as_os_str()))
        })
        .filter(|p| p.as_path() != Path::new(".fontstrike-v1"))
        .map(|p| p.display().to_string())
        .collect();
    if !top_unknown.is_empty() {
        notes.push(format!(
            "Preserved verbatim (not editable here): {}",
            top_unknown.join(", ")
        ));
    }

    // Unknown fontinfo keys are invisible to norad's typed FontInfo; keep them.
    if let Some(original) = read_dict(&root.join("fontinfo.plist"))? {
        let known = match plist::to_value(&font.font_info) {
            Ok(plist::Value::Dictionary(d)) => d,
            _ => plist::Dictionary::new(),
        };
        for (key, value) in original {
            if !known.contains_key(&key) && key != "guidelines" {
                preserved.fontinfo_extra.insert(key, value);
            }
        }
        if !preserved.fontinfo_extra.is_empty() {
            let keys: Vec<_> = preserved.fontinfo_extra.keys().cloned().collect();
            notes.push(format!(
                "fontinfo.plist keys not modelled by FontStrike are preserved unchanged: {}",
                keys.join(", ")
            ));
        }
    }
    for dir in layer_dirs.keys() {
        if let Some(original) = read_dict(&root.join(dir).join("layerinfo.plist"))? {
            let extra: plist::Dictionary = original
                .into_iter()
                .filter(|(k, _)| !LAYERINFO_KNOWN.contains(&k.as_str()))
                .collect();
            if !extra.is_empty() {
                preserved.layerinfo_extra.insert(dir.clone(), extra);
            }
        }
    }
    Ok((preserved, notes))
}

fn write_dict(path: &Path, dict: plist::Dictionary) -> Result<()> {
    plist::Value::Dictionary(dict)
        .to_file_xml(path)
        .map_err(|e| io_err(&format!("write {}", path.display()), e))
}

fn merge_extra(path: &Path, extra: &plist::Dictionary) -> Result<()> {
    if extra.is_empty() {
        return Ok(());
    }
    let mut dict = read_dict(path)?.unwrap_or_default();
    for (k, v) in extra {
        if !dict.contains_key(k) {
            dict.insert(k.clone(), v.clone());
        }
    }
    write_dict(path, dict)
}

/// Writes `font` as UFO 3 into a brand-new directory `dest`.
pub(crate) fn write_model(font: &Font, dest: &Path) -> Result<()> {
    if dest.exists() {
        return Err(Error::new(format!("{} already exists", dest.display())));
    }
    if font.meta.format_version != FormatVersion::V3 {
        let mut upgraded = font.clone();
        upgraded.meta.format_version = FormatVersion::V3;
        upgraded.meta.format_version_minor = 0;
        return upgraded.save(dest).map_err(|e| io_err("write UFO", e));
    }
    font.save(dest).map_err(|e| io_err("write UFO", e))
}

fn sync_tree(root: &Path) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| io_err("flush stage", e))? {
            let path = entry.map_err(|e| io_err("flush stage", e))?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| io_err("flush stage", e))?;
            }
        }
    }
    Ok(())
}

/// Renames with short retries: on Windows, indexers and antivirus scanners
/// briefly hold handles inside freshly written folders.
fn rename_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut last = None;
    for attempt in 0..20 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e)
                if e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(32) =>
            {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(25 * (attempt + 1)));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
}

fn compare_models(expected: &Font, actual: &Font) -> Result<()> {
    let fail = |what: &str| {
        Err(Error::new(format!(
            "staged UFO check failed: {what} differs after reload; source preserved"
        )))
    };
    if expected.font_info != actual.font_info {
        return fail("fontinfo");
    }
    if expected.lib != actual.lib {
        return fail("lib.plist");
    }
    if expected.groups != actual.groups {
        return fail("groups");
    }
    if expected.kerning != actual.kerning {
        return fail("kerning");
    }
    if expected.features.replace("\r\n", "\n") != actual.features {
        return fail("features");
    }
    if expected.layers.len() != actual.layers.len() {
        return fail("layer list");
    }
    for (a, b) in expected.layers.iter().zip(actual.layers.iter()) {
        if a.name() != b.name() || a.len() != b.len() || a.color != b.color || a.lib != b.lib {
            return fail(&format!("layer {}", a.name()));
        }
        for glyph in a.iter() {
            match b.get_glyph(glyph.name()) {
                Some(other) if other == glyph => {}
                _ => return fail(&format!("glyph {} in layer {}", glyph.name(), a.name())),
            }
        }
    }
    Ok(())
}

/// Where a simulated interruption stops a save (tests and fault drills).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveInterruption {
    AfterStage,
    AfterJournal,
    AfterBackupRename,
    AfterSwap,
}

/// Saves `font` to `root` atomically, preserving unmodelled data.
pub(crate) fn save(
    font: &Font,
    root: &Path,
    disk: &DiskState,
    stop: Option<SaveInterruption>,
) -> Result<DiskState> {
    if let Some(problem) = pending_recovery(root) {
        return Err(Error::new(format!("{problem}. Use Recover before saving.")));
    }
    let stage = staging_path(root);
    let backup = backup_path(root);
    let journal = journal_path(root);
    let current = snapshot_tree(root)?;
    if root.exists() && current != disk.snapshot {
        return Err(Error::new(
            "the UFO changed on disk outside FontStrike since it was opened; reopen it before saving (nothing was written)",
        ));
    }
    if let Some(parent) = root.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        return Err(Error::new(format!(
            "folder {} does not exist",
            parent.display()
        )));
    }
    let result = (|| -> Result<Snapshot> {
        write_model(font, &stage)?;
        // Carry over data the model does not own.
        for rel in &disk.preserved.verbatim {
            let from = root.join(rel);
            let to = stage.join(rel);
            if from.is_dir() {
                fs::create_dir_all(&to).map_err(|e| io_err("preserve folder", e))?;
            } else if from.is_file() {
                if to.exists() {
                    return Err(Error::new(format!(
                        "staged model file conflicts with preserved {}; source preserved",
                        rel.display()
                    )));
                }
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent).map_err(|e| io_err("preserve folder", e))?;
                }
                fs::copy(&from, &to)
                    .map_err(|e| io_err(&format!("preserve {}", rel.display()), e))?;
            }
        }
        merge_extra(
            &stage.join("fontinfo.plist"),
            &disk.preserved.fontinfo_extra,
        )?;
        for (dir, extra) in &disk.preserved.layerinfo_extra {
            if stage.join(dir).is_dir() {
                merge_extra(&stage.join(dir).join("layerinfo.plist"), extra)?;
            }
        }
        let reloaded = std::panic::catch_unwind(|| Font::load(&stage))
            .map_err(|_| Error::new("staged UFO check crashed the reader; source preserved"))?
            .map_err(|e| io_err("staged UFO check", e))?;
        let mut expected = font.clone();
        expected.meta.format_version = FormatVersion::V3;
        compare_models(&expected, &reloaded)?;
        let staged = snapshot_tree(&stage)?;
        if let Some(missing) = current
            .keys()
            .find(|p| !staged.contains_key(*p) && !disk.preserved.managed.contains(*p))
        {
            return Err(Error::new(format!(
                "saving would drop {}; source preserved",
                missing.display()
            )));
        }
        sync_tree(&stage)?;
        Ok(staged)
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&stage);
        return Err(e);
    }
    if stop == Some(SaveInterruption::AfterStage) {
        return Err(Error::new("simulated interruption after staging"));
    }
    if snapshot_tree(root)? != current {
        let _ = fs::remove_dir_all(&stage);
        return Err(Error::new(
            "the UFO changed on disk during save; source preserved",
        ));
    }
    {
        let mut f = fs::File::create(&journal).map_err(|e| io_err("write save journal", e))?;
        f.write_all(b"FontStrike commit journal v1\nstaging complete\n")
            .and_then(|()| f.sync_all())
            .map_err(|e| io_err("write save journal", e))?;
    }
    if stop == Some(SaveInterruption::AfterJournal) {
        return Err(Error::new("simulated interruption after journal"));
    }
    if root.exists() {
        rename_retry(root, &backup).map_err(|e| {
            let _ = fs::remove_file(&journal);
            let _ = fs::remove_dir_all(&stage);
            io_err(
                "move current UFO aside (is a file inside it open in another program?)",
                e,
            )
        })?;
    }
    if stop == Some(SaveInterruption::AfterBackupRename) {
        return Err(Error::new("simulated interruption after backup rename"));
    }
    if let Err(e) = rename_retry(&stage, root) {
        if backup.exists() && rename_retry(&backup, root).is_ok() {
            let _ = fs::remove_file(&journal);
            let _ = fs::remove_dir_all(&stage);
        }
        return Err(io_err("move saved UFO into place", e));
    }
    if stop == Some(SaveInterruption::AfterSwap) {
        return Err(Error::new("simulated interruption after swap"));
    }
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|e| io_err("remove previous version", e))?;
    }
    let _ = fs::remove_file(&journal);
    let snapshot = snapshot_tree(root)?;
    let (preserved, _) = analyse(root, font, &snapshot)?;
    Ok(DiskState {
        snapshot,
        preserved,
    })
}

/// Describes leftover save artifacts that need [`recover`], if any.
pub fn pending_recovery(root: &Path) -> Option<String> {
    let stage = staging_path(root).exists();
    let backup = backup_path(root).exists();
    let journal = journal_path(root).exists();
    match (journal, stage, backup) {
        (false, false, false) => None,
        (true, _, _) => Some("A save was interrupted after it was fully written".into()),
        (false, true, _) => {
            Some("An unfinished save folder was left by an interrupted save".into())
        }
        (false, false, true) => {
            Some("A previous-version backup was left by an interrupted save".into())
        }
    }
}

/// What [`recover`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecoveryReport {
    pub actions: Vec<String>,
    /// Folders kept for manual inspection instead of being deleted.
    pub kept: Vec<PathBuf>,
}

fn free_name(base: &Path) -> PathBuf {
    for n in 1.. {
        let candidate = sibling(base, &format!(".fontstrike-recovered-{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

/// Finishes or rolls back an interrupted save. Nothing a user could need is
/// deleted: an incomplete stage or an ambiguous backup is renamed to
/// `<name>.fontstrike-recovered-N` and reported.
pub fn recover(root: &Path) -> Result<RecoveryReport> {
    let stage = staging_path(root);
    let backup = backup_path(root);
    let journal = journal_path(root);
    let mut report = RecoveryReport::default();
    let rename = |a: &Path, b: &Path| rename_retry(a, b).map_err(|e| io_err("recovery rename", e));
    if journal.exists() {
        // The stage was complete and verified: roll the commit forward.
        if stage.exists() {
            if root.exists() {
                if backup.exists() {
                    let keep = free_name(root);
                    rename(&backup, &keep)?;
                    report.kept.push(keep);
                }
                rename(root, &backup)?;
            }
            rename(&stage, root)?;
            report
                .actions
                .push("Completed the interrupted save with the fully written version".into());
        } else if !root.exists() && backup.exists() {
            rename(&backup, root)?;
            report
                .actions
                .push("Restored the previous version (the new version was missing)".into());
        }
        if backup.exists() && root.exists() {
            fs::remove_dir_all(&backup).map_err(|e| io_err("remove superseded backup", e))?;
            report
                .actions
                .push("Removed the superseded previous-version backup".into());
        }
        fs::remove_file(&journal).map_err(|e| io_err("remove journal", e))?;
    } else {
        if stage.exists() {
            let keep = free_name(&stage);
            rename(&stage, &keep)?;
            report.actions.push(format!(
                "Moved an unfinished save aside to {} (the saved UFO was not changed)",
                keep.display()
            ));
            report.kept.push(keep);
        }
        if backup.exists() {
            if root.exists() {
                let keep = free_name(&backup);
                rename(&backup, &keep)?;
                report
                    .actions
                    .push(format!("Kept an older backup as {}", keep.display()));
                report.kept.push(keep);
            } else {
                rename(&backup, root)?;
                report
                    .actions
                    .push("Restored the last saved version from its backup".into());
            }
        }
    }
    if report.actions.is_empty() {
        report.actions.push("Nothing to recover".into());
    }
    Ok(report)
}

/// Writes an autosave copy of unsaved edits next to the UFO.
pub(crate) fn write_autosave(font: &Font, root: &Path) -> Result<()> {
    let tmp = autosave_tmp_path(root);
    let dest = autosave_path(root);
    let backup = autosave_backup_path(root);
    if backup.exists() {
        if !dest.is_dir() {
            return Err(Error::new(
                "an interrupted autosave has a recoverable previous copy; restore or discard it first",
            ));
        }
        // A complete new copy is already in place; the older backup can go.
        fs::remove_dir_all(&backup).map_err(|e| io_err("clear old autosave backup", e))?;
    }
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| io_err("clear autosave", e))?;
    }
    write_model(font, &tmp)?;
    sync_tree(&tmp)?;
    if dest.exists() {
        rename_retry(&dest, &backup).map_err(|e| io_err("back up autosave", e))?;
    }
    if let Err(error) = rename_retry(&tmp, &dest) {
        if backup.exists() {
            let _ = rename_retry(&backup, &dest);
        }
        return Err(io_err("place autosave", error));
    }
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|e| io_err("clear old autosave backup", e))?;
    }
    Ok(())
}

pub(crate) fn remove_autosave(root: &Path) -> Result<()> {
    for p in [
        autosave_path(root),
        autosave_tmp_path(root),
        autosave_backup_path(root),
    ] {
        if p.exists() {
            fs::remove_dir_all(&p).map_err(|e| io_err("remove autosave", e))?;
        }
    }
    Ok(())
}

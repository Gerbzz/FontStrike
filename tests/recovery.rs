//! Interrupted saves, recovery, autosave, repeated saves and exports.

mod common;
use common::*;
use fontstrike::session::{OpenOutcome, Session};
use fontstrike::{FontProject, SaveInterruption, pending_recovery, recover};
use std::fs;

fn fresh(name: &str) -> (std::path::PathBuf, FontProject) {
    let dir = scratch(name);
    let root = dir.join("Font.ufo");
    let mut p = FontProject::create(&root, "Recovery Fixture", "Regular").unwrap();
    p.add_glyph("A", &['A'], 600.0).unwrap();
    p.add_rectangle("A", 50.0, 0.0, 550.0, 700.0).unwrap();
    p.save().unwrap();
    (root, p)
}

#[test]
fn every_interruption_point_recovers_to_old_or_new_version() {
    for (stop, expect_new) in [
        (SaveInterruption::AfterStage, false),
        (SaveInterruption::AfterJournal, true),
        (SaveInterruption::AfterBackupRename, true),
        (SaveInterruption::AfterSwap, true),
    ] {
        let (root, mut p) = fresh(&format!("interrupt-{stop:?}"));
        p.set_width("A", 777.0).unwrap();
        assert!(p.save_interrupted(stop).is_err());
        drop(p); // simulated crash
        let problem = pending_recovery(&root);
        assert!(problem.is_some(), "{stop:?} leaves recoverable state");
        // Opening refuses until recovery runs; saving refuses too.
        let err = FontProject::open(&root)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(err.contains("Recover"), "{stop:?}: {err}");
        let report = recover(&root).unwrap();
        assert!(!report.actions.is_empty());
        assert!(
            pending_recovery(&root).is_none(),
            "{stop:?}: clean after recovery"
        );
        let reopened = FontProject::open(&root).unwrap();
        let width = reopened.width("A").unwrap();
        assert_eq!(width, if expect_new { 777.0 } else { 600.0 }, "{stop:?}");
        // Nothing is silently deleted: an unfinished stage is kept aside.
        if stop == SaveInterruption::AfterStage {
            assert_eq!(report.kept.len(), 1);
            assert!(report.kept[0].exists());
        }
        // The project saves normally afterwards.
        let mut again = reopened;
        again.set_width("A", 640.0).unwrap();
        again.save().unwrap();
        assert_eq!(FontProject::open(&root).unwrap().width("A").unwrap(), 640.0);
    }
}

#[test]
fn legacy_backup_without_journal_is_restored_when_source_is_missing() {
    let (root, p) = fresh("legacy");
    drop(p);
    let backup = root.with_file_name("Font.ufo.fontstrike-backup");
    fs::rename(&root, &backup).unwrap();
    assert!(FontProject::open(&root).is_err());
    let report = recover(&root).unwrap();
    assert!(report.actions[0].contains("Restored"));
    assert_eq!(FontProject::open(&root).unwrap().width("A").unwrap(), 600.0);
}

#[test]
fn session_offers_recovery_and_autosave() {
    let (root, mut p) = fresh("session-recovery");
    p.set_width("A", 700.0).unwrap();
    let _ = p.save_interrupted(SaveInterruption::AfterBackupRename);
    drop(p);
    let mut s = Session::new();
    assert!(matches!(
        s.open_font(Some(root.clone()), false),
        OpenOutcome::NeedsRecovery(_)
    ));
    assert_eq!(s.recover_and_open(&root), OpenOutcome::Opened);
    assert_eq!(s.project.as_ref().unwrap().width("A").unwrap(), 700.0);

    // Unsaved edits survive a crash through autosave.
    let p = s.project.as_mut().unwrap();
    assert!(!p.autosave().unwrap(), "clean project writes no autosave");
    p.set_width("A", 720.0).unwrap();
    p.add_glyph("B", &['B'], 500.0).unwrap();
    assert!(p.autosave().unwrap());
    let saved_tree = tree(&root);
    s.project = None; // simulated crash: nothing saved
    assert_eq!(
        tree(&root),
        saved_tree,
        "autosave never touches the UFO itself"
    );
    let mut s2 = Session::new();
    assert_eq!(
        s2.open_font(Some(root.clone()), false),
        OpenOutcome::AutosaveAvailable
    );
    assert_eq!(s2.resolve_autosave(&root, true), OpenOutcome::Opened);
    let restored = s2.project.as_mut().unwrap();
    assert_eq!(restored.width("A").unwrap(), 720.0);
    assert!(restored.glyph("B").is_some());
    assert!(restored.is_dirty());
    restored.undo().unwrap();
    assert_eq!(
        restored.width("A").unwrap(),
        700.0,
        "restore is one undoable step"
    );
    restored.redo().unwrap();
    assert!(s2.save());
    assert!(
        !FontProject::autosave_available(&root),
        "saving clears the autosave"
    );
    assert_eq!(FontProject::open(&root).unwrap().width("A").unwrap(), 720.0);

    // Declining an autosave discards it and opens the saved version.
    let p = s2.project.as_mut().unwrap();
    p.set_width("A", 730.0).unwrap();
    p.autosave().unwrap();
    let mut s3 = Session::new();
    assert_eq!(
        s3.open_font(Some(root.clone()), false),
        OpenOutcome::AutosaveAvailable
    );
    assert_eq!(s3.resolve_autosave(&root, false), OpenOutcome::Opened);
    assert_eq!(s3.project.as_ref().unwrap().width("A").unwrap(), 720.0);
    assert!(!FontProject::autosave_available(&root));
}

#[test]
fn repeated_saves_and_exports_leave_no_temporary_files() {
    let (root, mut p) = fresh("repeat");
    let dir = root.parent().unwrap().to_path_buf();
    let ttf = dir.join("Font.ttf");
    for i in 0..8 {
        p.set_width("A", 600.0 + i as f64).unwrap();
        p.save().unwrap();
        let report = p.export_ttf(&ttf).unwrap();
        assert_eq!(report.mappings_checked, 2);
        let face_bytes = fs::read(&ttf).unwrap();
        let face = ttf_parser::Face::parse(&face_bytes, 0).unwrap();
        let gid = face.glyph_index('A').unwrap();
        assert_eq!(face.glyph_hor_advance(gid), Some(600 + i));
    }
    assert_eq!(
        listing(&dir),
        vec!["Font.ttf".to_string(), "Font.ufo".to_string()]
    );
    assert_eq!(FontProject::open(&root).unwrap().width("A").unwrap(), 607.0);
}

#[test]
fn failed_export_keeps_previous_file_and_stale_temp_files_are_not_touched() {
    let (root, mut p) = fresh("export-fail");
    let dir = root.parent().unwrap().to_path_buf();
    let ttf = dir.join("Font.ttf");
    p.export_ttf(&ttf).unwrap();
    let good = fs::read(&ttf).unwrap();
    // A stale temp file from a crashed export does not block or get deleted.
    let stale = dir.join(".Font.ttf.fontstrike-crashed.tmp");
    fs::write(&stale, b"stale").unwrap();
    p.delete_glyph(".notdef").unwrap();
    let err = p.export_ttf(&ttf).unwrap_err().to_string();
    assert!(err.contains(".notdef"), "{err}");
    assert_eq!(fs::read(&ttf).unwrap(), good, "previous export intact");
    assert_eq!(fs::read(&stale).unwrap(), b"stale");
    p.undo().unwrap();
    p.export_ttf(&ttf).unwrap();
    assert!(
        p.export_ttf(&dir.join("Font.otf")).is_err(),
        "only .ttf is produced"
    );
    assert!(
        p.export_ttf(&dir.join("missing-folder").join("x.ttf"))
            .is_err()
    );
    // Export compiles unsaved edits without saving the source.
    let source_before = tree(&root);
    p.set_width("A", 655.0).unwrap();
    p.export_ttf(&ttf).unwrap();
    assert_eq!(tree(&root), source_before);
}

#[test]
fn interrupted_autosave_swap_still_offers_previous_copy() {
    let (root, mut project) = fresh("autosave-swap");
    project.set_width("A", 713.0).unwrap();
    assert!(project.autosave().unwrap());
    let autosave = fontstrike::storage::autosave_path(&root);
    let backup = root.with_file_name("Font.ufo.fontstrike-autosave-backup");
    fs::rename(&autosave, &backup).unwrap(); // crash after old copy moved aside

    let mut session = Session::new();
    assert_eq!(
        session.open_font(Some(root.clone()), false),
        OpenOutcome::AutosaveAvailable
    );
    assert_eq!(session.resolve_autosave(&root, true), OpenOutcome::Opened);
    assert_eq!(session.project.as_ref().unwrap().width("A").unwrap(), 713.0);
}

#[test]
fn replacing_autosave_keeps_latest_copy_and_clears_backup() {
    let (root, mut project) = fresh("autosave-replace");
    project.set_width("A", 713.0).unwrap();
    assert!(project.autosave().unwrap());
    project.set_width("A", 714.0).unwrap();
    assert!(project.autosave().unwrap());
    assert!(
        !root
            .with_file_name("Font.ufo.fontstrike-autosave-backup")
            .exists()
    );
    let restored = FontProject::open_autosave(&root).unwrap();
    assert_eq!(restored.width("A").unwrap(), 714.0);
}

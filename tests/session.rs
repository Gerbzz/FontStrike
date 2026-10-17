//! Dialog cancellation and unsaved-change guards in the document controller.

mod common;
use common::*;
use fontstrike::session::{Level, OpenOutcome, Session};

#[test]
fn cancelled_dialogs_change_nothing() {
    let dir = scratch("cancel");
    let mut s = Session::new();
    assert!(!s.new_font(None, "X", "Regular", false));
    assert_eq!(s.notice.level, Level::Info);
    assert!(s.notice.text.contains("cancelled"));
    assert!(s.project.is_none());
    assert_eq!(s.open_font(None, false), OpenOutcome::Cancelled);
    assert!(s.project.is_none());
    assert!(!s.export_now(None));
    assert!(listing(&dir).is_empty(), "nothing written");

    let root = dir.join("Doc.ufo");
    assert!(s.new_font(Some(root.clone()), "Doc", "Regular", false));
    let before = tree(&root);
    s.project
        .as_mut()
        .unwrap()
        .set_width("space", 300.0)
        .unwrap();
    assert!(!s.save_as(None));
    assert!(s.notice.text.contains("cancelled"));
    assert!(!s.export_now(None));
    assert!(s.notice.text.contains("cancelled"));
    assert_eq!(
        tree(&root),
        before,
        "cancelled Save As / Export wrote nothing"
    );
    assert!(s.has_unsaved());
    assert_eq!(listing(&dir), vec!["Doc.ufo".to_string()]);
}

#[test]
fn unsaved_changes_block_replacing_the_document_until_discarded() {
    let dir = scratch("guard");
    let mut s = Session::new();
    let a = dir.join("A.ufo");
    let b = dir.join("B.ufo");
    assert!(s.new_font(Some(a.clone()), "A", "Regular", false));
    assert!(
        s.new_font(Some(b.clone()), "B", "Regular", false),
        "clean document is replaced freely"
    );
    s.project
        .as_mut()
        .unwrap()
        .set_width("space", 260.0)
        .unwrap();
    assert_eq!(s.open_font(Some(a.clone()), false), OpenOutcome::Blocked);
    assert_eq!(s.notice.level, Level::Warning);
    assert_eq!(s.project.as_ref().unwrap().root(), b.as_path());
    assert!(!s.close(false));
    assert_eq!(s.open_font(Some(a.clone()), true), OpenOutcome::Opened);
    assert_eq!(s.project.as_ref().unwrap().root(), a.as_path());
    assert!(
        fontstrike::FontProject::open(&b)
            .unwrap()
            .width("space")
            .unwrap()
            == 250.0,
        "discarded edit never saved"
    );
}

#[test]
fn save_as_copies_unmodelled_files_and_switches_document() {
    let dir = scratch("saveas");
    let src = dir.join("Rich.ufo");
    rich_ufo(&src);
    let mut s = Session::new();
    assert_eq!(s.open_font(Some(src.clone()), false), OpenOutcome::Opened);
    s.project.as_mut().unwrap().set_width("A", 640.0).unwrap();
    let copy = dir.join("Copy");
    assert!(s.save_as(Some(copy.clone())));
    let copy = dir.join("Copy.ufo");
    assert_eq!(s.project.as_ref().unwrap().root(), copy.as_path());
    assert!(copy.join("com.example.unknown-root-file.txt").exists());
    assert_eq!(
        fontstrike::FontProject::open(&copy)
            .unwrap()
            .width("A")
            .unwrap(),
        640.0
    );
    assert_eq!(
        fontstrike::FontProject::open(&src)
            .unwrap()
            .width("A")
            .unwrap(),
        600.0,
        "original untouched"
    );
    assert!(
        !s.save_as(Some(src)),
        "Save As refuses to overwrite an existing UFO"
    );
}

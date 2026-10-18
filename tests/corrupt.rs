//! Corrupt or unsupported sources are refused with a message, untouched.

mod common;
use common::*;
use fontstrike::FontProject;
use fontstrike::session::{Level, OpenOutcome, Session};
use std::fs;
use std::path::Path;

fn assert_refused_untouched(root: &Path, expect: &str) {
    let before = if root.exists() {
        tree(root)
    } else {
        Vec::new()
    };
    let err = match FontProject::open(root) {
        Ok(_) => panic!("{} unexpectedly opened", root.display()),
        Err(e) => e.to_string(),
    };
    assert!(
        err.to_lowercase().contains(&expect.to_lowercase()),
        "{}: {err}",
        root.display()
    );
    let after = if root.exists() {
        tree(root)
    } else {
        Vec::new()
    };
    assert_eq!(
        before,
        after,
        "refused open must not modify {}",
        root.display()
    );
    // No staging/backup/journal folders were created next to it.
    let parent = root.parent().unwrap();
    assert!(
        listing(parent).iter().all(|n| !n.contains(".fontstrike-")),
        "{:?}",
        listing(parent)
    );
}

#[test]
fn corrupt_and_unsupported_ufos_are_refused_without_changes() {
    let dir = scratch("corrupt");

    let missing_meta = dir.join("NoMeta.ufo");
    rich_ufo(&missing_meta);
    fs::remove_file(missing_meta.join("metainfo.plist")).unwrap();
    assert_refused_untouched(&missing_meta, "metainfo.plist is missing");

    let garbage_meta = dir.join("GarbageMeta.ufo");
    rich_ufo(&garbage_meta);
    fs::write(garbage_meta.join("metainfo.plist"), b"\x00\x01 not a plist").unwrap();
    assert_refused_untouched(&garbage_meta, "metainfo.plist");

    let future = dir.join("Future.ufo");
    rich_ufo(&future);
    write(
        &future.join("metainfo.plist"),
        &plist(
            "<dict><key>creator</key><string>x</string><key>formatVersion</key><integer>4</integer></dict>",
        ),
    );
    assert_refused_untouched(&future, "newer than FontStrike supports");

    let bad_glif = dir.join("BadGlif.ufo");
    rich_ufo(&bad_glif);
    fs::write(
        bad_glif.join("glyphs/A_.glif"),
        b"<?xml version=\"1.0\"?><glyph name=\"A\" format=\"2\"><advance width=\"600\"><outline>",
    )
    .unwrap();
    assert_refused_untouched(&bad_glif, "could not read UFO");

    let unknown_element = dir.join("UnknownElement.ufo");
    rich_ufo(&unknown_element);
    fs::write(unknown_element.join("glyphs/A_.glif"), b"<?xml version=\"1.0\"?><glyph name=\"A\" format=\"2\"><advance width=\"600\"/><futureElement/></glyph>").unwrap();
    assert_refused_untouched(&unknown_element, "could not read UFO");

    let missing_glif = dir.join("MissingGlif.ufo");
    rich_ufo(&missing_glif);
    fs::remove_file(missing_glif.join("glyphs/o.glif")).unwrap();
    assert_refused_untouched(&missing_glif, "could not read UFO");

    let truncated_contents = dir.join("TruncatedContents.ufo");
    rich_ufo(&truncated_contents);
    fs::write(
        truncated_contents.join("glyphs/contents.plist"),
        b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>A</key>",
    )
    .unwrap();
    assert_refused_untouched(&truncated_contents, "could not read UFO");

    let wrong_type = dir.join("WrongType.ufo");
    rich_ufo(&wrong_type);
    write(
        &wrong_type.join("fontinfo.plist"),
        &plist("<dict><key>unitsPerEm</key><string>a thousand</string></dict>"),
    );
    assert_refused_untouched(&wrong_type, "could not read UFO");

    let ufoz = dir.join("Packed.ufoz");
    fs::write(&ufoz, b"PK\x03\x04 zipped ufo").unwrap();
    assert!(
        FontProject::open(&ufoz)
            .unwrap_err()
            .to_string()
            .contains("not a UFO folder")
    );

    let missing = dir.join("DoesNotExist.ufo");
    assert!(FontProject::open(&missing).is_err());
    assert!(!missing.exists());
}

#[test]
fn failed_open_keeps_the_current_document() {
    let dir = scratch("failed-open");
    let good = dir.join("Good.ufo");
    let mut session = Session::new();
    assert!(session.new_font(Some(good.clone()), "Good", "Regular", false));
    let bad = dir.join("Bad.ufo");
    fs::create_dir_all(&bad).unwrap();
    assert_eq!(session.open_font(Some(bad), false), OpenOutcome::Failed);
    assert_eq!(session.notice.level, Level::Error);
    assert_eq!(session.project.as_ref().unwrap().root(), good.as_path());
}

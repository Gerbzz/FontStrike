use fontstrike::{
    FontProject,
    vector_path::{Anchor, PathData, SubPath},
};
use kurbo::Point;

fn sample_path() -> PathData {
    PathData::single(SubPath::new(
        vec![
            Anchor::with_handles(
                Point::new(100.0, -120.0),
                Point::new(75.0, -140.0),
                Point::new(140.0, -160.0),
            ),
            Anchor::with_handles(
                Point::new(420.0, -700.0),
                Point::new(390.0, -650.0),
                Point::new(440.0, -710.0),
            ),
            Anchor::corner(Point::new(500.0, -100.0)),
        ],
        true,
    ))
}

#[test]
fn asymmetric_cubic_width_and_kerning_survive_ufo_roundtrip() {
    let path = std::env::temp_dir().join(format!("fontstrike-source-{}.ufo", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let mut project = FontProject::create(&path, "Strike Test", "Regular").unwrap();
    project
        .set_outline("A", Some('A'), 620.0, &sample_path())
        .unwrap();
    project
        .set_outline("V", Some('V'), 600.0, &sample_path())
        .unwrap();
    project.set_kerning("A", "V", -80.0).unwrap();
    project.save().unwrap();

    let reopened = FontProject::open(&path).unwrap();
    assert_eq!(reopened.width("A").unwrap(), 620.0);
    assert_eq!(reopened.kerning("A", "V"), Some(-80.0));
    let mut actual = reopened.outline("A").unwrap();
    let expected = sample_path();
    assert!(actual.subpaths[0].closed);
    let first = expected.subpaths[0].anchors[0].p;
    let rotation = actual.subpaths[0]
        .anchors
        .iter()
        .position(|a| a.p == first)
        .unwrap();
    actual.subpaths[0].anchors.rotate_left(rotation);
    for (a, b) in actual.subpaths[0]
        .anchors
        .iter()
        .zip(&expected.subpaths[0].anchors)
    {
        assert!((a.p.x - b.p.x).abs() < 1e-6);
        assert!((a.p.y - b.p.y).abs() < 1e-6);
        assert!((a.h_in.x - b.h_in.x).abs() < 1e-6);
        assert!((a.h_in.y - b.h_in.y).abs() < 1e-6);
        assert!((a.h_out.x - b.h_out.x).abs() < 1e-6);
        assert!((a.h_out.y - b.h_out.y).abs() < 1e-6);
    }
    assert!(path.join("glyphs").join("contents.plist").exists());
}

#[test]
fn undo_and_invalid_open_outline_keep_source_intact() {
    let path = std::env::temp_dir().join(format!("fontstrike-undo-{}.ufo", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let mut project = FontProject::create(&path, "Strike Test", "Regular").unwrap();
    project
        .set_outline("A", Some('A'), 620.0, &sample_path())
        .unwrap();
    let changed = PathData::single(SubPath::new(
        vec![
            Anchor::corner(Point::new(0.0, 0.0)),
            Anchor::corner(Point::new(200.0, 0.0)),
        ],
        false,
    ));
    assert!(
        project
            .set_outline("A", Some('A'), 400.0, &changed)
            .is_err()
    );
    assert_eq!(project.width("A").unwrap(), 620.0);
    project.set_width("A", 650.0).unwrap();
    assert!(project.can_undo());
    assert!(!project.can_redo());
    project.undo().unwrap();
    assert!(project.can_redo());
    assert_eq!(project.width("A").unwrap(), 620.0);
    project.redo().unwrap();
    assert!(!project.can_redo());
    assert_eq!(project.width("A").unwrap(), 650.0);
}

#[test]
fn external_ufo_changes_after_open_and_unknown_root_files_are_preserved() {
    let path = std::env::temp_dir().join(format!("fontstrike-external-{}.ufo", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let mut project = FontProject::create(&path, "Conflict Fixture", "Regular").unwrap();
    let data = path.join("data").join("external.txt");
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::write(&data, b"external data added after open").unwrap();
    project.set_width("space", 251.0).unwrap();
    assert!(project.save().unwrap_err().to_string().contains("changed outside"));
    assert_eq!(std::fs::read(&data).unwrap(), b"external data added after open");
    assert_eq!(FontProject::open(&path).unwrap().width("space").unwrap(), 250.0);

    let unrelated = path.join("unrelated-root.txt");
    std::fs::write(&unrelated, b"preserve unrelated file").unwrap();
    let mut reopened = FontProject::open(&path).unwrap();
    reopened.set_width("space", 252.0).unwrap();
    assert!(reopened.save().unwrap_err().to_string().contains("would be lost"));
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"preserve unrelated file");
    assert_eq!(std::fs::read(&data).unwrap(), b"external data added after open");
    assert_eq!(FontProject::open(&path).unwrap().width("space").unwrap(), 250.0);
}

//! Opening an external UFO, editing it, and saving must not lose data.

mod common;
use common::*;
use fontstrike::norad::{Font, PointType};
use fontstrike::{FontProject, PointId};
use std::fs;

#[test]
fn edit_and_save_of_rich_external_ufo_preserves_everything_else() {
    let dir = scratch("rich");
    let root = dir.join("Rich.ufo");
    rich_ufo(&root);
    let before = Font::load(&root).unwrap();

    let mut project = FontProject::open(&root).unwrap();
    let notes = project.notes().join("\n");
    assert!(
        notes.contains("com.example.unknown-root-file.txt"),
        "{notes}"
    );
    assert!(notes.contains("com.example.futureFontinfoKey"), "{notes}");
    assert!(notes.contains("public.background"), "{notes}");
    assert!(notes.contains("components"), "{notes}");
    assert!(notes.contains("quadratic"), "{notes}");
    assert!(notes.contains("features.fea"), "{notes}");

    // Edit one point of A and the width of o.
    project
        .move_points("A", &[PointId::new(0, 1)], 10.0, 0.0)
        .unwrap();
    project.set_width("o", 530.0).unwrap();
    project.save().unwrap();

    let after = Font::load(&root).unwrap();
    // Everything not edited is semantically identical.
    assert_eq!(after.lib, before.lib);
    assert_eq!(after.groups, before.groups);
    assert_eq!(after.kerning, before.kerning);
    assert_eq!(after.features, before.features);
    assert_eq!(after.font_info, before.font_info);
    assert_eq!(after.layers.len(), 2);
    assert_eq!(
        after
            .layers
            .get("public.background")
            .unwrap()
            .get_glyph("A"),
        before
            .layers
            .get("public.background")
            .unwrap()
            .get_glyph("A")
    );
    for name in [".notdef", "space", "Aacute", "acutecomb"] {
        assert_eq!(
            after.default_layer().get_glyph(name),
            before.default_layer().get_glyph(name),
            "{name}"
        );
    }
    let a0 = before.default_layer().get_glyph("A").unwrap();
    let a1 = after.default_layer().get_glyph("A").unwrap();
    assert_eq!(
        a1.contours[0].points[1].x,
        a0.contours[0].points[1].x + 10.0
    );
    // Identifiers, names, anchors, guidelines, note, lib survive the edit.
    assert_eq!(a1.contours[0].points[0].name.as_deref(), Some("bottomLeft"));
    assert_eq!(
        a1.contours[0].points[0].identifier().map(|i| i.as_str()),
        Some("pt1")
    );
    assert_eq!(
        a1.contours[0].identifier().map(|i| i.as_str()),
        Some("contour1")
    );
    assert_eq!(
        (
            a1.anchors.clone(),
            a1.guidelines.clone(),
            a1.note.clone(),
            a1.lib.clone()
        ),
        (
            a0.anchors.clone(),
            a0.guidelines.clone(),
            a0.note.clone(),
            a0.lib.clone()
        )
    );
    // qcurve contour is untouched and still quadratic.
    let o = after.default_layer().get_glyph("o").unwrap();
    assert_eq!(o.width, 530.0);
    assert!(
        o.contours[0]
            .points
            .iter()
            .any(|p| p.typ == PointType::QCurve)
    );
    assert_eq!(
        o.contours,
        before.default_layer().get_glyph("o").unwrap().contours
    );

    // Unmodelled files are byte-identical; unknown plist keys survive.
    for rel in [
        "com.example.unknown-root-file.txt",
        "com.example.unknown-dir/inner.bin",
        "glyphs/README-unknown.txt",
        "data/com.example.tool/settings.json",
    ] {
        let expected = match rel {
            "com.example.unknown-root-file.txt" => "unknown root file\n",
            "com.example.unknown-dir/inner.bin" => "binary-ish payload\n",
            "glyphs/README-unknown.txt" => "unknown file inside the glyph folder\n",
            _ => "{\"keep\": true}\n",
        };
        assert_eq!(
            fs::read_to_string(root.join(rel)).unwrap(),
            expected,
            "{rel}"
        );
    }
    let fontinfo = fs::read_to_string(root.join("fontinfo.plist")).unwrap();
    assert!(fontinfo.contains("com.example.futureFontinfoKey") && fontinfo.contains("keep me"));
    let layerinfo = fs::read_to_string(root.join("glyphs/layerinfo.plist")).unwrap();
    assert!(layerinfo.contains("com.example.layerKey") && layerinfo.contains("layer data"));

    // A second save after reopening is stable (idempotent).
    let mut again = FontProject::open(&root).unwrap();
    let snapshot = tree(&root);
    again.set_width("o", 530.0).unwrap(); // no-op edit
    assert!(!again.is_dirty());
    again.set_width("o", 531.0).unwrap();
    again.undo().unwrap();
    assert!(!again.is_dirty(), "undo back to the saved state is clean");
    again.set_width("o", 531.0).unwrap();
    again.save().unwrap();
    again.set_width("o", 530.0).unwrap();
    again.save().unwrap();
    assert_eq!(
        tree(&root),
        snapshot,
        "save → edit → revert → save reproduces identical bytes"
    );
}

#[test]
fn group_kerning_and_components_are_resolved() {
    let dir = scratch("groups");
    let root = dir.join("Rich.ufo");
    rich_ufo(&root);
    let mut project = FontProject::open(&root).unwrap();
    assert_eq!(
        project.effective_kerning("A", "o"),
        -55.0,
        "glyph pair beats group pair"
    );
    assert_eq!(
        project.effective_kerning("Aacute", "o"),
        -40.0,
        "group pair applies to members"
    );
    project
        .set_kerning("public.kern1.A", "public.kern2.o", -45.0)
        .unwrap();
    assert_eq!(project.effective_kerning("Aacute", "o"), -45.0);
    assert!(
        project.set_kerning("public.kern2.o", "A", -10.0).is_err(),
        "kern2 group on the left is refused"
    );
    let bounds = project.glyph_bounds("Aacute").unwrap();
    assert!(
        bounds.y1 >= 840.0,
        "component accent is included: {bounds:?}"
    );
    // Renaming A updates kerning, groups, components, and glyph order.
    project.rename_glyph("A", "A.alt").unwrap();
    assert_eq!(project.kerning("A.alt", "o"), Some(-55.0));
    let font = project.font();
    assert!(
        font.groups["public.kern1.A"]
            .iter()
            .any(|n| n.as_str() == "A.alt")
    );
    assert_eq!(
        font.default_layer().get_glyph("Aacute").unwrap().components[0]
            .base
            .as_str(),
        "A.alt"
    );
    assert!(
        font.layers
            .get("public.background")
            .unwrap()
            .get_glyph("A.alt")
            .is_some()
    );
    assert!(project.glyph_names().iter().position(|n| n == "A.alt") == Some(2));
    project.undo().unwrap();
    assert_eq!(project.kerning("A", "o"), Some(-55.0));
    assert!(
        project
            .font()
            .layers
            .get("public.background")
            .unwrap()
            .get_glyph("A")
            .is_some()
    );
    // A glyph used as a component cannot be deleted.
    assert!(
        project
            .delete_glyph("acutecomb")
            .unwrap_err()
            .to_string()
            .contains("component")
    );
}

#[test]
fn ufo2_source_is_upgraded_on_save_with_a_note() {
    let dir = scratch("ufo2");
    let root = dir.join("Old.ufo");
    ufo2(&root);
    let mut project = FontProject::open(&root).unwrap();
    assert!(project.notes().iter().any(|n| n.contains("UFO 2")));
    assert_eq!(project.effective_kerning("A", "A"), -20.0);
    project.set_width("A", 610.0).unwrap();
    project.save().unwrap();
    let meta = fs::read_to_string(root.join("metainfo.plist")).unwrap();
    assert!(meta.contains("<integer>3</integer>"));
    let reopened = FontProject::open(&root).unwrap();
    assert_eq!(reopened.width("A").unwrap(), 610.0);
    assert_eq!(reopened.effective_kerning("A", "A"), -20.0);
    assert!(
        reopened
            .kerning_groups()
            .iter()
            .any(|g| g.starts_with("public.kern1."))
    );
}

#[test]
fn external_change_after_open_is_refused_and_not_overwritten() {
    let dir = scratch("conflict");
    let root = dir.join("Conflict.ufo");
    let mut project = FontProject::create(&root, "Conflict Fixture", "Regular").unwrap();
    let data = root.join("data").join("external.txt");
    fs::create_dir_all(data.parent().unwrap()).unwrap();
    fs::write(&data, b"external data added after open").unwrap();
    project.set_width("space", 251.0).unwrap();
    let err = project.save().unwrap_err().to_string();
    assert!(err.contains("changed on disk"), "{err}");
    assert_eq!(fs::read(&data).unwrap(), b"external data added after open");
    assert_eq!(
        FontProject::open(&root).unwrap().width("space").unwrap(),
        250.0
    );
    assert!(project.is_dirty(), "the refused edit is still in memory");
}

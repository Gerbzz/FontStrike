//! Every edit type is undoable and redoable, and survives save/reopen.

mod common;
use common::*;
use fontstrike::norad::{Font, PointType};
use fontstrike::{FontMetrics, FontProject, PointId};

type Edit = (&'static str, fn(&mut FontProject));

fn snapshot(p: &FontProject) -> Font {
    p.font().clone()
}

fn assert_same(a: &Font, b: &Font, what: &str) {
    assert_eq!(a.font_info, b.font_info, "{what}: info");
    assert_eq!(a.kerning, b.kerning, "{what}: kerning");
    assert_eq!(a.groups, b.groups, "{what}: groups");
    assert_eq!(a.lib, b.lib, "{what}: lib");
    let names = |f: &Font| {
        f.default_layer()
            .iter()
            .map(|g| g.name().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(a), names(b), "{what}: glyph set");
    for g in a.default_layer().iter() {
        assert_eq!(
            Some(g),
            b.default_layer().get_glyph(g.name()),
            "{what}: glyph {}",
            g.name()
        );
    }
}

#[test]
fn each_edit_type_undoes_and_redoes_exactly() {
    let dir = scratch("undo-all");
    let root = dir.join("Undo.ufo");
    let mut p = fontstrike::demo::build(&root).unwrap();
    let edits: Vec<Edit> = vec![
        ("add glyph", |p| p.add_glyph("B", &['B'], 600.0).unwrap()),
        ("rename", |p| p.rename_glyph("B", "B.alt").unwrap()),
        ("delete glyph", |p| p.delete_glyph("B.alt").unwrap()),
        ("unicode", |p| p.set_codepoints("H", &['H', 'h']).unwrap()),
        ("width", |p| p.set_width("H", 700.0).unwrap()),
        ("sidebearings", |p| {
            p.set_sidebearings("H", Some(90.0), Some(40.0)).unwrap()
        }),
        ("move points", |p| {
            p.move_points("A", &[PointId::new(0, 0), PointId::new(0, 1)], 5.0, -3.0)
                .unwrap()
        }),
        ("set point", |p| {
            p.set_point("o", PointId::new(0, 0), 290.0, -20.0).unwrap()
        }),
        ("insert point", |p| {
            p.insert_point("T", 0, 1, 0.5).unwrap();
        }),
        ("delete point", |p| {
            p.delete_points("H", &[PointId::new(0, 3)]).unwrap()
        }),
        ("smooth", |p| {
            p.toggle_smooth("A", PointId::new(0, 2)).unwrap()
        }),
        ("to curve", |p| p.convert_segment("n", 0, 2, true).unwrap()),
        ("add rectangle", |p| {
            p.add_rectangle("n", 10.0, 10.0, 20.0, 20.0).unwrap();
        }),
        ("add ellipse", |p| {
            p.add_ellipse("n", 10.0, 10.0, 60.0, 60.0).unwrap();
        }),
        ("delete contour", |p| p.delete_contour("A", 1).unwrap()),
        ("reverse contour", |p| p.reverse_contour("o", 1).unwrap()),
        ("kerning set", |p| p.set_kerning("H", "o", -15.0).unwrap()),
        ("kerning change", |p| {
            p.set_kerning("A", "V", -95.0).unwrap()
        }),
        ("kerning remove", |p| p.remove_kerning("T", "o").unwrap()),
        ("metrics", |p| {
            let mut m = p.metrics();
            m.ascender = 820.0;
            m.family = "Renamed Test Sans".into();
            p.set_metrics(&m).unwrap()
        }),
    ];
    for (what, edit) in edits {
        let before = snapshot(&p);
        let was_dirty = p.is_dirty();
        edit(&mut p);
        let after = snapshot(&p);
        assert!(p.is_dirty(), "{what} marks dirty");
        assert!(p.can_undo());
        p.undo().unwrap();
        assert_same(&p.font().clone(), &before, &format!("{what} undo"));
        assert_eq!(p.is_dirty(), was_dirty, "{what}: undo restores dirty state");
        p.redo().unwrap();
        assert_same(&p.font().clone(), &after, &format!("{what} redo"));
        // Saved and reopened, the edit is still there.
        p.save().unwrap();
        let reopened = FontProject::open(&root).unwrap();
        assert_same(reopened.font(), &after, &format!("{what} reopen"));
    }
    // A new edit after undo clears redo.
    p.undo().unwrap();
    assert!(p.can_redo());
    p.set_width("space", 260.0).unwrap();
    assert!(!p.can_redo());
}

#[test]
fn gestures_group_a_drag_into_one_step_and_can_be_cancelled() {
    let dir = scratch("gesture");
    let mut p = fontstrike::demo::build(&dir.join("G.ufo")).unwrap();
    let start = snapshot(&p);
    let depth = p.can_undo();
    p.begin_gesture("Drag point");
    for _ in 0..10 {
        p.move_points("A", &[PointId::new(0, 0)], 2.0, 1.0).unwrap();
    }
    p.end_gesture();
    assert_eq!(p.undo_label(), Some("Drag point"));
    assert_eq!(p.glyph("A").unwrap().contours[0].points[0].x, 50.0);
    p.undo().unwrap();
    assert_same(&p.font().clone(), &start, "gesture undo");
    assert_eq!(p.can_undo(), depth);

    p.begin_gesture("Drag point");
    p.move_points("A", &[PointId::new(0, 0)], 30.0, 0.0)
        .unwrap();
    p.cancel_gesture();
    assert_same(&p.font().clone(), &start, "cancelled gesture");
    assert!(!p.can_redo() || p.redo_label() == Some("Drag point"));
    p.begin_gesture("Click without moving");
    p.end_gesture();
    assert_ne!(p.undo_label(), Some("Click without moving"));
}

#[test]
fn point_editing_semantics() {
    let dir = scratch("points");
    let mut p = fontstrike::demo::build(&dir.join("P.ufo")).unwrap();
    // Moving an on-curve point carries its handles.
    let o = p.glyph("o").unwrap().contours[0].clone();
    p.move_points("o", &[PointId::new(0, 3)], 10.0, 0.0)
        .unwrap();
    let moved = &p.glyph("o").unwrap().contours[0];
    for i in [2, 3, 4] {
        assert_eq!(moved.points[i].x, o.points[i].x + 10.0, "point {i}");
    }
    assert_eq!(moved.points[5].x, o.points[5].x);
    // Moving a handle moves only the handle.
    p.move_points("o", &[PointId::new(0, 4)], 0.0, 7.0).unwrap();
    assert_eq!(p.glyph("o").unwrap().contours[0].points[3].y, o.points[3].y);
    // Insert on a curve keeps a smooth on-curve point.
    let id = p.insert_point("o", 0, 3, 0.5).unwrap();
    let pt = &p.glyph("o").unwrap().contours[0].points[id.point];
    assert_eq!(pt.typ, PointType::Curve);
    assert!(pt.smooth);
    // Pen tool: open contour, append, close.
    let c = p.start_contour("H", 300.0, 800.0).unwrap();
    p.append_point("H", c, 400.0, 800.0).unwrap();
    assert!(p.close_contour("H", c).is_err(), "needs three points");
    p.append_point("H", c, 350.0, 900.0).unwrap();
    p.close_contour("H", c).unwrap();
    assert!(p.glyph("H").unwrap().contours[c].is_closed());
    // Deleting down to fewer than three on-curve points removes the contour.
    let count = p.glyph("H").unwrap().contours.len();
    p.delete_points("H", &[PointId::new(c, 0), PointId::new(c, 1)])
        .unwrap();
    assert_eq!(p.glyph("H").unwrap().contours.len(), count - 1);
    // Invalid input is refused without changing anything.
    let before = snapshot(&p);
    assert!(p.set_width("H", f64::NAN).is_err());
    assert!(p.set_point("H", PointId::new(0, 0), 1e9, 0.0).is_err());
    assert!(p.add_glyph("A", &[], 500.0).is_err());
    assert!(
        p.add_glyph("B", &['A'], 500.0)
            .unwrap_err()
            .to_string()
            .contains("already assigned")
    );
    assert!(p.rename_glyph("A", "public.kern1.x").is_err());
    assert!(p.set_kerning("A", "Nope", 10.0).is_err());
    assert!(
        p.set_metrics(&FontMetrics {
            ascender: -500.0,
            ..p.metrics()
        })
        .is_err()
    );
    assert_same(&p.font().clone(), &before, "refused edits");
}

#[test]
fn sidebearings_shift_outline_and_width() {
    let dir = scratch("sb");
    let mut p = fontstrike::demo::build(&dir.join("S.ufo")).unwrap();
    assert_eq!(p.sidebearings("H"), Some((70.0, 70.0)));
    p.set_sidebearings("H", Some(100.0), None).unwrap();
    assert_eq!(p.sidebearings("H"), Some((100.0, 70.0)));
    assert_eq!(p.width("H").unwrap(), 690.0);
    p.set_sidebearings("H", None, Some(20.0)).unwrap();
    assert_eq!(p.width("H").unwrap(), 640.0);
    assert!(
        p.set_sidebearings("space", Some(10.0), None).is_err(),
        "empty glyph has no bearings"
    );
}

mod common;
use fontstrike::FontProject;
use fontstrike::vector_path::{Anchor, PathData, SubPath};
use kurbo::Point;
use ttf_parser::{Face, OutlineBuilder};

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> PathData {
    PathData::single(SubPath::new(
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            .into_iter()
            .map(|(x, y)| Anchor::corner(Point::new(x, -y)))
            .collect(),
        true,
    ))
}

#[derive(Default)]
struct OutlineProbe {
    moves: usize,
    lines: usize,
    closes: usize,
}
impl OutlineBuilder for OutlineProbe {
    fn move_to(&mut self, _: f32, _: f32) {
        self.moves += 1;
    }
    fn line_to(&mut self, _: f32, _: f32) {
        self.lines += 1;
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.lines += 1;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.lines += 1;
    }
    fn close(&mut self) {
        self.closes += 1;
    }
}

#[test]
fn compiled_ttf_readback_preserves_glyph_outlines_advances_and_pair_kerning() {
    let root = std::env::temp_dir().join(format!("fontstrike-export-{}.ufo", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut project = FontProject::create(&root, "Fixture Sans", "Regular").unwrap();
    project
        .set_outline(".notdef", None, 600.0, &rectangle(60.0, 0.0, 520.0, 700.0))
        .unwrap();
    for (name, width, x0, x1) in [
        ("A", 620.0, 100.0, 520.0),
        ("V", 600.0, 80.0, 520.0),
        ("T", 590.0, 60.0, 530.0),
        ("o", 530.0, 70.0, 470.0),
    ] {
        project
            .set_outline(
                name,
                name.chars().next(),
                width,
                &rectangle(x0, 0.0, x1, 700.0),
            )
            .unwrap();
    }
    project.set_kerning("A", "V", -80.0).unwrap();
    project.set_kerning("T", "o", -40.0).unwrap();
    project.save().unwrap();
    let output = root.with_extension("ttf");
    project.export_ttf(&output).unwrap();
    let bytes = std::fs::read(&output).unwrap();
    let face = Face::parse(&bytes, 0).unwrap();
    assert_eq!(face.units_per_em(), 1000);
    assert!(face.tables().cmap.is_some());
    assert!(face.tables().glyf.is_some());
    assert!(face.tables().hmtx.is_some());
    assert!(face.tables().gpos.is_some());
    assert!(face.tables().name.is_some());
    for (character, width) in [('A', 620), ('V', 600), ('T', 590), ('o', 530), (' ', 250)] {
        let glyph = face.glyph_index(character).unwrap();
        assert_eq!(face.glyph_hor_advance(glyph), Some(width));
    }
    let mut outline = OutlineProbe::default();
    let bounds = face
        .outline_glyph(face.glyph_index('A').unwrap(), &mut outline)
        .unwrap();
    assert_eq!(
        (bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max),
        (100, 0, 520, 700)
    );
    assert!(outline.moves >= 1 && outline.lines >= 3 && outline.closes >= 1);
    let shape_face = rustybuzz::Face::from_slice(&bytes, 0).unwrap();
    for (text, expected) in [("AV", 620 + 600 - 80), ("To", 590 + 530 - 40)] {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(&shape_face, &[], buffer);
        let advance: i32 = glyphs.glyph_positions().iter().map(|p| p.x_advance).sum();
        assert_eq!(advance, expected, "{text}");
    }
}

#[test]
fn synthetic_test_font_exports_curves_and_shaped_kerning() {
    let dir = common::scratch("export-demo");
    let root = dir.join("Demo.ufo");
    let project = fontstrike::demo::build(&root).unwrap();
    let ttf = dir.join("Demo.ttf");
    let report = project.export_ttf(&ttf).unwrap();
    assert_eq!(report.mappings_checked, 7);
    assert_eq!(report.outlines_checked, 6);
    assert_eq!(report.kerning_pairs_checked, 3);
    let bytes = std::fs::read(&ttf).unwrap();
    let face = rustybuzz::Face::from_slice(&bytes, 0).unwrap();
    let advance = |text: &str| -> i32 {
        let mut b = rustybuzz::UnicodeBuffer::new();
        b.push_str(text);
        b.guess_segment_properties();
        rustybuzz::shape(&face, &[], b)
            .glyph_positions()
            .iter()
            .map(|p| p.x_advance)
            .sum()
    };
    assert_eq!(advance("AV"), 640 + 620 - 80);
    assert_eq!(advance("VA"), 620 + 640 - 70);
    assert_eq!(advance("To"), 580 + 560 - 60);
    assert_eq!(advance("HH"), 660 * 2, "unkerned pair is unchanged");
}

#[test]
fn rich_external_source_with_groups_components_and_qcurves_exports() {
    let dir = common::scratch("export-rich");
    let root = dir.join("Rich.ufo");
    common::rich_ufo(&root);
    let project = FontProject::open(&root).unwrap();
    let ttf = dir.join("Rich.ttf");
    let report = project.export_ttf(&ttf).unwrap();
    assert!(report.kerning_pairs_checked >= 2, "{report:?}");
    let bytes = std::fs::read(&ttf).unwrap();
    let face = rustybuzz::Face::from_slice(&bytes, 0).unwrap();
    let advance = |text: &str| -> i32 {
        let mut b = rustybuzz::UnicodeBuffer::new();
        b.push_str(text);
        b.guess_segment_properties();
        rustybuzz::shape(&face, &[], b)
            .glyph_positions()
            .iter()
            .map(|p| p.x_advance)
            .sum()
    };
    assert_eq!(advance("Ao"), 600 + 520 - 55, "glyph exception wins");
    assert_eq!(
        advance("\u{00C1}o"),
        600 + 520 - 40,
        "group kerning applies to Aacute"
    );
}

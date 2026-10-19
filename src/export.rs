//! TrueType export through fontc, followed by an independent readback check
//! (ttf-parser for tables/metrics/outlines, rustybuzz for shaped kerning).

use crate::outline;
use crate::storage::write_model;
use crate::{Error, Result};
use norad::Font;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// What the post-compile verification covered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportReport {
    pub path: PathBuf,
    pub bytes: usize,
    pub glyphs_in_font: u16,
    pub mappings_checked: usize,
    pub outlines_checked: usize,
    pub kerning_pairs_checked: usize,
    pub kerning_pairs_skipped: usize,
    pub notes: Vec<String>,
}

impl ExportReport {
    pub fn summary(&self) -> String {
        format!(
            "{} glyphs, {} mappings and advances, {} outline bounds, {} shaped kerning pairs verified",
            self.glyphs_in_font,
            self.mappings_checked,
            self.outlines_checked,
            self.kerning_pairs_checked
        )
    }
}

/// A self-contained export (owns a copy of the font, so it can run on a
/// background thread while the editor stays responsive).
#[derive(Clone)]
pub struct ExportJob {
    pub(crate) font: Font,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!(
        "{tag}-{}-{}-{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

const MAX_KERN_CHECKS: usize = 600;

impl ExportJob {
    pub fn new(font: Font) -> Self {
        Self { font }
    }

    /// Compiles the font and verifies the bytes before atomically replacing
    /// `destination`. On any failure the destination is untouched.
    pub fn run(&self, destination: &Path) -> Result<ExportReport> {
        if destination
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase)
            != Some("ttf".into())
        {
            return Err(Error::new("TTF export path must end in .ttf"));
        }
        let dir = match destination.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        if !dir.is_dir() {
            return Err(Error::new(format!(
                "export folder {} does not exist",
                dir.display()
            )));
        }
        if destination.is_dir() {
            return Err(Error::new("export path is a folder"));
        }
        let bytes = self.compile()?;
        let mut report = verify(&self.font, &bytes)?;
        report.path = destination.to_path_buf();
        report.bytes = bytes.len();
        let file_name = destination
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(".{file_name}.{}.tmp", unique("fontstrike")));
        let write = (|| -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, destination)
        })();
        if let Err(e) = write {
            let _ = fs::remove_file(&tmp);
            return Err(Error::new(format!("write TTF: {e}")));
        }
        Ok(report)
    }

    /// Compiles to bytes without writing the destination.
    pub fn compile(&self) -> Result<Vec<u8>> {
        if self.font.default_layer().get_glyph(".notdef").is_none() {
            return Err(Error::new(
                "add a .notdef glyph before exporting (fonts must start with it)",
            ));
        }
        let temp = std::env::temp_dir().join(format!("{}.ufo", unique("fontstrike-export")));
        let result = (|| {
            write_model(&self.font, &temp)?;
            let source = fontc::Input::new(&temp)
                .and_then(|input| input.create_source())
                .map_err(|e| Error::new(format!("font source: {e}")))?;
            fontc::generate_font(source, fontc::Options::default())
                .map_err(|e| Error::new(format!("TTF compile: {e}")))
        })();
        let _ = fs::remove_dir_all(&temp);
        result
    }
}

fn has_kern_feature(features: &str) -> bool {
    features
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .any(|l| {
            let mut words = l.split_whitespace();
            matches!(
                (words.next(), words.next()),
                (Some("feature"), Some("kern"))
            )
        })
}

/// Independently reads the compiled bytes back and compares them with the
/// source model.
pub fn verify(font: &Font, bytes: &[u8]) -> Result<ExportReport> {
    let face = ttf_parser::Face::parse(bytes, 0)
        .map_err(|e| Error::new(format!("compiled TTF readback: {e}")))?;
    let mut report = ExportReport {
        glyphs_in_font: face.number_of_glyphs(),
        ..Default::default()
    };
    let upm = font
        .font_info
        .units_per_em
        .map(|u| u.as_f64().round() as u16)
        .unwrap_or(1000);
    if face.units_per_em() != upm {
        return Err(Error::new(format!(
            "compiled unitsPerEm {} differs from source {upm}",
            face.units_per_em()
        )));
    }
    let t = face.tables();
    let missing: Vec<&str> = [
        ("cmap", t.cmap.is_some()),
        ("glyf", t.glyf.is_some()),
        ("hmtx", t.hmtx.is_some()),
        ("name", t.name.is_some()),
        ("OS/2", t.os2.is_some()),
        ("post", t.post.is_some()),
    ]
    .into_iter()
    .filter(|(_, ok)| !ok)
    .map(|(n, _)| n)
    .collect();
    if !missing.is_empty() {
        return Err(Error::new(format!(
            "compiled TTF lacks tables: {}",
            missing.join(", ")
        )));
    }
    if let Some(family) = font.font_info.family_name.as_deref() {
        let found = face.names().into_iter().any(|n| {
            (n.name_id == ttf_parser::name_id::FAMILY
                || n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
                && n.to_string().as_deref() == Some(family)
        });
        if !found {
            return Err(Error::new(format!(
                "compiled name table lacks family {family:?}"
            )));
        }
    }
    let layer = font.default_layer();
    for glyph in layer.iter() {
        for ch in glyph.codepoints.iter() {
            let id = face.glyph_index(ch).ok_or_else(|| {
                Error::new(format!(
                    "compiled TTF is missing U+{:04X} ({})",
                    ch as u32,
                    glyph.name()
                ))
            })?;
            let expected = glyph.width.round();
            if face.glyph_hor_advance(id).map(f64::from) != Some(expected) {
                return Err(Error::new(format!(
                    "compiled advance of {} is {:?}, source says {expected}",
                    glyph.name(),
                    face.glyph_hor_advance(id)
                )));
            }
            report.mappings_checked += 1;
            if let Some(src) = outline::bounds(font, glyph) {
                struct Sink;
                impl ttf_parser::OutlineBuilder for Sink {
                    fn move_to(&mut self, _: f32, _: f32) {}
                    fn line_to(&mut self, _: f32, _: f32) {}
                    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
                    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
                    fn close(&mut self) {}
                }
                let bb = face.outline_glyph(id, &mut Sink).ok_or_else(|| {
                    Error::new(format!("compiled glyph {} has no outline", glyph.name()))
                })?;
                // glyf boxes cover quadratic control points, which can sit a
                // little outside the tight cubic bounds.
                let tol = if outline::line_only(glyph) && glyph.components.is_empty() {
                    1.0
                } else {
                    f64::from(upm) * 0.05
                };
                let pairs = [
                    (f64::from(bb.x_min), src.x0),
                    (f64::from(bb.y_min), src.y0),
                    (f64::from(bb.x_max), src.x1),
                    (f64::from(bb.y_max), src.y1),
                ];
                if pairs.iter().any(|(a, b)| (a - b).abs() > tol) {
                    return Err(Error::new(format!(
                        "compiled bounds of {} ({},{},{},{}) differ from source ({:.0},{:.0},{:.0},{:.0})",
                        glyph.name(),
                        bb.x_min,
                        bb.y_min,
                        bb.x_max,
                        bb.y_max,
                        src.x0,
                        src.y0,
                        src.x1,
                        src.y1
                    )));
                }
                report.outlines_checked += 1;
            }
        }
    }
    // Shaped kerning check.
    if font.kerning.is_empty() {
        return Ok(report);
    }
    if has_kern_feature(&font.features) {
        report.notes.push(
            "features.fea defines its own kern feature; shaped kerning was not compared with kerning.plist".into(),
        );
        return Ok(report);
    }
    let shaper = rustybuzz::Face::from_slice(bytes, 0)
        .ok_or_else(|| Error::new("rustybuzz could not load the compiled TTF"))?;
    let resolver = font.kerning_resolver();
    let members = |name: &str| -> Vec<String> {
        match font.groups.get(name) {
            Some(list) => list.iter().take(3).map(|n| n.to_string()).collect(),
            None => vec![name.to_string()],
        }
    };
    let char_of = |name: &str| {
        layer
            .get_glyph(name)
            .and_then(|g| g.codepoints.iter().next())
    };
    'outer: for (first, row) in &font.kerning {
        for second in row.keys() {
            for l in members(first) {
                for r in members(second) {
                    if report.kerning_pairs_checked >= MAX_KERN_CHECKS {
                        report
                            .notes
                            .push(format!("kerning check capped at {MAX_KERN_CHECKS} pairs"));
                        break 'outer;
                    }
                    let (Some(lc), Some(rc)) = (char_of(&l), char_of(&r)) else {
                        report.kerning_pairs_skipped += 1;
                        continue;
                    };
                    let k = resolver.get(&l, &r).unwrap_or(0.0);
                    let wl = layer.get_glyph(&l).map(|g| g.width).unwrap_or(0.0);
                    let wr = layer.get_glyph(&r).map(|g| g.width).unwrap_or(0.0);
                    let mut buffer = rustybuzz::UnicodeBuffer::new();
                    buffer.push_str(&format!("{lc}{rc}"));
                    buffer.guess_segment_properties();
                    let shaped = rustybuzz::shape(&shaper, &[], buffer);
                    let total: i32 = shaped.glyph_positions().iter().map(|p| p.x_advance).sum();
                    let expected = (wl.round() + wr.round() + k.round()) as i32;
                    if (total - expected).abs() > 1 {
                        return Err(Error::new(format!(
                            "shaped {l}+{r} advance {total} differs from source {expected} (kerning {k})"
                        )));
                    }
                    report.kerning_pairs_checked += 1;
                }
            }
        }
    }
    if report.kerning_pairs_skipped > 0 {
        report.notes.push(format!(
            "{} kerning pairs involve glyphs without Unicode and could not be shaped",
            report.kerning_pairs_skipped
        ));
    }
    Ok(report)
}

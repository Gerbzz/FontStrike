//! FontStrike's editable UFO source core. The donor path model uses screen y-down.

use kurbo::{Affine, BezPath};
use norad::{Codepoints, Contour, ContourPoint, Font, Glyph, Name, PointType};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const EPS: f64 = 1e-9;
#[path = "../third_party/vectorcraft/path.rs"]
pub mod vector_path;

#[derive(Debug)]
pub struct Error(String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

pub struct FontProject {
    root: PathBuf,
    font: Font,
    undo: Vec<Font>,
    redo: Vec<Font>,
    source_snapshot: RefCell<BTreeMap<PathBuf, String>>,
}

impl FontProject {
    pub fn create(root: &Path, family: &str, style: &str) -> Result<Self> {
        if root.extension().and_then(|x| x.to_str()) != Some("ufo") || root.exists() {
            return Err(Error("new font path must be a free .ufo directory".into()));
        }
        if family.trim().is_empty() || style.trim().is_empty() {
            return Err(Error("family and style must be nonempty".into()));
        }
        let mut font = Font::new();
        font.font_info.family_name = Some(family.trim().into());
        font.font_info.style_name = Some(style.trim().into());
        font.font_info.units_per_em = Some(1000_u32.into());
        font.font_info.ascender = Some(800.0);
        font.font_info.descender = Some(-200.0);
        let mut notdef = Glyph::new(".notdef");
        notdef.width = 600.0;
        font.default_layer_mut().insert_glyph(notdef);
        let mut space = Glyph::new("space");
        space.width = 250.0;
        space.codepoints = Codepoints::new([' ']);
        font.default_layer_mut().insert_glyph(space);
        let project = Self {
            root: root.to_path_buf(),
            font,
            undo: Vec::new(),
            redo: Vec::new(),
            source_snapshot: RefCell::new(BTreeMap::new()),
        };
        project.save()?;
        Ok(project)
    }
    pub fn open(root: &Path) -> Result<Self> {
        let backup = backup_path(root);
        let actual = if root.exists() {
            root
        } else if backup.exists() {
            backup.as_path()
        } else {
            return Err(Error("UFO directory not found".into()));
        };
        if !actual.join(".fontstrike-v1").exists() {
            return Err(Error(
                "external UFO editing is pending preservation review; source left untouched".into(),
            ));
        }
        let font = Font::load(actual).map_err(|e| Error(format!("UFO open: {e}")))?;
        if font.layers.len() != 1 || !font.features.is_empty() {
            return Err(Error(
                "multi-layer or feature-rich UFO is not editable in this increment".into(),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            font,
            undo: Vec::new(),
            redo: Vec::new(),
            source_snapshot: RefCell::new(snapshot_tree(actual)?),
        })
    }
    pub fn family(&self) -> Option<&str> {
        self.font.font_info.family_name.as_deref()
    }
    pub fn codepoint(&self, name: &str) -> Result<Option<char>> {
        self.font
            .default_layer()
            .get_glyph(name)
            .map(|glyph| glyph.codepoints.iter().next())
            .ok_or_else(|| Error(format!("glyph {name} not found")))
    }
    pub fn width(&self, name: &str) -> Result<f64> {
        self.font
            .default_layer()
            .get_glyph(name)
            .map(|g| g.width)
            .ok_or_else(|| Error(format!("glyph {name} not found")))
    }
    pub fn glyph_names(&self) -> Vec<String> {
        self.font
            .default_layer()
            .iter()
            .map(|g| g.name().to_string())
            .collect()
    }
    pub fn kerning(&self, left: &str, right: &str) -> Option<f64> {
        self.font.kerning.get(left)?.get(right).copied()
    }
    fn changed(&mut self, before: Font) {
        self.undo.push(before);
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn set_outline(
        &mut self,
        name: &str,
        codepoint: Option<char>,
        width: f64,
        outline: &vector_path::PathData,
    ) -> Result<()> {
        valid_name(name)?;
        valid_width(width)?;
        if outline
            .subpaths
            .iter()
            .any(|p| !p.closed || p.anchors.len() < 3)
        {
            return Err(Error(
                "font contours must be closed with at least three anchors".into(),
            ));
        }
        if let Some(c) = codepoint {
            if self
                .font
                .default_layer()
                .iter()
                .any(|g| g.name().as_ref() != name && g.codepoints.contains(c))
            {
                return Err(Error(format!(
                    "Unicode U+{:04X} is already assigned",
                    c as u32
                )));
            }
        }
        let contours = to_ufo_contours(outline)?;
        let before = self.font.clone();
        let mut glyph = self
            .font
            .default_layer()
            .get_glyph(name)
            .cloned()
            .unwrap_or_else(|| Glyph::new(name));
        glyph.width = width;
        glyph.codepoints = Codepoints::new(codepoint);
        glyph.contours = contours;
        self.font.default_layer_mut().insert_glyph(glyph);
        self.changed(before);
        Ok(())
    }
    pub fn outline(&self, name: &str) -> Result<vector_path::PathData> {
        let glyph = self
            .font
            .default_layer()
            .get_glyph(name)
            .ok_or_else(|| Error(format!("glyph {name} not found")))?;
        if !glyph.components.is_empty() {
            return Err(Error("component glyph editing is pending".into()));
        }
        let mut bez = BezPath::new();
        for contour in &glyph.contours {
            if !contour.is_closed() {
                return Err(Error("open contour is not editable here".into()));
            }
            let sub = contour
                .to_kurbo()
                .map_err(|e| Error(format!("glyph contour: {e}")))?;
            bez.extend(sub.elements().iter().copied());
            bez.close_path();
        }
        bez.apply_affine(Affine::scale_non_uniform(1.0, -1.0));
        Ok(vector_path::PathData::from_bezpath(&bez))
    }
    pub fn set_width(&mut self, name: &str, width: f64) -> Result<()> {
        valid_width(width)?;
        let before = self.font.clone();
        let glyph = self
            .font
            .default_layer_mut()
            .get_glyph_mut(name)
            .ok_or_else(|| Error(format!("glyph {name} not found")))?;
        glyph.width = width;
        self.changed(before);
        Ok(())
    }
    pub fn set_kerning(&mut self, left: &str, right: &str, amount: f64) -> Result<()> {
        if !amount.is_finite() || !(-500.0..=500.0).contains(&amount) {
            return Err(Error("kerning must be -500..500 font units".into()));
        }
        for name in [left, right] {
            if self.font.default_layer().get_glyph(name).is_none() {
                return Err(Error(format!("glyph {name} not found")));
            }
        }
        let first = Name::new(left).map_err(|e| Error(format!("left glyph name: {e}")))?;
        let second = Name::new(right).map_err(|e| Error(format!("right glyph name: {e}")))?;
        let before = self.font.clone();
        self.font
            .kerning
            .entry(first)
            .or_default()
            .insert(second, amount);
        self.changed(before);
        Ok(())
    }
    pub fn undo(&mut self) -> Result<()> {
        let previous = self
            .undo
            .pop()
            .ok_or_else(|| Error("nothing to undo".into()))?;
        self.redo.push(std::mem::replace(&mut self.font, previous));
        Ok(())
    }
    pub fn redo(&mut self) -> Result<()> {
        let next = self
            .redo
            .pop()
            .ok_or_else(|| Error("nothing to redo".into()))?;
        self.undo.push(std::mem::replace(&mut self.font, next));
        Ok(())
    }
    pub fn save(&self) -> Result<()> {
        let stage = staging_path(&self.root);
        let backup = backup_path(&self.root);
        if stage.exists() || backup.exists() {
            return Err(Error(
                "staging or backup UFO exists; recover it before saving".into(),
            ));
        }
        let current = snapshot_tree(&self.root)?;
        if current != *self.source_snapshot.borrow() {
            return Err(Error("UFO changed outside FontStrike; reload before saving".into()));
        }
        self.font
            .save(&stage)
            .map_err(|e| Error(format!("UFO save: {e}")))?;
        fs::write(stage.join(".fontstrike-v1"), b"FontStrike UFO source v1\n")
            .map_err(|e| Error(e.to_string()))?;
        // Verify the staged source is loadable before replacing the active directory.
        Font::load(&stage).map_err(|e| Error(format!("staged UFO check: {e}")))?;
        let staged = snapshot_tree(&stage)?;
        if let Some(missing) = current.keys().find(|path| !staged.contains_key(*path)) {
            fs::remove_dir_all(&stage).map_err(|e| Error(e.to_string()))?;
            return Err(Error(format!("UFO content would be lost: {}; source preserved", missing.display())));
        }
        if snapshot_tree(&self.root)? != current {
            fs::remove_dir_all(&stage).map_err(|e| Error(e.to_string()))?;
            return Err(Error("UFO changed during save; source preserved".into()));
        }
        if self.root.exists() {
            fs::rename(&self.root, &backup).map_err(|e| Error(e.to_string()))?;
        }
        if let Err(e) = fs::rename(&stage, &self.root) {
            if backup.exists() {
                let _ = fs::rename(&backup, &self.root);
            }
            return Err(Error(e.to_string()));
        }
        *self.source_snapshot.borrow_mut() = snapshot_tree(&self.root)?;
        if backup.exists() {
            fs::remove_dir_all(backup).map_err(|e| Error(e.to_string()))?;
        }
        Ok(())
    }

    /// Compile the editable UFO to a static TrueType font. The UFO remains the
    /// source of truth; a failed compile never replaces a previous export.
    pub fn export_ttf(&self, destination: &Path) -> Result<()> {
        if destination.extension().and_then(|x| x.to_str()) != Some("ttf") {
            return Err(Error("TTF export path must end in .ttf".into()));
        }
        let stage = destination.with_extension("ttf.fontstrike-staging");
        let backup = destination.with_extension("ttf.fontstrike-backup");
        if stage.exists() || backup.exists() {
            return Err(Error("staging or backup TTF exists; recover it before exporting".into()));
        }
        for required in [".notdef", "space"] {
            if self.font.default_layer().get_glyph(required).is_none() {
                return Err(Error(format!("required glyph {required} is missing")));
            }
        }
        self.save()?;
        let source = fontc::Input::new(&self.root)
            .and_then(|input| input.create_source())
            .map_err(|e| Error(format!("font source: {e}")))?;
        let bytes = fontc::generate_font(source, fontc::Options::default())
            .map_err(|e| Error(format!("TTF compile: {e}")))?;
        let face = ttf_parser::Face::parse(&bytes, 0)
            .map_err(|e| Error(format!("compiled TTF readback: {e}")))?;
        if face.units_per_em() != 1000
            || face.tables().cmap.is_none()
            || face.tables().glyf.is_none()
            || face.tables().hmtx.is_none()
            || face.tables().name.is_none()
        {
            return Err(Error("compiled TTF is missing required tables or metrics".into()));
        }
        for glyph in self.font.default_layer().iter() {
            for character in glyph.codepoints.iter() {
                let id = face.glyph_index(character).ok_or_else(|| {
                    Error(format!("compiled TTF is missing U+{:04X}", character as u32))
                })?;
                let expected = glyph.width.round() as u16;
                if face.glyph_hor_advance(id) != Some(expected) {
                    return Err(Error(format!("compiled advance differs for {}", glyph.name())));
                }
            }
        }
        fs::write(&stage, bytes).map_err(|e| Error(format!("TTF write: {e}")))?;
        if destination.exists() {
            fs::rename(destination, &backup).map_err(|e| Error(format!("TTF backup: {e}")))?;
        }
        if let Err(e) = fs::rename(&stage, destination) {
            if backup.exists() {
                let _ = fs::rename(&backup, destination);
            }
            return Err(Error(format!("TTF replace: {e}")));
        }
        if backup.exists() {
            fs::remove_file(backup).map_err(|e| Error(format!("TTF backup cleanup: {e}")))?;
        }
        Ok(())
    }
}

fn snapshot_tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut snapshot = BTreeMap::new();
    if !root.exists() { return Ok(snapshot); }
    if !root.is_dir() { return Err(Error("UFO source is not a directory".into())); }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|e| Error(e.to_string()))? {
            let entry = entry.map_err(|e| Error(e.to_string()))?;
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|e| Error(e.to_string()))?.to_path_buf();
            let kind = fs::symlink_metadata(&path).map_err(|e| Error(e.to_string()))?.file_type();
            if kind.is_symlink() { return Err(Error(format!("UFO symlink is not supported: {}", relative.display()))); }
            if kind.is_dir() {
                snapshot.insert(relative, "directory".into());
                pending.push(path);
            } else if kind.is_file() {
                let mut file = fs::File::open(&path).map_err(|e| Error(e.to_string()))?;
                let mut hash = Sha256::new();
                io::copy(&mut file, &mut hash).map_err(|e| Error(e.to_string()))?;
                snapshot.insert(relative, format!("{:x}", hash.finalize()));
            } else { return Err(Error(format!("unsupported UFO file type: {}", relative.display()))); }
        }
    }
    Ok(snapshot)
}

fn staging_path(root: &Path) -> PathBuf {
    root.with_extension("ufo.fontstrike-staging")
}
fn backup_path(root: &Path) -> PathBuf {
    root.with_extension("ufo.fontstrike-backup")
}
fn valid_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 80
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(Error("invalid glyph name".into()));
    }
    Ok(())
}
fn valid_width(width: f64) -> Result<()> {
    if !width.is_finite() || !(1.0..=4000.0).contains(&width) {
        return Err(Error("advance width must be 1..4000".into()));
    }
    Ok(())
}
fn cp(x: f64, y: f64, typ: PointType) -> ContourPoint {
    ContourPoint::new(x, -y, typ, false, None, None)
}
fn to_ufo_contours(path: &vector_path::PathData) -> Result<Vec<Contour>> {
    let mut contours = Vec::with_capacity(path.subpaths.len());
    for sub in &path.subpaths {
        let anchors = &sub.anchors;
        if !sub.closed || anchors.len() < 3 {
            return Err(Error("open or short contour".into()));
        }
        if anchors.iter().any(|a| {
            [a.p, a.h_in, a.h_out].iter().any(|p| {
                !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 32768.0 || p.y.abs() > 32768.0
            })
        }) {
            return Err(Error(
                "non-finite or out-of-range contour coordinate".into(),
            ));
        }
        let last = anchors.last().unwrap();
        let first = &anchors[0];
        let close_curve = last.has_out() || first.has_in();
        let mut points = vec![cp(
            first.p.x,
            first.p.y,
            if close_curve {
                PointType::Curve
            } else {
                PointType::Line
            },
        )];
        for pair in anchors.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.has_out() || b.has_in() {
                points.push(cp(a.h_out.x, a.h_out.y, PointType::OffCurve));
                points.push(cp(b.h_in.x, b.h_in.y, PointType::OffCurve));
                points.push(cp(b.p.x, b.p.y, PointType::Curve));
            } else {
                points.push(cp(b.p.x, b.p.y, PointType::Line));
            }
        }
        if close_curve {
            points.push(cp(last.h_out.x, last.h_out.y, PointType::OffCurve));
            points.push(cp(first.h_in.x, first.h_in.y, PointType::OffCurve));
        }
        contours.push(Contour::new(points, None));
    }
    Ok(contours)
}

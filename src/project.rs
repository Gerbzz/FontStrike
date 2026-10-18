//! The editable font project: model, edits, and undo/redo.

use crate::export::{ExportJob, ExportReport};
use crate::outline::{self, PointId};
use crate::storage::{self, DiskState, SaveInterruption};
use crate::{Error, Result, vector_path};
use kurbo::{Affine, BezPath, Rect};
use norad::{
    Codepoints, Contour, ContourPoint, Font, FontInfo, FormatVersion, Glyph, Groups, Kerning, Name,
    Plist, PointType,
};
use std::fs;
use std::path::{Path, PathBuf};

const UNDO_LIMIT: usize = 200;
const GLYPH_ORDER: &str = "public.glyphOrder";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Glyph(String, String),
    Kerning,
    Groups,
    Info,
    Lib,
}

#[derive(Clone, Debug)]
enum Part {
    Glyph(String, String, Option<Box<Glyph>>),
    Kerning(Kerning),
    Groups(Groups),
    Info(Box<FontInfo>),
    Lib(Plist),
}

impl Part {
    fn key(&self) -> Key {
        match self {
            Part::Glyph(l, g, _) => Key::Glyph(l.clone(), g.clone()),
            Part::Kerning(_) => Key::Kerning,
            Part::Groups(_) => Key::Groups,
            Part::Info(_) => Key::Info,
            Part::Lib(_) => Key::Lib,
        }
    }
}

fn capture(font: &Font, key: &Key) -> Part {
    match key {
        Key::Glyph(layer, name) => Part::Glyph(
            layer.clone(),
            name.clone(),
            font.layers
                .get(layer)
                .and_then(|l| l.get_glyph(name))
                .cloned()
                .map(Box::new),
        ),
        Key::Kerning => Part::Kerning(font.kerning.clone()),
        Key::Groups => Part::Groups(font.groups.clone()),
        Key::Info => Part::Info(Box::new(font.font_info.clone())),
        Key::Lib => Part::Lib(font.lib.clone()),
    }
}

/// Puts `part` into the font and returns what it replaced.
fn restore(font: &mut Font, part: Part) -> Part {
    let previous = capture(font, &part.key());
    match part {
        Part::Glyph(layer, name, glyph) => {
            if let Some(l) = font.layers.get_mut(&layer) {
                match glyph {
                    Some(g) => l.insert_glyph(*g),
                    None => {
                        l.remove_glyph(&name);
                    }
                }
            }
        }
        Part::Kerning(k) => font.kerning = k,
        Part::Groups(g) => font.groups = g,
        Part::Info(i) => font.font_info = *i,
        Part::Lib(l) => font.lib = l,
    }
    previous
}

fn part_matches(font: &Font, part: &Part) -> bool {
    match (part, capture(font, &part.key())) {
        (Part::Glyph(_, _, a), Part::Glyph(_, _, b)) => *a == b,
        (Part::Kerning(a), Part::Kerning(b)) => *a == b,
        (Part::Groups(a), Part::Groups(b)) => *a == b,
        (Part::Info(a), Part::Info(b)) => a == &b,
        (Part::Lib(a), Part::Lib(b)) => *a == b,
        _ => false,
    }
}

#[derive(Clone, Debug)]
struct UndoEntry {
    label: String,
    parts: Vec<Part>,
    revision: u64,
}

/// Editable vertical metrics and naming, in font units.
#[derive(Clone, Debug, PartialEq)]
pub struct FontMetrics {
    pub family: String,
    pub style: String,
    pub units_per_em: f64,
    pub ascender: f64,
    pub descender: f64,
    pub x_height: f64,
    pub cap_height: f64,
}

/// An open UFO font source.
pub struct FontProject {
    root: PathBuf,
    font: Font,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    gesture: Option<UndoEntry>,
    revision: u64,
    saved_revision: u64,
    counter: u64,
    disk: DiskState,
    notes: Vec<String>,
}

fn name(s: &str) -> Result<Name> {
    if s.starts_with("public.kern") {
        return Err(Error::new("glyph names may not start with public.kern"));
    }
    Name::new(s).map_err(|_| Error::new(format!("invalid glyph name {s:?}")))
}

fn valid_width(width: f64) -> Result<()> {
    if !width.is_finite() || !(0.0..=10_000.0).contains(&width) {
        return Err(Error::new("advance width must be 0..10000 units"));
    }
    Ok(())
}

fn load_font(path: &Path) -> Result<Font> {
    std::panic::catch_unwind(|| Font::load(path))
        .map_err(|_| Error::new("the UFO reader crashed on this file; it was not modified"))?
        .map_err(|e| {
            let mut msg = format!("could not read UFO: {e}");
            let mut source = std::error::Error::source(&e);
            while let Some(s) = source {
                msg.push_str(&format!(": {s}"));
                source = s.source();
            }
            Error::new(format!("{msg}. Nothing was changed."))
        })
}

fn format_major(root: &Path) -> Result<Option<u64>> {
    let meta = root.join("metainfo.plist");
    if !meta.is_file() {
        return Err(Error::new(
            "not a UFO: metainfo.plist is missing (FontStrike opens .ufo folders, not .ufoz, .glyphs, .ttf or .otf)",
        ));
    }
    let value = plist::Value::from_file(&meta).map_err(|e| {
        Error::new(format!(
            "could not read metainfo.plist: {e}. Nothing was changed."
        ))
    })?;
    Ok(value
        .as_dictionary()
        .and_then(|d| d.get("formatVersion"))
        .and_then(|v| v.as_unsigned_integer()))
}

impl FontProject {
    // ----- lifecycle -------------------------------------------------------

    /// Creates and saves a new UFO 3 source with `.notdef` and `space`.
    pub fn create(root: &Path, family: &str, style: &str) -> Result<Self> {
        if root.extension().and_then(|x| x.to_str()) != Some("ufo") {
            return Err(Error::new("new font path must end in .ufo"));
        }
        if root.exists() {
            return Err(Error::new("a file or folder with that name already exists"));
        }
        if family.trim().is_empty() || style.trim().is_empty() {
            return Err(Error::new("family and style must be nonempty"));
        }
        let mut font = Font::new();
        font.font_info.family_name = Some(family.trim().into());
        font.font_info.style_name = Some(style.trim().into());
        font.font_info.units_per_em = Some(1000_u32.into());
        font.font_info.ascender = Some(800.0);
        font.font_info.descender = Some(-200.0);
        font.font_info.x_height = Some(500.0);
        font.font_info.cap_height = Some(700.0);
        let mut notdef = Glyph::new(".notdef");
        notdef.width = 600.0;
        notdef.contours = vec![
            outline::polygon(&[(60.0, 0.0), (540.0, 0.0), (540.0, 700.0), (60.0, 700.0)])?,
            outline::polygon(&[(120.0, 60.0), (120.0, 640.0), (480.0, 640.0), (480.0, 60.0)])?,
        ];
        font.default_layer_mut().insert_glyph(notdef);
        let mut space = Glyph::new("space");
        space.width = 250.0;
        space.codepoints = Codepoints::new([' ']);
        font.default_layer_mut().insert_glyph(space);
        let mut project = Self::from_font(root, font, DiskState::default(), Vec::new());
        project.saved_revision = u64::MAX;
        project.save()?;
        Ok(project)
    }

    fn from_font(root: &Path, font: Font, disk: DiskState, notes: Vec<String>) -> Self {
        Self {
            root: root.to_path_buf(),
            font,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            revision: 0,
            saved_revision: 0,
            counter: 0,
            disk,
            notes,
        }
    }

    /// Opens a UFO 2 or 3 folder. Unknown data is kept and reported in
    /// [`FontProject::notes`]; unreadable or newer-format sources are refused
    /// without touching them.
    pub fn open(root: &Path) -> Result<Self> {
        if let Some(problem) = storage::pending_recovery(root) {
            return Err(Error::new(format!(
                "{problem}. Choose Recover to finish or roll it back before opening."
            )));
        }
        storage::refuse_symlink_root(root)?;
        if !root.is_dir() {
            return Err(Error::new(format!(
                "{} is not a UFO folder",
                root.display()
            )));
        }
        let major = format_major(root)?;
        let mut notes = Vec::new();
        match major {
            Some(3) => {}
            Some(1) | Some(2) => notes.push(format!(
                "UFO {} source: it will be saved as UFO 3 (kerning groups and glif files are upgraded)",
                major.unwrap()
            )),
            Some(v) => {
                return Err(Error::new(format!(
                    "UFO format {v} is newer than FontStrike supports (UFO 2 and 3). Nothing was changed."
                )));
            }
            None => return Err(Error::new("metainfo.plist has no formatVersion. Nothing was changed.")),
        }
        let snapshot = storage::snapshot_tree(root)?;
        let mut font = load_font(root)?;
        let (preserved, mut extra) = storage::analyse(root, &font, &snapshot)?;
        notes.append(&mut extra);
        font.meta.format_version = FormatVersion::V3;
        notes.extend(describe(&font));
        Ok(Self::from_font(
            root,
            font,
            DiskState {
                snapshot,
                preserved,
            },
            notes,
        ))
    }

    /// True when an autosave of unsaved edits exists for `root`.
    pub fn autosave_available(root: &Path) -> bool {
        storage::autosave_to_restore(root).is_some()
    }

    /// Opens `root` and replaces its glyphs, kerning, and info with the
    /// autosaved edits. The result is unsaved until [`FontProject::save`].
    pub fn open_autosave(root: &Path) -> Result<Self> {
        let mut project = Self::open(root)?;
        let path = storage::autosave_to_restore(root)
            .ok_or_else(|| Error::new("no autosave copy is available"))?;
        let autosaved = load_font(&path)?;
        if path != storage::autosave_path(root) {
            storage::promote_autosave_backup(root)?;
        }
        let mut parts = Vec::new();
        // Express the restore as one undoable edit.
        let mut keys = vec![Key::Kerning, Key::Groups, Key::Info, Key::Lib];
        for layer in autosaved.layers.iter().chain(project.font.layers.iter()) {
            for g in layer.iter() {
                let k = Key::Glyph(layer.name().to_string(), g.name().to_string());
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        for key in &keys {
            parts.push(capture(&project.font, key));
        }
        project.font.kerning = autosaved.kerning.clone();
        project.font.groups = autosaved.groups.clone();
        project.font.font_info = autosaved.font_info.clone();
        project.font.lib = autosaved.lib.clone();
        for layer in autosaved.layers.iter() {
            let target = match project.font.layers.get_mut(layer.name()) {
                Some(l) => l,
                None => continue,
            };
            target.clear();
            for g in layer.iter() {
                target.insert_glyph(g.clone());
            }
        }
        project.push_entry("Restore autosave".into(), parts);
        project.notes.insert(
            0,
            "Restored unsaved edits from autosave; save to keep them".into(),
        );
        Ok(project)
    }

    /// Removes the autosave for `root` (after the user declines it).
    pub fn discard_autosave(root: &Path) -> Result<()> {
        storage::remove_autosave(root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Read-only access to the underlying UFO model.
    pub fn font(&self) -> &Font {
        &self.font
    }
    /// Notes about the source gathered at open (preserved data, limits).
    pub fn notes(&self) -> &[String] {
        &self.notes
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    /// Changes whenever the model changes (useful for caches).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    // ----- saving / export ------------------------------------------------

    /// Atomically saves to the project root; see [`crate::storage`].
    pub fn save(&mut self) -> Result<()> {
        self.save_impl(None)
    }

    #[doc(hidden)]
    pub fn save_interrupted(&mut self, stop: SaveInterruption) -> Result<()> {
        self.save_impl(Some(stop))
    }

    fn save_impl(&mut self, stop: Option<SaveInterruption>) -> Result<()> {
        if self.gesture.is_some() {
            self.end_gesture();
        }
        self.disk = storage::save(&self.font, &self.root, &self.disk, stop)?;
        self.saved_revision = self.revision;
        let _ = storage::remove_autosave(&self.root);
        Ok(())
    }

    /// Saves a copy of the current edits under a new .ufo path and switches
    /// the project to it. Unmodelled files from the old location are copied.
    pub fn save_as(&mut self, new_root: &Path) -> Result<()> {
        if new_root.extension().and_then(|x| x.to_str()) != Some("ufo") {
            return Err(Error::new("UFO path must end in .ufo"));
        }
        if new_root.exists() {
            return Err(Error::new(
                "choose a new .ufo name; that one already exists",
            ));
        }
        if self.root.exists() {
            let source = fs::canonicalize(&self.root)
                .map_err(|e| Error::new(format!("inspect source UFO: {e}")))?;
            let parent = fs::canonicalize(new_root.parent().unwrap_or(Path::new(".")))
                .map_err(|e| Error::new(format!("inspect Save As folder: {e}")))?;
            if parent.starts_with(&source) {
                return Err(Error::new("Save As destination is inside the source UFO"));
            }
        }
        let mut disk = DiskState::default();
        if self.root.exists() {
            // Copy the old tree first so verbatim data carries over.
            copy_tree(&self.root, new_root)?;
            disk.snapshot = storage::snapshot_tree(new_root)?;
            disk.preserved = self.disk.preserved.clone();
        }
        let old_root = std::mem::replace(&mut self.root, new_root.to_path_buf());
        let old_disk = std::mem::replace(&mut self.disk, disk);
        if let Err(e) = self.save() {
            self.root = old_root;
            self.disk = old_disk;
            return Err(e);
        }
        Ok(())
    }

    /// Writes an autosave copy if there are unsaved edits.
    pub fn autosave(&self) -> Result<bool> {
        if !self.is_dirty() {
            return Ok(false);
        }
        storage::write_autosave(&self.font, &self.root)?;
        Ok(true)
    }

    /// A detached export task for a background thread.
    pub fn export_job(&self) -> ExportJob {
        ExportJob::new(self.font.clone())
    }

    /// Compiles the current (possibly unsaved) model to TTF and verifies it.
    /// The UFO source on disk is not modified.
    pub fn export_ttf(&self, destination: &Path) -> Result<ExportReport> {
        self.export_job().run(destination)
    }

    // ----- queries --------------------------------------------------------

    pub fn family(&self) -> Option<&str> {
        self.font.font_info.family_name.as_deref()
    }

    pub fn metrics(&self) -> FontMetrics {
        let i = &self.font.font_info;
        let upm = i.units_per_em.map(|u| u.as_f64()).unwrap_or(1000.0);
        FontMetrics {
            family: i.family_name.clone().unwrap_or_default(),
            style: i.style_name.clone().unwrap_or_default(),
            units_per_em: upm,
            ascender: i.ascender.unwrap_or(upm * 0.8),
            descender: i.descender.unwrap_or(-upm * 0.2),
            x_height: i.x_height.unwrap_or(upm * 0.5),
            cap_height: i.cap_height.unwrap_or(upm * 0.7),
        }
    }

    fn default_layer_name(&self) -> String {
        self.font.default_layer().name().to_string()
    }

    /// Glyph names in `public.glyphOrder` order, then the rest alphabetically.
    pub fn glyph_names(&self) -> Vec<String> {
        let layer = self.font.default_layer();
        let mut out = Vec::with_capacity(layer.len());
        let mut seen = std::collections::HashSet::new();
        if let Some(order) = self.font.lib.get(GLYPH_ORDER).and_then(|v| v.as_array()) {
            for n in order.iter().filter_map(|v| v.as_string()) {
                if layer.contains_glyph(n) && seen.insert(n.to_string()) {
                    out.push(n.to_string());
                }
            }
        }
        for g in layer.iter() {
            if seen.insert(g.name().to_string()) {
                out.push(g.name().to_string());
            }
        }
        out
    }

    pub fn glyph(&self, name: &str) -> Option<&Glyph> {
        self.font.default_layer().get_glyph(name)
    }

    fn require(&self, name: &str) -> Result<&Glyph> {
        self.glyph(name)
            .ok_or_else(|| Error::new(format!("glyph {name} not found")))
    }

    pub fn codepoint(&self, name: &str) -> Result<Option<char>> {
        Ok(self.require(name)?.codepoints.iter().next())
    }
    pub fn codepoints(&self, name: &str) -> Result<Vec<char>> {
        Ok(self.require(name)?.codepoints.iter().collect())
    }
    pub fn width(&self, name: &str) -> Result<f64> {
        Ok(self.require(name)?.width)
    }
    /// The glyph mapped to `c` in the default layer.
    pub fn glyph_for_char(&self, c: char) -> Option<String> {
        self.font
            .default_layer()
            .iter()
            .find(|g| g.codepoints.contains(c))
            .map(|g| g.name().to_string())
    }
    /// Contours plus resolved components (font units, y up).
    pub fn glyph_path(&self, name: &str) -> Option<BezPath> {
        self.glyph(name)
            .map(|g| outline::resolved_path(&self.font, g))
    }
    pub fn glyph_bounds(&self, name: &str) -> Option<Rect> {
        self.glyph(name)
            .and_then(|g| outline::bounds(&self.font, g))
    }
    /// (left side bearing, right side bearing), if the glyph has an outline.
    pub fn sidebearings(&self, name: &str) -> Option<(f64, f64)> {
        let g = self.glyph(name)?;
        let b = outline::bounds(&self.font, g)?;
        Some((b.x0, g.width - b.x1))
    }
    pub fn kerning(&self, left: &str, right: &str) -> Option<f64> {
        self.font.kerning.get(left)?.get(right).copied()
    }
    /// Kerning after UFO group resolution (glyph/group precedence rules).
    pub fn effective_kerning(&self, left: &str, right: &str) -> f64 {
        self.font.kerning_resolver().get(left, right).unwrap_or(0.0)
    }
    pub fn kerning_pairs(&self) -> Vec<(String, String, f64)> {
        self.font
            .kerning
            .iter()
            .flat_map(|(l, row)| {
                row.iter()
                    .map(move |(r, v)| (l.to_string(), r.to_string(), *v))
            })
            .collect()
    }
    /// Kerning group names (`public.kern1.*`, `public.kern2.*`).
    pub fn kerning_groups(&self) -> Vec<String> {
        self.font
            .groups
            .keys()
            .filter(|k| k.starts_with("public.kern"))
            .map(|k| k.to_string())
            .collect()
    }

    /// Legacy whole-outline accessor in the VectorCraft path model (y down).
    pub fn outline(&self, name: &str) -> Result<vector_path::PathData> {
        let glyph = self.require(name)?;
        let mut bez = BezPath::new();
        for contour in &glyph.contours {
            let sub = contour
                .to_kurbo()
                .map_err(|e| Error::new(format!("glyph contour: {e}")))?;
            bez.extend(sub.elements().iter().copied());
            if contour.is_closed() {
                bez.close_path();
            }
        }
        bez.apply_affine(Affine::scale_non_uniform(1.0, -1.0));
        Ok(vector_path::PathData::from_bezpath(&bez))
    }

    // ----- undo machinery -------------------------------------------------

    fn next_revision(&mut self) -> u64 {
        self.counter += 1;
        // Revisions only need to be unique within this session.
        self.counter.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1
    }

    fn push_entry(&mut self, label: String, parts: Vec<Part>) {
        let entry = UndoEntry {
            label,
            parts,
            revision: self.revision,
        };
        self.revision = self.next_revision();
        self.undo.push(entry);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Applies `f` as one undoable step touching `keys`.
    fn edit(
        &mut self,
        label: &str,
        keys: Vec<Key>,
        f: impl FnOnce(&mut Font) -> Result<()>,
    ) -> Result<()> {
        let mut keys = keys;
        let mut seen = Vec::new();
        keys.retain(|k| {
            if seen.contains(k) {
                false
            } else {
                seen.push(k.clone());
                true
            }
        });
        let parts: Vec<Part> = keys.iter().map(|k| capture(&self.font, k)).collect();
        if let Err(e) = f(&mut self.font) {
            for p in parts {
                restore(&mut self.font, p);
            }
            return Err(e);
        }
        let changed = parts.iter().any(|p| !part_matches(&self.font, p));
        if !changed {
            return Ok(());
        }
        if let Some(g) = &mut self.gesture {
            for p in parts {
                if !g.parts.iter().any(|q| q.key() == p.key()) {
                    g.parts.push(p);
                }
            }
            self.revision = self.next_revision();
        } else {
            self.push_entry(label.into(), parts);
        }
        Ok(())
    }

    /// Starts grouping edits (e.g. a pointer drag) into one undo step.
    pub fn begin_gesture(&mut self, label: &str) {
        if self.gesture.is_none() {
            self.gesture = Some(UndoEntry {
                label: label.into(),
                parts: Vec::new(),
                revision: self.revision,
            });
        }
    }
    pub fn in_gesture(&self) -> bool {
        self.gesture.is_some()
    }
    /// Commits the grouped edits as a single undo step.
    pub fn end_gesture(&mut self) {
        if let Some(g) = self.gesture.take() {
            if g.parts.iter().any(|p| !part_matches(&self.font, p)) {
                self.undo.push(g);
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.remove(0);
                }
                self.redo.clear();
            } else {
                self.revision = g.revision;
            }
        }
    }
    /// Reverts the grouped edits (e.g. Escape during a drag).
    pub fn cancel_gesture(&mut self) {
        if let Some(g) = self.gesture.take() {
            for p in g.parts {
                restore(&mut self.font, p);
            }
            self.revision = g.revision;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }
    pub fn undo(&mut self) -> Result<()> {
        self.end_gesture();
        let entry = self
            .undo
            .pop()
            .ok_or_else(|| Error::new("nothing to undo"))?;
        let inverse = self.swap(entry);
        self.redo.push(inverse);
        Ok(())
    }
    pub fn redo(&mut self) -> Result<()> {
        self.end_gesture();
        let entry = self
            .redo
            .pop()
            .ok_or_else(|| Error::new("nothing to redo"))?;
        let inverse = self.swap(entry);
        self.undo.push(inverse);
        Ok(())
    }
    fn swap(&mut self, entry: UndoEntry) -> UndoEntry {
        let mut parts = Vec::with_capacity(entry.parts.len());
        for p in entry.parts.into_iter().rev() {
            parts.push(restore(&mut self.font, p));
        }
        parts.reverse();
        let inverse = UndoEntry {
            label: entry.label,
            parts,
            revision: self.revision,
        };
        self.revision = entry.revision;
        inverse
    }

    // ----- glyph edits ----------------------------------------------------

    fn gkey(&self, name: &str) -> Key {
        Key::Glyph(self.default_layer_name(), name.to_string())
    }

    fn check_codepoints(&self, name: &str, cps: &[char]) -> Result<()> {
        for &c in cps {
            if let Some(other) = self
                .font
                .default_layer()
                .iter()
                .find(|g| g.name().as_ref() != name && g.codepoints.contains(c))
            {
                return Err(Error::new(format!(
                    "U+{:04X} is already assigned to {}",
                    c as u32,
                    other.name()
                )));
            }
        }
        Ok(())
    }

    /// Replaces a glyph's contours from the VectorCraft path model (y down),
    /// creating the glyph if needed. Components, anchors, and libs are kept.
    pub fn set_outline(
        &mut self,
        glyph_name: &str,
        codepoint: Option<char>,
        width: f64,
        path: &vector_path::PathData,
    ) -> Result<()> {
        let n = name(glyph_name)?;
        valid_width(width)?;
        if path
            .subpaths
            .iter()
            .any(|p| !p.closed || p.anchors.len() < 3)
        {
            return Err(Error::new(
                "font contours must be closed with at least three anchors",
            ));
        }
        let cps: Vec<char> = codepoint.into_iter().collect();
        self.check_codepoints(glyph_name, &cps)?;
        let contours = crate::to_ufo_contours(path)?;
        let is_new = self.glyph(glyph_name).is_none();
        let mut keys = vec![self.gkey(glyph_name)];
        if is_new {
            keys.push(Key::Lib);
        }
        self.edit("Set outline", keys, |font| {
            let mut glyph = font
                .default_layer()
                .get_glyph(glyph_name)
                .cloned()
                .unwrap_or_else(|| Glyph::new(n.as_str()));
            glyph.width = width;
            glyph.codepoints = Codepoints::new(cps);
            glyph.contours = contours;
            font.default_layer_mut().insert_glyph(glyph);
            if is_new {
                append_order(font, glyph_name);
            }
            Ok(())
        })
    }

    /// Adds an empty glyph.
    pub fn add_glyph(&mut self, glyph_name: &str, codepoints: &[char], width: f64) -> Result<()> {
        let n = name(glyph_name)?;
        valid_width(width)?;
        if self.glyph(glyph_name).is_some() {
            return Err(Error::new(format!("glyph {glyph_name} already exists")));
        }
        self.check_codepoints(glyph_name, codepoints)?;
        let keys = vec![self.gkey(glyph_name), Key::Lib];
        self.edit(&format!("Add glyph {glyph_name}"), keys, |font| {
            let mut g = Glyph::new(n.as_str());
            g.width = width;
            g.codepoints = Codepoints::new(codepoints.iter().copied());
            font.default_layer_mut().insert_glyph(g);
            append_order(font, glyph_name);
            Ok(())
        })
    }

    /// Renames a glyph in every layer and updates kerning, groups, component
    /// references, and `public.glyphOrder`.
    pub fn rename_glyph(&mut self, old: &str, new: &str) -> Result<()> {
        if old == new {
            return Ok(());
        }
        let new_name = name(new)?;
        self.require(old)?;
        if self.glyph(new).is_some() {
            return Err(Error::new(format!("glyph {new} already exists")));
        }
        let mut keys = vec![Key::Kerning, Key::Groups, Key::Lib];
        for layer in self.font.layers.iter() {
            let ln = layer.name().to_string();
            if layer.contains_glyph(old) {
                keys.push(Key::Glyph(ln.clone(), old.into()));
                keys.push(Key::Glyph(ln.clone(), new.into()));
            }
            for g in layer.iter() {
                if g.has_component_with_base(old) {
                    keys.push(Key::Glyph(ln.clone(), g.name().to_string()));
                }
            }
        }
        self.edit(&format!("Rename {old} to {new}"), keys, |font| {
            for layer in font.layers.iter_mut() {
                if layer.contains_glyph(old) {
                    layer
                        .rename_glyph(old, new, false)
                        .map_err(|e| Error::new(format!("rename: {e}")))?;
                }
                for g in layer.iter_mut() {
                    for c in g.components.iter_mut() {
                        if c.base.as_str() == old {
                            c.base = new_name.clone();
                        }
                    }
                }
            }
            let kerning = std::mem::take(&mut font.kerning);
            for (l, row) in kerning {
                let l = if l.as_str() == old {
                    new_name.clone()
                } else {
                    l
                };
                let entry = font.kerning.entry(l).or_default();
                for (r, v) in row {
                    entry.insert(
                        if r.as_str() == old {
                            new_name.clone()
                        } else {
                            r
                        },
                        v,
                    );
                }
            }
            for members in font.groups.values_mut() {
                for m in members.iter_mut() {
                    if m.as_str() == old {
                        *m = new_name.clone();
                    }
                }
            }
            if let Some(plist::Value::Array(order)) = font.lib.get_mut(GLYPH_ORDER) {
                for v in order.iter_mut() {
                    if v.as_string() == Some(old) {
                        *v = plist::Value::String(new.into());
                    }
                }
            }
            Ok(())
        })
    }

    /// Deletes a glyph from the default layer, removing its direct kerning
    /// pairs, group memberships, and glyph-order entry. Refused while other
    /// glyphs use it as a component.
    pub fn delete_glyph(&mut self, glyph_name: &str) -> Result<()> {
        self.require(glyph_name)?;
        if let Some(user) = self
            .font
            .default_layer()
            .iter()
            .find(|g| g.has_component_with_base(glyph_name))
        {
            return Err(Error::new(format!(
                "{} uses {glyph_name} as a component; remove that first",
                user.name()
            )));
        }
        let keys = vec![self.gkey(glyph_name), Key::Kerning, Key::Groups, Key::Lib];
        self.edit(&format!("Delete glyph {glyph_name}"), keys, |font| {
            font.default_layer_mut().remove_glyph(glyph_name);
            font.kerning.remove(glyph_name);
            for row in font.kerning.values_mut() {
                row.remove(glyph_name);
            }
            font.kerning.retain(|_, row| !row.is_empty());
            for members in font.groups.values_mut() {
                members.retain(|m| m.as_str() != glyph_name);
            }
            if let Some(plist::Value::Array(order)) = font.lib.get_mut(GLYPH_ORDER) {
                order.retain(|v| v.as_string() != Some(glyph_name));
            }
            Ok(())
        })
    }

    pub fn set_codepoints(&mut self, glyph_name: &str, cps: &[char]) -> Result<()> {
        self.require(glyph_name)?;
        self.check_codepoints(glyph_name, cps)?;
        let key = self.gkey(glyph_name);
        self.edit("Change Unicode", vec![key], |font| {
            let g = font.default_layer_mut().get_glyph_mut(glyph_name).unwrap();
            g.codepoints = Codepoints::new(cps.iter().copied());
            Ok(())
        })
    }

    pub fn set_width(&mut self, glyph_name: &str, width: f64) -> Result<()> {
        valid_width(width)?;
        self.require(glyph_name)?;
        let key = self.gkey(glyph_name);
        self.edit("Change advance width", vec![key], |font| {
            font.default_layer_mut()
                .get_glyph_mut(glyph_name)
                .unwrap()
                .width = width;
            Ok(())
        })
    }

    /// Sets side bearings by shifting the outline (contours, components,
    /// anchors, guidelines) and adjusting the advance width.
    pub fn set_sidebearings(
        &mut self,
        glyph_name: &str,
        lsb: Option<f64>,
        rsb: Option<f64>,
    ) -> Result<()> {
        let (cur_l, cur_r) = self
            .sidebearings(glyph_name)
            .ok_or_else(|| Error::new("side bearings need an outline"))?;
        let lsb = lsb.unwrap_or(cur_l);
        let rsb = rsb.unwrap_or(cur_r);
        if !lsb.is_finite() || !rsb.is_finite() {
            return Err(Error::new("side bearings must be numbers"));
        }
        let dx = lsb - cur_l;
        let width = self.width(glyph_name)? + dx + (rsb - cur_r);
        valid_width(width)?;
        let key = self.gkey(glyph_name);
        self.edit("Change side bearings", vec![key], |font| {
            let g = font.default_layer_mut().get_glyph_mut(glyph_name).unwrap();
            for c in g.contours.iter_mut() {
                for p in c.points.iter_mut() {
                    p.x += dx;
                }
            }
            for c in g.components.iter_mut() {
                c.transform.x_offset += dx;
            }
            for a in g.anchors.iter_mut() {
                a.x += dx;
            }
            g.width = width;
            Ok(())
        })
    }

    fn glyph_edit(
        &mut self,
        glyph_name: &str,
        label: &str,
        f: impl FnOnce(&mut Glyph) -> Result<()>,
    ) -> Result<()> {
        self.require(glyph_name)?;
        let key = self.gkey(glyph_name);
        self.edit(label, vec![key], |font| {
            let g = font.default_layer_mut().get_glyph_mut(glyph_name).unwrap();
            f(g)
        })
    }

    /// Moves points by (dx, dy). Selected on-curve points carry their
    /// attached off-curve handles.
    pub fn move_points(
        &mut self,
        glyph_name: &str,
        ids: &[PointId],
        dx: f64,
        dy: f64,
    ) -> Result<()> {
        if dx == 0.0 && dy == 0.0 || ids.is_empty() {
            return Ok(());
        }
        self.glyph_edit(glyph_name, "Move points", |g| {
            let mut moving = std::collections::BTreeSet::new();
            for id in ids {
                let c = g
                    .contours
                    .get(id.contour)
                    .ok_or_else(|| Error::new("contour index out of range"))?;
                let n = c.points.len();
                if id.point >= n {
                    return Err(Error::new("point index out of range"));
                }
                moving.insert(*id);
                if outline::is_on(&c.points[id.point]) {
                    let closed = c.is_closed();
                    let i = id.point;
                    let prev = (closed || i > 0).then(|| (i + n - 1) % n);
                    let next = (closed || i + 1 < n).then(|| (i + 1) % n);
                    for j in [prev, next].into_iter().flatten() {
                        if c.points[j].typ == PointType::OffCurve {
                            moving.insert(PointId::new(id.contour, j));
                        }
                    }
                }
            }
            for id in moving {
                let p = &mut g.contours[id.contour].points[id.point];
                p.x += dx;
                p.y += dy;
                outline::check_coords([(p.x, p.y)])?;
            }
            Ok(())
        })
    }

    /// Moves one point to an absolute position (handles follow on-curve points).
    pub fn set_point(&mut self, glyph_name: &str, id: PointId, x: f64, y: f64) -> Result<()> {
        let p = self
            .require(glyph_name)?
            .contours
            .get(id.contour)
            .and_then(|c| c.points.get(id.point))
            .ok_or_else(|| Error::new("point not found"))?;
        let (dx, dy) = (x - p.x, y - p.y);
        self.move_points(glyph_name, &[id], dx, dy)
    }

    pub fn insert_point(
        &mut self,
        glyph_name: &str,
        contour: usize,
        segment_end: usize,
        t: f64,
    ) -> Result<PointId> {
        let mut out = PointId::new(contour, 0);
        self.glyph_edit(glyph_name, "Add point", |g| {
            let c = g
                .contours
                .get_mut(contour)
                .ok_or_else(|| Error::new("contour not found"))?;
            out.point = outline::insert_point(c, segment_end, t)?;
            Ok(())
        })?;
        Ok(out)
    }

    /// Deletes points (highest index first so earlier ids stay valid).
    pub fn delete_points(&mut self, glyph_name: &str, ids: &[PointId]) -> Result<()> {
        let mut ids = ids.to_vec();
        ids.sort();
        ids.dedup();
        self.glyph_edit(glyph_name, "Delete points", |g| {
            for id in ids.iter().rev() {
                let Some(c) = g.contours.get_mut(id.contour) else {
                    continue;
                };
                if id.point >= c.points.len() {
                    continue;
                }
                if outline::delete_point(c, id.point)? {
                    g.contours.remove(id.contour);
                }
            }
            Ok(())
        })
    }

    pub fn toggle_smooth(&mut self, glyph_name: &str, id: PointId) -> Result<()> {
        self.glyph_edit(glyph_name, "Toggle smooth", |g| {
            let p = g
                .contours
                .get_mut(id.contour)
                .and_then(|c| c.points.get_mut(id.point))
                .ok_or_else(|| Error::new("point not found"))?;
            if p.typ == PointType::OffCurve {
                return Err(Error::new("only on-curve points can be smooth"));
            }
            p.smooth = !p.smooth;
            Ok(())
        })
    }

    pub fn convert_segment(
        &mut self,
        glyph_name: &str,
        contour: usize,
        segment_end: usize,
        to_curve: bool,
    ) -> Result<()> {
        let label = if to_curve {
            "Convert to curve"
        } else {
            "Convert to line"
        };
        self.glyph_edit(glyph_name, label, |g| {
            let c = g
                .contours
                .get_mut(contour)
                .ok_or_else(|| Error::new("contour not found"))?;
            outline::convert_segment(c, segment_end, to_curve)
        })
    }

    pub fn add_contour(&mut self, glyph_name: &str, contour: Contour) -> Result<usize> {
        outline::check_coords(contour.points.iter().map(|p| (p.x, p.y)))?;
        let mut index = 0;
        self.glyph_edit(glyph_name, "Add contour", |g| {
            g.contours.push(contour);
            index = g.contours.len() - 1;
            Ok(())
        })?;
        Ok(index)
    }

    pub fn add_rectangle(
        &mut self,
        glyph_name: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<usize> {
        if x0 >= x1 || y0 >= y1 {
            return Err(Error::new("rectangle min must be below max"));
        }
        // Counter-clockwise: outer contour direction for cubic (PostScript) UFOs.
        self.add_contour(
            glyph_name,
            outline::polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])?,
        )
    }

    pub fn add_ellipse(
        &mut self,
        glyph_name: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<usize> {
        self.add_contour(glyph_name, outline::ellipse(x0, y0, x1, y1)?)
    }

    pub fn delete_contour(&mut self, glyph_name: &str, contour: usize) -> Result<()> {
        self.glyph_edit(glyph_name, "Delete contour", |g| {
            if contour >= g.contours.len() {
                return Err(Error::new("contour not found"));
            }
            g.contours.remove(contour);
            Ok(())
        })
    }

    pub fn reverse_contour(&mut self, glyph_name: &str, contour: usize) -> Result<()> {
        self.glyph_edit(glyph_name, "Reverse contour", |g| {
            let c = g
                .contours
                .get_mut(contour)
                .ok_or_else(|| Error::new("contour not found"))?;
            outline::reverse_contour(c)
        })
    }

    /// Pen tool: starts an open contour at (x, y).
    pub fn start_contour(&mut self, glyph_name: &str, x: f64, y: f64) -> Result<usize> {
        let point = ContourPoint::new(x, y, PointType::Move, false, None, None);
        self.add_contour(glyph_name, Contour::new(vec![point], None))
    }

    /// Pen tool: appends a line point to an open contour.
    pub fn append_point(
        &mut self,
        glyph_name: &str,
        contour: usize,
        x: f64,
        y: f64,
    ) -> Result<PointId> {
        outline::check_coords([(x, y)])?;
        let mut id = PointId::new(contour, 0);
        self.glyph_edit(glyph_name, "Add point", |g| {
            let c = g
                .contours
                .get_mut(contour)
                .ok_or_else(|| Error::new("contour not found"))?;
            if c.is_closed() {
                return Err(Error::new("contour is already closed"));
            }
            c.points
                .push(ContourPoint::new(x, y, PointType::Line, false, None, None));
            id.point = c.points.len() - 1;
            Ok(())
        })?;
        Ok(id)
    }

    /// Pen tool: closes an open contour.
    pub fn close_contour(&mut self, glyph_name: &str, contour: usize) -> Result<()> {
        self.glyph_edit(glyph_name, "Close contour", |g| {
            let c = g
                .contours
                .get_mut(contour)
                .ok_or_else(|| Error::new("contour not found"))?;
            if c.is_closed() {
                return Ok(());
            }
            if c.points.iter().filter(|p| outline::is_on(p)).count() < 3 {
                return Err(Error::new("a closed contour needs at least three points"));
            }
            c.points[0].typ = PointType::Line;
            Ok(())
        })
    }

    // ----- kerning and info -----------------------------------------------

    fn kerning_side_ok(&self, side: &str, first: bool) -> Result<Name> {
        let prefix = if first {
            "public.kern1."
        } else {
            "public.kern2."
        };
        if side.starts_with("public.kern") {
            if !side.starts_with(prefix) || !self.font.groups.contains_key(side) {
                return Err(Error::new(format!(
                    "{side} is not a {} kerning group",
                    if first { "left" } else { "right" }
                )));
            }
        } else if self.glyph(side).is_none() {
            return Err(Error::new(format!("glyph {side} not found")));
        }
        Name::new(side).map_err(|_| Error::new(format!("invalid kerning side {side:?}")))
    }

    /// Sets a kerning value for a glyph or group pair.
    pub fn set_kerning(&mut self, left: &str, right: &str, amount: f64) -> Result<()> {
        if !amount.is_finite() || !(-2000.0..=2000.0).contains(&amount) {
            return Err(Error::new("kerning must be -2000..2000 font units"));
        }
        let first = self.kerning_side_ok(left, true)?;
        let second = self.kerning_side_ok(right, false)?;
        self.edit(
            &format!("Kern {left} {right}"),
            vec![Key::Kerning],
            |font| {
                font.kerning
                    .entry(first)
                    .or_default()
                    .insert(second, amount);
                Ok(())
            },
        )
    }

    pub fn remove_kerning(&mut self, left: &str, right: &str) -> Result<()> {
        if self.kerning(left, right).is_none() {
            return Err(Error::new(format!("no kerning pair {left} {right}")));
        }
        self.edit(
            &format!("Remove kerning {left} {right}"),
            vec![Key::Kerning],
            |font| {
                if let Some(row) = font.kerning.get_mut(left) {
                    row.remove(right);
                    if row.is_empty() {
                        font.kerning.remove(left);
                    }
                }
                Ok(())
            },
        )
    }

    pub fn set_metrics(&mut self, m: &FontMetrics) -> Result<()> {
        if m.family.trim().is_empty() || m.style.trim().is_empty() {
            return Err(Error::new("family and style must be nonempty"));
        }
        if !(16.0..=16384.0).contains(&m.units_per_em) {
            return Err(Error::new("units per em must be 16..16384"));
        }
        for v in [m.ascender, m.descender, m.x_height, m.cap_height] {
            if !v.is_finite() || v.abs() > 32_000.0 {
                return Err(Error::new("metrics must be within ±32000"));
            }
        }
        if m.ascender <= m.descender {
            return Err(Error::new("ascender must be above descender"));
        }
        let m = m.clone();
        self.edit("Change font info", vec![Key::Info], move |font| {
            let i = &mut font.font_info;
            i.family_name = Some(m.family.trim().into());
            i.style_name = Some(m.style.trim().into());
            let upm = m.units_per_em;
            i.units_per_em = Some((upm.round() as u32).into());
            i.ascender = Some(m.ascender);
            i.descender = Some(m.descender);
            i.x_height = Some(m.x_height);
            i.cap_height = Some(m.cap_height);
            Ok(())
        })
    }
}

fn append_order(font: &mut Font, glyph: &str) {
    if let Some(plist::Value::Array(order)) = font.lib.get_mut(GLYPH_ORDER)
        && !order.iter().any(|v| v.as_string() == Some(glyph))
    {
        order.push(plist::Value::String(glyph.into()));
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir(to).map_err(|e| Error::new(format!("create {}: {e}", to.display())))?;
    for entry in std::fs::read_dir(from).map_err(|e| Error::new(e.to_string()))? {
        let entry = entry.map_err(|e| Error::new(e.to_string()))?;
        let target = to.join(entry.file_name());
        // file_type() does not follow links, so a link is never copied through.
        let kind = entry.file_type().map_err(|e| Error::new(e.to_string()))?;
        if kind.is_symlink() {
            return Err(Error::new(format!(
                "UFO contains a symlink, which FontStrike does not follow: {}",
                entry.path().display()
            )));
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| Error::new(e.to_string()))?;
        }
    }
    Ok(())
}

/// Informational notes about data FontStrike shows but edits only partially.
fn describe(font: &Font) -> Vec<String> {
    let mut notes = Vec::new();
    if font.layers.len() > 1 {
        let names: Vec<_> = font
            .layers
            .iter()
            .skip(1)
            .map(|l| l.name().to_string())
            .collect();
        notes.push(format!(
            "Only the default layer is edited; other layers are preserved: {}",
            names.join(", ")
        ));
    }
    let layer = font.default_layer();
    let with_components = layer.iter().filter(|g| !g.components.is_empty()).count();
    if with_components > 0 {
        notes.push(format!(
            "{with_components} glyphs use components: they are drawn and kept, and their own contours are editable, but component placement is not edited here"
        ));
    }
    let quadratic = layer
        .iter()
        .filter(|g| {
            g.contours
                .iter()
                .any(|c| c.points.iter().any(|p| p.typ == PointType::QCurve))
        })
        .count();
    if quadratic > 0 {
        notes.push(format!(
            "{quadratic} glyphs use quadratic (TrueType) curves: points can be moved and deleted, but multi-point quadratic runs cannot be split"
        ));
    }
    let images = layer.iter().filter(|g| g.image.is_some()).count();
    if images > 0 {
        notes.push(format!(
            "{images} glyphs reference background images; kept but not displayed"
        ));
    }
    if !font.features.is_empty() {
        notes.push("features.fea is preserved and compiled on export, but not edited here".into());
    }
    if !font.groups.is_empty() {
        notes.push(format!(
            "{} groups are preserved; group kerning is applied and can be edited by group name",
            font.groups.len()
        ));
    }
    notes
}

impl std::fmt::Debug for FontProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontProject")
            .field("root", &self.root)
            .field("glyphs", &self.font.default_layer().len())
            .field("dirty", &self.is_dirty())
            .finish()
    }
}

impl FontProject {
    /// Forgets undo/redo history (used after programmatic construction,
    /// e.g. the synthetic test font, so the user starts with a clean slate).
    pub fn clear_history(&mut self) {
        self.end_gesture();
        self.undo.clear();
        self.redo.clear();
    }
}

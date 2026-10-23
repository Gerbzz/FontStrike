//! FontStrike desktop editor (egui). All document logic lives in the
//! `fontstrike` library; this file is presentation and input handling.

use eframe::egui::{
    self, Align, Align2, Color32, FontFamily, FontId, Key, KeyboardShortcut, Modifiers, Pos2, Rect,
    RichText, Rounding, Sense, Stroke, TextStyle, TextureHandle, TextureOptions, pos2, vec2,
};
use fontstrike::norad::{Glyph, PointType};
use fontstrike::outline::{self, SegmentKind};
use fontstrike::session::{Level, Notice, OpenOutcome, Session};
use fontstrike::{ExportReport, FontMetrics, FontProject, PointId, raster};
use kurbo::{Affine, BezPath, Point};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ----- palette ---------------------------------------------------------------
const BG: Color32 = Color32::from_rgb(17, 20, 24);
const CANVAS: Color32 = Color32::from_rgb(21, 25, 30);
const PANEL: Color32 = Color32::from_rgb(27, 32, 38);
const RAISED: Color32 = Color32::from_rgb(35, 41, 48);
const LINE: Color32 = Color32::from_rgb(52, 60, 69);
const GUIDE: Color32 = Color32::from_rgb(64, 74, 84);
const TEXT: Color32 = Color32::from_rgb(230, 235, 236);
const MUTED: Color32 = Color32::from_rgb(158, 170, 176);
const ACCENT: Color32 = Color32::from_rgb(242, 178, 102);
const ACCENT_DIM: Color32 = Color32::from_rgb(140, 100, 58);
const SELECT: Color32 = Color32::from_rgb(110, 182, 255);
const SUCCESS: Color32 = Color32::from_rgb(126, 214, 152);
const WARN: Color32 = Color32::from_rgb(246, 204, 104);
const DANGER: Color32 = Color32::from_rgb(255, 128, 118);
const COMPONENT: Color32 = Color32::from_rgb(132, 160, 214);

const LOGO_PNG: &[u8] = include_bytes!("../../assets/branding/FontStrike-logo.png");
const PWDS_PNG: &[u8] = include_bytes!("../../assets/branding/PWDS-logo.png");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Pen,
}

enum Drag {
    Points { start: Point, applied: (f64, f64) },
    Marquee { start: Pos2, additive: bool },
    Pan,
}

#[derive(Clone)]
enum Pending {
    New,
    Open,
    Demo,
    Quit,
}

enum Dialog {
    Discard(Pending),
    NewFont {
        family: String,
        style: String,
    },
    Recovery(PathBuf, String),
    Autosave(PathBuf),
    AddGlyph {
        name: String,
        unicode: String,
        width: f64,
    },
    DeleteGlyph(String),
    Shortcuts,
}

struct ViewState {
    zoom: f32,
    origin: Pos2,
    needs_fit: bool,
}

struct Thumb {
    key: u64,
    tex: TextureHandle,
}

type ExportResult = (PathBuf, fontstrike::Result<ExportReport>);

struct App {
    session: Session,
    selected: Option<String>,
    selection: BTreeSet<PointId>,
    tool: Tool,
    view: ViewState,
    drag: Option<Drag>,
    pen_contour: Option<usize>,
    cursor_units: Option<Point>,
    filter: String,
    sample: String,
    show_fill: bool,
    name_buf: String,
    unicode_buf: String,
    buffers_for: Option<(String, u64)>,
    kern_right: String,
    kern_value: f64,
    shape: [f64; 4],
    metrics_buf: Option<FontMetrics>,
    dialog: Option<Dialog>,
    about: bool,
    export_rx: Option<mpsc::Receiver<ExportResult>>,
    thumbs: HashMap<String, Thumb>,
    preview_tex: Option<(u64, TextureHandle)>,
    canvas_fill: Option<(u64, TextureHandle)>,
    logo: Option<TextureHandle>,
    pwds: Option<TextureHandle>,
    last_autosave: Instant,
    allow_close: bool,
    capture: Option<PathBuf>,
    capture_requested: bool,
    frames: usize,
    styled: bool,
}

// ----- helpers ---------------------------------------------------------------

fn decode_png(bytes: &[u8], max: usize) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = info.line_size / w;
    let rgba: Vec<u8> = match channels {
        4 => buf[..w * h * 4].to_vec(),
        2 => buf[..w * h * 2]
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return None,
    };
    // Box-downscale so large artwork stays crisp at UI sizes.
    let factor = (w.max(h) / max).max(1);
    if factor == 1 {
        return Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba));
    }
    let (nw, nh) = (w / factor, h / factor);
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            for dy in 0..factor {
                for dx in 0..factor {
                    let i = ((y * factor + dy) * w + (x * factor + dx)) * 4;
                    let a = rgba[i + 3] as u32;
                    for (c, slot) in acc.iter_mut().take(3).enumerate() {
                        *slot += rgba[i + c] as u32 * a;
                    }
                    acc[3] += a;
                }
            }
            let o = (y * nw + x) * 4;
            let n = (factor * factor) as u32;
            for c in 0..3 {
                out[o + c] = acc[c].checked_div(acc[3]).unwrap_or(0) as u8;
            }
            out[o + 3] = (acc[3] / n) as u8;
        }
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([nw, nh], &out))
}

fn coverage_image(c: &raster::Coverage, color: Color32) -> egui::ColorImage {
    let pixels = c
        .alpha
        .iter()
        .map(|&a| Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a))
        .collect();
    egui::ColorImage {
        size: [c.width, c.height],
        pixels,
    }
}

/// Cheap content fingerprint for thumbnail caching.
fn glyph_key(g: &Glyph) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    g.width.to_bits().hash(&mut h);
    for c in &g.contours {
        for p in &c.points {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        }
        0xffu8.hash(&mut h);
    }
    for c in &g.components {
        c.base.as_str().hash(&mut h);
        c.transform.x_offset.to_bits().hash(&mut h);
        c.transform.y_offset.to_bits().hash(&mut h);
    }
    h.finish()
}

fn parse_unicodes(text: &str) -> Result<Vec<char>, String> {
    let mut out = Vec::new();
    for token in text.split([' ', ',']).filter(|t| !t.is_empty()) {
        let hex = token.trim_start_matches("U+").trim_start_matches("u+");
        let c = if token.chars().count() == 1 {
            token.chars().next()
        } else if hex.len() >= 2 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else {
            None
        };
        out.push(c.ok_or_else(|| format!("{token:?} is not a character or hex code like 0041"))?);
    }
    Ok(out)
}

fn format_unicodes(cps: &[char]) -> String {
    cps.iter()
        .map(|c| format!("{:04X}", *c as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

fn section(ui: &mut egui::Ui, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong().color(TEXT))
        .default_open(open)
        .show(ui, |ui| {
            ui.add_space(2.0);
            add(ui);
            ui.add_space(6.0);
        });
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(MUTED)).wrap());
}

fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_string())
            .color(Color32::from_rgb(28, 22, 14))
            .strong(),
    )
    .fill(ACCENT)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

impl App {
    fn new(project: Option<FontProject>, capture: Option<PathBuf>, select: Option<String>) -> Self {
        let mut session = Session::new();
        if let Some(p) = project {
            session.notice = Notice {
                level: Level::Success,
                text: format!("Opened {}", fontstrike::session::display_name(p.root())),
            };
            session.project = Some(p);
        }
        let mut app = Self {
            session,
            selected: None,
            selection: BTreeSet::new(),
            tool: Tool::Select,
            view: ViewState {
                zoom: 0.5,
                origin: pos2(0.0, 0.0),
                needs_fit: true,
            },
            drag: None,
            pen_contour: None,
            cursor_units: None,
            filter: String::new(),
            sample: "VAT To HAT noon AVA".into(),
            show_fill: true,
            name_buf: String::new(),
            unicode_buf: String::new(),
            buffers_for: None,
            kern_right: String::new(),
            kern_value: -40.0,
            shape: [80.0, 0.0, 520.0, 700.0],
            metrics_buf: None,
            dialog: None,
            about: false,
            export_rx: None,
            thumbs: HashMap::new(),
            preview_tex: None,
            canvas_fill: None,
            logo: None,
            pwds: None,
            last_autosave: Instant::now(),
            allow_close: false,
            capture,
            capture_requested: false,
            frames: 0,
            styled: false,
        };
        app.select_default();
        if let Some(name) = select {
            app.select_glyph(&name);
        }
        app
    }

    fn project(&self) -> Option<&FontProject> {
        self.session.project.as_ref()
    }

    fn select_default(&mut self) {
        let first = self.project().and_then(|p| {
            let names = p.glyph_names();
            names
                .iter()
                .find(|n| n.as_str() == "A")
                .cloned()
                .or_else(|| names.first().cloned())
        });
        self.selected = first;
        self.after_selection_change();
        self.metrics_buf = None;
        self.thumbs.clear();
    }

    fn select_glyph(&mut self, name: &str) {
        if self.project().is_some_and(|p| p.glyph(name).is_some()) {
            self.selected = Some(name.to_string());
            self.after_selection_change();
        }
    }

    fn after_selection_change(&mut self) {
        self.selection.clear();
        self.pen_contour = None;
        self.drag = None;
        self.view.needs_fit = true;
        self.buffers_for = None;
    }

    fn report<T>(&mut self, result: fontstrike::Result<T>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.info(Level::Error, e.to_string());
                None
            }
        }
    }

    fn info(&mut self, level: Level, text: impl Into<String>) {
        self.session.notice = Notice {
            level,
            text: text.into(),
        };
    }

    /// Runs an edit on the current glyph and reports errors in the status bar.
    fn edit_glyph<T>(
        &mut self,
        f: impl FnOnce(&mut FontProject, &str) -> fontstrike::Result<T>,
    ) -> Option<T> {
        let name = self.selected.clone()?;
        let result = {
            let p = self.session.project.as_mut()?;
            f(p, &name)
        };
        self.report(result)
    }

    fn with_project(&mut self, f: impl FnOnce(&mut FontProject)) {
        if let Some(p) = self.session.project.as_mut() {
            f(p);
        }
    }

    // ----- document actions ---------------------------------------------

    fn request(&mut self, action: Pending) {
        if self.session.has_unsaved() {
            self.dialog = Some(Dialog::Discard(action));
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => {
                self.dialog = Some(Dialog::NewFont {
                    family: "Untitled Sans".into(),
                    style: "Regular".into(),
                });
            }
            Pending::Open => {
                let picked = rfd::FileDialog::new()
                    .set_title("Open a .ufo folder")
                    .pick_folder();
                self.open_path(picked);
            }
            Pending::Demo => {
                let picked = rfd::FileDialog::new()
                    .set_title("Save the synthetic test font")
                    .set_file_name("FontStrikeTestSans.ufo")
                    .save_file();
                match picked {
                    None => self.info(Level::Info, "Test font cancelled; nothing changed"),
                    Some(mut path) => {
                        if path.extension().is_none() {
                            path.set_extension("ufo");
                        }
                        match fontstrike::demo::build(&path) {
                            Ok(p) => {
                                self.session.project = Some(p);
                                self.info(Level::Success, "Created the synthetic test font");
                                self.select_default();
                            }
                            Err(e) => {
                                self.info(Level::Error, format!("Could not create test font: {e}"))
                            }
                        }
                    }
                }
            }
            Pending::Quit => {
                self.allow_close = true;
            }
        }
    }

    fn open_path(&mut self, picked: Option<PathBuf>) {
        let outcome = self.session.open_font(picked.clone(), true);
        match outcome {
            OpenOutcome::Opened => self.select_default(),
            OpenOutcome::NeedsRecovery(problem) => {
                if let Some(p) = picked {
                    self.dialog = Some(Dialog::Recovery(p, problem));
                }
            }
            OpenOutcome::AutosaveAvailable => {
                if let Some(p) = picked {
                    self.dialog = Some(Dialog::Autosave(p));
                }
            }
            _ => {}
        }
    }

    fn save(&mut self) {
        if self
            .session
            .project
            .as_ref()
            .is_some_and(|p| p.in_gesture())
        {
            return;
        }
        self.session.save();
    }

    fn save_as(&mut self) {
        let Some(p) = self.project() else { return };
        let name = fontstrike::session::display_name(p.root());
        let picked = rfd::FileDialog::new()
            .set_title("Save UFO as")
            .set_file_name(name)
            .save_file();
        self.session.save_as(picked);
    }

    fn export(&mut self) {
        if self.export_rx.is_some() {
            return;
        }
        let Some(p) = self.project() else { return };
        let job = p.export_job();
        let m = p.metrics();
        let suggested = format!(
            "{}-{}.ttf",
            m.family.replace(' ', ""),
            m.style.replace(' ', "")
        );
        let mut dialog = rfd::FileDialog::new()
            .set_title("Export TrueType font")
            .add_filter("TrueType font", &["ttf"])
            .set_file_name(suggested);
        if let Some(dir) = p.root().parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            self.session.finish_export(None, None);
            return;
        };
        if path.extension().is_none() {
            path.set_extension("ttf");
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = job.run(&path);
            let _ = tx.send((path, result));
        });
        self.export_rx = Some(rx);
        self.info(Level::Info, "Compiling and verifying TTF…");
    }

    fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.export_rx else { return };
        match rx.try_recv() {
            Ok((path, result)) => {
                self.export_rx = None;
                self.session.finish_export(Some(&path), Some(result));
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.export_rx = None;
                self.info(
                    Level::Error,
                    "Export stopped unexpectedly; nothing was written",
                );
            }
        }
    }

    fn undo(&mut self, redo: bool) {
        let Some(p) = self.session.project.as_mut() else {
            return;
        };
        let label = if redo { p.redo_label() } else { p.undo_label() }.map(str::to_string);
        let result = if redo { p.redo() } else { p.undo() };
        if self.report(result).is_some() {
            self.info(
                Level::Info,
                format!(
                    "{}: {}",
                    if redo { "Redid" } else { "Undid" },
                    label.unwrap_or_default()
                ),
            );
            // The step may have renamed or deleted the selected glyph.
            if let Some(name) = &self.selected
                && self.project().is_some_and(|p| p.glyph(name).is_none())
            {
                self.select_default();
            }
            self.selection.clear();
            self.buffers_for = None;
            self.metrics_buf = None;
        }
    }

    // ----- styling -------------------------------------------------------

    fn style(&mut self, ctx: &egui::Context) {
        if self.styled {
            return;
        }
        self.styled = true;
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(17.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = vec2(8.0, 7.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        style.spacing.interact_size = vec2(40.0, 28.0);
        let mut v = egui::Visuals::dark();
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = RAISED;
        v.extreme_bg_color = Color32::from_rgb(14, 17, 20);
        v.faint_bg_color = RAISED;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_rounding = Rounding::same(10.0);
        v.selection.bg_fill = Color32::from_rgb(56, 86, 120);
        v.selection.stroke = Stroke::new(1.5, SELECT);
        v.hyperlink_color = SELECT;
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.rounding = Rounding::same(6.0);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.inactive.bg_fill = RAISED;
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.bg_fill = Color32::from_rgb(46, 54, 63);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 54, 63);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(92, 104, 116));
        v.widgets.active.bg_fill = Color32::from_rgb(58, 68, 79);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(58, 68, 79);
        style.visuals = v;
        ctx.set_style(style);
        self.logo = decode_png(LOGO_PNG, 220)
            .map(|img| ctx.load_texture("logo", img, TextureOptions::LINEAR));
        self.pwds = decode_png(PWDS_PNG, 400)
            .map(|img| ctx.load_texture("pwds", img, TextureOptions::LINEAR));
    }

    // ----- keyboard ------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let fire = |m: Modifiers, k: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
        };
        let shift_cmd = Modifiers::COMMAND | Modifiers::SHIFT;
        if fire(shift_cmd, Key::S) {
            self.save_as();
        }
        if fire(Modifiers::COMMAND, Key::S) {
            self.save();
        }
        if fire(Modifiers::COMMAND, Key::O) {
            self.request(Pending::Open);
        }
        if fire(Modifiers::COMMAND, Key::N) {
            self.request(Pending::New);
        }
        if fire(Modifiers::COMMAND, Key::E) {
            self.export();
        }
        if ctx.wants_keyboard_input() || self.project().is_none() {
            return; // typing in a field: leave editing keys alone
        }
        if fire(shift_cmd, Key::Z) || fire(Modifiers::COMMAND, Key::Y) {
            self.undo(true);
        }
        if fire(Modifiers::COMMAND, Key::Z) {
            self.undo(false);
        }
        if fire(Modifiers::COMMAND, Key::Num0) {
            self.view.needs_fit = true;
        }
        if fire(Modifiers::COMMAND, Key::A) {
            self.select_all_points();
        }
        let (plain, shift) = ctx.input(|i| (i.modifiers.is_none(), i.modifiers.shift_only()));
        if plain && ctx.input(|i| i.key_pressed(Key::V)) {
            self.finish_pen();
            self.tool = Tool::Select;
        }
        if plain && ctx.input(|i| i.key_pressed(Key::P)) {
            self.tool = Tool::Pen;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if let Some(Drag::Points { .. }) = self.drag {
                self.with_project(|p| p.cancel_gesture());
                self.drag = None;
                self.info(Level::Info, "Drag cancelled");
            } else if self.pen_contour.is_some() {
                self.finish_pen();
            } else {
                self.selection.clear();
            }
        }
        if (plain || shift)
            && ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
        {
            self.delete_selection();
        }
        let step = if shift { 10.0 } else { 1.0 };
        for (key, dx, dy) in [
            (Key::ArrowLeft, -step, 0.0),
            (Key::ArrowRight, step, 0.0),
            (Key::ArrowUp, 0.0, step),
            (Key::ArrowDown, 0.0, -step),
        ] {
            if (plain || shift) && !self.selection.is_empty() && ctx.input(|i| i.key_pressed(key)) {
                let ids: Vec<_> = self.selection.iter().copied().collect();
                self.edit_glyph(|p, g| p.move_points(g, &ids, dx, dy));
            }
        }
    }

    fn select_all_points(&mut self) {
        if let Some(g) = self
            .selected
            .as_ref()
            .and_then(|n| self.project()?.glyph(n))
        {
            self.selection = g
                .contours
                .iter()
                .enumerate()
                .flat_map(|(ci, c)| (0..c.points.len()).map(move |pi| PointId::new(ci, pi)))
                .collect();
        }
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let ids: Vec<_> = self.selection.iter().copied().collect();
        let n = ids.len();
        if self.edit_glyph(|p, g| p.delete_points(g, &ids)).is_some() {
            self.selection.clear();
            self.info(
                Level::Info,
                format!("Deleted {n} point{}", if n == 1 { "" } else { "s" }),
            );
        }
    }

    fn finish_pen(&mut self) {
        let Some(c) = self.pen_contour.take() else {
            return;
        };
        let count = self
            .selected
            .as_ref()
            .and_then(|n| self.project()?.glyph(n))
            .and_then(|g| g.contours.get(c))
            .map(|c| c.points.len())
            .unwrap_or(0);
        if count >= 3 {
            if self.edit_glyph(|p, g| p.close_contour(g, c)).is_some() {
                self.info(Level::Success, "Contour closed");
            }
        } else if count > 0 {
            self.edit_glyph(|p, g| p.delete_contour(g, c));
            self.info(
                Level::Info,
                "A contour needs three points; removed the unfinished one",
            );
        }
        self.selection.clear();
    }

    // ----- top bar -------------------------------------------------------

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(12.0, 6.0)).stroke(Stroke::new(1.0, LINE)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let has = self.project().is_some();
                    {
                        ui.menu_button("File", |ui| self.file_menu(ui, has));
                        ui.menu_button("Edit", |ui| self.edit_menu(ui, has));
                        ui.menu_button("View", |ui| {
                            if ui.add(egui::Button::new("Fit glyph").shortcut_text("Ctrl+0")).clicked() {
                                ui.close_menu();
                                self.view.needs_fit = true;
                            }
                            ui.checkbox(&mut self.show_fill, "Filled preview behind outline");
                        });
                        ui.menu_button("Help", |ui| {
                            if ui.button("Keyboard shortcuts").clicked() {
                                ui.close_menu();
                                self.dialog = Some(Dialog::Shortcuts);
                            }
                            if ui.button("About FontStrike").clicked() {
                                ui.close_menu();
                                self.about = true;
                            }
                        });
                    }
                    ui.separator();
                    ui.add_enabled_ui(has, |ui| {
                        if !has {
                            return;
                        }
                        for (tool, label, tip) in [
                            (Tool::Select, "Select  V", "Select and drag points; double-click a segment to add a point"),
                            (Tool::Pen, "Pen  P", "Click to place points; click the first point or press Esc to close"),
                        ] {
                            let on = self.tool == tool;
                            let b = egui::Button::new(RichText::new(label).color(if on { BG } else { TEXT }))
                                .fill(if on { ACCENT } else { RAISED });
                            if ui.add(b).on_hover_text(tip).clicked() {
                                if tool == Tool::Select {
                                    self.finish_pen();
                                }
                                self.tool = tool;
                            }
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if !has {
                            return;
                        }
                        let busy = self.export_rx.is_some();
                        if ui
                            .add_enabled(has && !busy, primary(if busy { "Exporting…" } else { "Export TTF" }))
                            .on_hover_text("Compile, verify, and write a .ttf (Ctrl+E)")
                            .clicked()
                        {
                            self.export();
                        }
                        let dirty = self.session.has_unsaved();
                        if ui.add_enabled(has && dirty, egui::Button::new("Save")).on_hover_text("Save the UFO source (Ctrl+S)").clicked() {
                            self.save();
                        }
                        let (cu, cr) = self.project().map(|p| (p.can_undo(), p.can_redo())).unwrap_or_default();
                        if ui.add_enabled(cr, egui::Button::new("Redo")).on_hover_text("Ctrl+Shift+Z").clicked() {
                            self.undo(true);
                        }
                        if ui.add_enabled(cu, egui::Button::new("Undo")).on_hover_text("Ctrl+Z").clicked() {
                            self.undo(false);
                        }
                        if let Some(p) = self.project() {
                            ui.separator();
                            let title = fontstrike::session::display_name(p.root());
                            let label = if dirty { format!("{title}  •  unsaved") } else { title };
                            ui.add(egui::Label::new(RichText::new(label).color(if dirty { WARN } else { MUTED })).truncate())
                                .on_hover_text(p.root().display().to_string());
                        }
                    });
                });
            });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui, has: bool) {
        let item = |ui: &mut egui::Ui, enabled: bool, label: &str, keys: &str| {
            let b = egui::Button::new(label).shortcut_text(keys);
            let hit = ui.add_enabled(enabled, b).clicked();
            if hit {
                ui.close_menu();
            }
            hit
        };
        if item(ui, true, "New font…", "Ctrl+N") {
            self.request(Pending::New);
        }
        if item(ui, true, "Open UFO…", "Ctrl+O") {
            self.request(Pending::Open);
        }
        if item(ui, true, "New synthetic test font…", "") {
            self.request(Pending::Demo);
        }
        ui.separator();
        if item(ui, has, "Save", "Ctrl+S") {
            self.save();
        }
        if item(ui, has, "Save As…", "Ctrl+Shift+S") {
            self.save_as();
        }
        if item(ui, has && self.export_rx.is_none(), "Export TTF…", "Ctrl+E") {
            self.export();
        }
        ui.separator();
        if item(ui, true, "Quit", "") {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui, has: bool) {
        let (u, r) = self
            .project()
            .map(|p| {
                (
                    p.undo_label().map(str::to_string),
                    p.redo_label().map(str::to_string),
                )
            })
            .unwrap_or_default();
        let ul = u
            .as_ref()
            .map(|l| format!("Undo {l}"))
            .unwrap_or_else(|| "Undo".into());
        let rl = r
            .as_ref()
            .map(|l| format!("Redo {l}"))
            .unwrap_or_else(|| "Redo".into());
        if ui
            .add_enabled(u.is_some(), egui::Button::new(ul).shortcut_text("Ctrl+Z"))
            .clicked()
        {
            ui.close_menu();
            self.undo(false);
        }
        if ui
            .add_enabled(
                r.is_some(),
                egui::Button::new(rl).shortcut_text("Ctrl+Shift+Z"),
            )
            .clicked()
        {
            ui.close_menu();
            self.undo(true);
        }
        ui.separator();
        if ui
            .add_enabled(
                has,
                egui::Button::new("Select all points").shortcut_text("Ctrl+A"),
            )
            .clicked()
        {
            ui.close_menu();
            self.select_all_points();
        }
        if ui
            .add_enabled(
                !self.selection.is_empty(),
                egui::Button::new("Delete points").shortcut_text("Del"),
            )
            .clicked()
        {
            ui.close_menu();
            self.delete_selection();
        }
    }

    // ----- status bar ----------------------------------------------------

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(14.0, 6.0))
                    .stroke(Stroke::new(1.0, LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let n = &self.session.notice;
                    let (color, mark) = match n.level {
                        Level::Info => (MUTED, "•"),
                        Level::Success => (SUCCESS, "✔"),
                        Level::Warning => (WARN, "⚠"),
                        Level::Error => (DANGER, "✖"),
                    };
                    ui.label(RichText::new(mark).color(color));
                    let text_color = if n.level == Level::Info { TEXT } else { color };
                    let text = n.text.clone();
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if let Some(p) = self.project() {
                            ui.label(
                                RichText::new(format!("{} glyphs", p.glyph_names().len()))
                                    .small()
                                    .color(MUTED),
                            );
                            ui.label(
                                RichText::new(format!("{:.0}%", self.view.zoom * 100.0))
                                    .small()
                                    .color(MUTED),
                            );
                            if let Some(c) = self.cursor_units {
                                ui.label(
                                    RichText::new(format!("x {:.0}  y {:.0}", c.x, c.y))
                                        .monospace()
                                        .small()
                                        .color(MUTED),
                                );
                            }
                        }
                        if self.export_rx.is_some() {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(text).color(text_color)).truncate(),
                            );
                        });
                    });
                });
            });
    }

    // ----- glyph list ----------------------------------------------------

    fn glyph_panel(&mut self, ctx: &egui::Context) {
        let narrow = ctx.screen_rect().width() < 1150.0;
        egui::SidePanel::left("glyphs")
            .default_width(if narrow { 170.0 } else { 236.0 })
            .width_range(150.0..=420.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(12.0)),
            )
            .show(ctx, |ui| {
                let Some(p) = self.session.project.as_ref() else {
                    return;
                };
                let m = p.metrics();
                let filter = self.filter.to_lowercase();
                let names: Vec<String> = p
                    .glyph_names()
                    .into_iter()
                    .filter(|n| {
                        filter.is_empty()
                            || n.to_lowercase().contains(&filter)
                            || p.codepoints(n)
                                .unwrap_or_default()
                                .iter()
                                .any(|c| c.to_lowercase().to_string() == filter)
                    })
                    .collect();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Glyphs").heading());
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button("+ Add")
                            .on_hover_text("Add a new glyph")
                            .clicked()
                        {
                            self.dialog = Some(Dialog::AddGlyph {
                                name: String::new(),
                                unicode: String::new(),
                                width: 600.0,
                            });
                        }
                    });
                });
                hint(ui, &format!("{} {}", m.family, m.style));
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Filter by name or character")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                let tile = 66.0;
                let gap = ui.spacing().item_spacing.x;
                let cols = ((ui.available_width() + gap) / (tile + gap))
                    .floor()
                    .max(1.0) as usize;
                let rows = names.len().div_ceil(cols);
                let mut clicked = None;
                egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                    ui,
                    tile + 22.0,
                    rows,
                    |ui, range| {
                        for row in range {
                            ui.horizontal(|ui| {
                                for name in names.iter().skip(row * cols).take(cols) {
                                    if self.glyph_tile(ui, name, tile) {
                                        clicked = Some(name.clone());
                                    }
                                }
                            });
                        }
                    },
                );
                if names.is_empty() {
                    hint(ui, "No glyphs match the filter.");
                }
                if let Some(n) = clicked {
                    self.select_glyph(&n);
                }
            });
    }

    fn thumb(&mut self, ctx: &egui::Context, name: &str, px: usize) -> Option<egui::TextureId> {
        let p = self.session.project.as_ref()?;
        let g = p.glyph(name)?;
        let key = glyph_key(g) ^ (px as u64).rotate_left(32);
        if let Some(t) = self.thumbs.get(name)
            && t.key == key
        {
            return Some(t.tex.id());
        }
        let m = p.metrics();
        let path = p.glyph_path(name).unwrap_or_default();
        let em = (m.ascender - m.descender).max(1.0);
        let scale = px as f64 * 0.74 / em.max(g.width);
        let dx = (px as f64 - g.width * scale) / 2.0;
        let dy = (px as f64 - em * scale) / 2.0 + m.ascender * scale;
        let t = Affine::new([scale, 0.0, 0.0, -scale, dx, dy]);
        let cov = raster::rasterize(&path, t, px, px);
        let tex = ctx.load_texture(
            format!("thumb-{name}"),
            coverage_image(&cov, TEXT),
            TextureOptions::LINEAR,
        );
        let id = tex.id();
        self.thumbs.insert(name.to_string(), Thumb { key, tex });
        Some(id)
    }

    fn glyph_tile(&mut self, ui: &mut egui::Ui, name: &str, size: f32) -> bool {
        let selected = self.selected.as_deref() == Some(name);
        let (rect, response) = ui.allocate_exact_size(vec2(size, size + 18.0), Sense::click());
        let cp = self
            .project()
            .and_then(|p| p.codepoint(name).ok().flatten());
        let tip = match cp {
            Some(c) => format!("{name}  U+{:04X}", c as u32),
            None => format!("{name} (no Unicode)"),
        };
        let response = response.on_hover_text(tip.clone());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                format!("Glyph {tip}"),
            )
        });
        let px = (size * ui.ctx().pixels_per_point()) as usize;
        let tex = self.thumb(ui.ctx(), name, px);
        let painter = ui.painter_at(rect.expand(2.0));
        let box_rect = Rect::from_min_size(rect.min, vec2(size, size));
        let fill = if selected {
            Color32::from_rgb(54, 45, 34)
        } else if response.hovered() {
            RAISED
        } else {
            Color32::from_rgb(31, 37, 43)
        };
        painter.rect_filled(box_rect, 6.0, fill);
        let stroke = if response.has_focus() {
            Stroke::new(2.0, SELECT)
        } else if selected {
            Stroke::new(1.5, ACCENT)
        } else {
            Stroke::new(1.0, LINE)
        };
        painter.rect_stroke(box_rect, 6.0, stroke);
        if let Some(t) = tex {
            painter.image(
                t,
                box_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.text(
            pos2(rect.center().x, rect.max.y - 8.0),
            Align2::CENTER_CENTER,
            truncate(name, 10),
            FontId::proportional(11.5),
            if selected { ACCENT } else { MUTED },
        );
        response.clicked()
    }

    // ----- inspector -----------------------------------------------------

    fn sync_buffers(&mut self) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let rev = self.project().map(|p| p.revision()).unwrap_or_default();
        if self.buffers_for.as_ref() == Some(&(name.clone(), rev)) {
            return;
        }
        if let Some(p) = self.project() {
            self.unicode_buf = format_unicodes(&p.codepoints(&name).unwrap_or_default());
        }
        self.name_buf = name.clone();
        self.buffers_for = Some((name, rev));
    }

    fn inspector(&mut self, ctx: &egui::Context) {
        let narrow = ctx.screen_rect().width() < 1150.0;
        egui::SidePanel::right("inspector")
            .default_width(if narrow { 268.0 } else { 316.0 })
            .width_range(250.0..=460.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if self.selected.is_none() {
                            ui.label(RichText::new("Inspector").heading());
                            hint(ui, "Add a glyph to start drawing.");
                            return;
                        }
                        self.sync_buffers();
                        self.glyph_section(ui);
                        self.metrics_section(ui);
                        self.selection_section(ui);
                        self.shapes_section(ui);
                        self.kerning_section(ui);
                        self.font_section(ui);
                        self.notes_section(ui);
                    });
            });
    }

    fn glyph_section(&mut self, ui: &mut egui::Ui) {
        let name = self.selected.clone().unwrap_or_default();
        section(ui, "Glyph", true, |ui| {
            egui::Grid::new("glyph-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                let l = ui.label("Name");
                let r = ui.add(egui::TextEdit::singleline(&mut self.name_buf).desired_width(150.0)).labelled_by(l.id);
                if r.lost_focus() && self.name_buf.trim() != name {
                    let new = self.name_buf.trim().to_string();
                    if self.edit_glyph(|p, g| p.rename_glyph(g, &new)).is_some() {
                        self.selected = Some(new.clone());
                        self.thumbs.clear();
                        self.info(Level::Success, format!("Renamed {name} to {new}; kerning, groups, and components updated"));
                    }
                    self.buffers_for = None;
                }
                ui.end_row();
                let l = ui.label("Unicode");
                let r = ui
                    .add(egui::TextEdit::singleline(&mut self.unicode_buf).hint_text("e.g. 0041").desired_width(150.0))
                    .labelled_by(l.id)
                    .on_hover_text("Hex codes or single characters, separated by spaces");
                if r.lost_focus() {
                    match parse_unicodes(&self.unicode_buf) {
                        Ok(cps) => {
                            let current = self.project().and_then(|p| p.codepoints(&name).ok()).unwrap_or_default();
                            if cps != current {
                                self.edit_glyph(|p, g| p.set_codepoints(g, &cps));
                            }
                        }
                        Err(e) => self.info(Level::Error, e),
                    }
                    self.buffers_for = None;
                }
                ui.end_row();
            });
            if ui.button("Delete glyph…").clicked() {
                self.dialog = Some(Dialog::DeleteGlyph(name.clone()));
            }
        });
    }

    fn drag_value(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        value: f64,
        range: std::ops::RangeInclusive<f64>,
        gesture: &str,
    ) -> Option<f64> {
        let mut v = value;
        let l = ui.label(label);
        let r = ui
            .add(
                egui::DragValue::new(&mut v)
                    .range(range)
                    .speed(1.0)
                    .max_decimals(2),
            )
            .labelled_by(l.id);
        if r.drag_started() {
            self.with_project(|p| p.begin_gesture(gesture));
        }
        let out = (r.changed() && v != value).then_some(v);
        if r.drag_stopped() {
            self.with_project(|p| p.end_gesture());
        }
        out
    }

    fn metrics_section(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let Some((width, sb)) = self
            .project()
            .map(|p| (p.width(&name).unwrap_or(0.0), p.sidebearings(&name)))
        else {
            return;
        };
        section(ui, "Metrics", true, |ui| {
            egui::Grid::new("metrics-grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    if let Some(v) = self.drag_value(
                        ui,
                        "Advance width",
                        width,
                        0.0..=10_000.0,
                        "Change advance width",
                    ) {
                        self.edit_glyph(|p, g| p.set_width(g, v));
                    }
                    ui.end_row();
                    if let Some((l, r)) = sb {
                        if let Some(v) = self.drag_value(
                            ui,
                            "Left side bearing",
                            l,
                            -5000.0..=5000.0,
                            "Change side bearings",
                        ) {
                            self.edit_glyph(|p, g| p.set_sidebearings(g, Some(v), None));
                        }
                        ui.end_row();
                        if let Some(v) = self.drag_value(
                            ui,
                            "Right side bearing",
                            r,
                            -5000.0..=5000.0,
                            "Change side bearings",
                        ) {
                            self.edit_glyph(|p, g| p.set_sidebearings(g, None, Some(v)));
                        }
                        ui.end_row();
                    }
                });
            if sb.is_none() {
                hint(ui, "Side bearings appear once the glyph has an outline.");
            }
        });
    }

    fn selection_section(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let Some(glyph) = self.project().and_then(|p| p.glyph(&name)).cloned() else {
            return;
        };
        section(ui, "Points & contours", true, |ui| {
            if self.selection.is_empty() {
                hint(
                    ui,
                    &format!(
                        "{} contours, {} points. Click a point to select it, drag a box to select several, double-click a segment to add a point.",
                        glyph.contours.len(),
                        glyph.contours.iter().map(|c| c.points.len()).sum::<usize>()
                    ),
                );
                if !glyph.components.is_empty() {
                    let bases: Vec<_> = glyph
                        .components
                        .iter()
                        .map(|c| c.base.to_string())
                        .collect();
                    hint(
                        ui,
                        &format!(
                            "Components (kept, drawn in blue, not editable here): {}",
                            bases.join(", ")
                        ),
                    );
                }
                return;
            }
            if self.selection.len() == 1 {
                let id = *self.selection.iter().next().unwrap();
                let Some(pt) = glyph
                    .contours
                    .get(id.contour)
                    .and_then(|c| c.points.get(id.point))
                    .cloned()
                else {
                    self.selection.clear();
                    return;
                };
                let kind = match (pt.typ, pt.smooth) {
                    (PointType::OffCurve, _) => "Off-curve handle",
                    (PointType::QCurve, _) => "Quadratic on-curve point",
                    (PointType::Move, _) => "Open contour start",
                    (_, true) => "Smooth point",
                    _ => "Corner point",
                };
                ui.label(
                    RichText::new(format!(
                        "{kind} · contour {} · point {}",
                        id.contour + 1,
                        id.point + 1
                    ))
                    .small()
                    .color(MUTED),
                );
                egui::Grid::new("pt-grid")
                    .num_columns(4)
                    .spacing([8.0, 8.0])
                    .show(ui, |ui| {
                        if let Some(x) =
                            self.drag_value(ui, "X", pt.x, -32000.0..=32000.0, "Move point")
                        {
                            self.edit_glyph(|p, g| p.set_point(g, id, x, pt.y));
                        }
                        if let Some(y) =
                            self.drag_value(ui, "Y", pt.y, -32000.0..=32000.0, "Move point")
                        {
                            self.edit_glyph(|p, g| p.set_point(g, id, pt.x, y));
                        }
                        ui.end_row();
                    });
                if pt.typ != PointType::OffCurve {
                    ui.horizontal_wrapped(|ui| {
                        let mut smooth = pt.smooth;
                        if ui
                            .checkbox(&mut smooth, "Smooth")
                            .on_hover_text("Marks the point as tangent-continuous")
                            .changed()
                        {
                            self.edit_glyph(|p, g| p.toggle_smooth(g, id));
                        }
                        let seg = glyph.contours.get(id.contour).and_then(|c| {
                            outline::segments(c).into_iter().find(|s| s.end == id.point)
                        });
                        if let Some(seg) = seg {
                            if seg.kind == SegmentKind::Line {
                                if ui.button("Curve incoming segment").clicked() {
                                    self.edit_glyph(|p, g| {
                                        p.convert_segment(g, id.contour, id.point, true)
                                    });
                                    self.selection.clear();
                                }
                            } else if ui.button("Straighten incoming segment").clicked() {
                                self.edit_glyph(|p, g| {
                                    p.convert_segment(g, id.contour, id.point, false)
                                });
                                self.selection.clear();
                            }
                        }
                    });
                }
            } else {
                hint(
                    ui,
                    &format!(
                        "{} points selected. Arrow keys nudge by 1 unit, Shift+arrow by 10.",
                        self.selection.len()
                    ),
                );
            }
            let contour = self.selection.iter().next().map(|i| i.contour);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Delete points")
                    .on_hover_text("Delete / Backspace")
                    .clicked()
                {
                    self.delete_selection();
                }
                if let Some(c) = contour {
                    if ui
                        .button("Reverse contour")
                        .on_hover_text("Flip contour direction")
                        .clicked()
                    {
                        self.edit_glyph(|p, g| p.reverse_contour(g, c));
                        self.selection.clear();
                    }
                    if ui.button("Delete contour").clicked() {
                        self.edit_glyph(|p, g| p.delete_contour(g, c));
                        self.selection.clear();
                    }
                }
            });
        });
    }

    fn shapes_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Add shape", false, |ui| {
            hint(ui, "Adds a closed contour in font units (y up).");
            egui::Grid::new("shape-grid")
                .num_columns(4)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    for (i, label) in ["Left", "Bottom", "Right", "Top"].iter().enumerate() {
                        let l = ui.label(*label);
                        ui.add(egui::DragValue::new(&mut self.shape[i]).speed(1.0))
                            .labelled_by(l.id);
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            let [x0, y0, x1, y1] = self.shape;
            ui.horizontal(|ui| {
                if ui.button("Rectangle").clicked() {
                    self.edit_glyph(|p, g| p.add_rectangle(g, x0, y0, x1, y1));
                }
                if ui.button("Ellipse").clicked() {
                    self.edit_glyph(|p, g| p.add_ellipse(g, x0, y0, x1, y1));
                }
            });
        });
    }

    fn kerning_section(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let Some(p) = self.project() else { return };
        let all = p.kerning_pairs();
        let pairs: Vec<_> = all
            .iter()
            .filter(|(l, r, _)| *l == name || *r == name)
            .cloned()
            .collect();
        let group_pairs: Vec<_> = {
            let resolver = p.font().kerning_resolver();
            let g1 = resolver.get_first_group(&name).map(|n| n.to_string());
            let g2 = resolver.get_second_group(&name).map(|n| n.to_string());
            all.iter()
                .filter(|(l, r, _)| Some(l) == g1.as_ref() || Some(r) == g2.as_ref())
                .cloned()
                .collect()
        };
        let names = p.glyph_names();
        section(ui, "Kerning", true, |ui| {
            if pairs.is_empty() && group_pairs.is_empty() {
                hint(ui, &format!("No pairs involve {name} yet."));
            }
            let mut remove = None;
            let mut change = None;
            for (l, r, v) in &pairs {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{l} · {r}")).monospace());
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("✖")
                            .on_hover_text(format!("Remove pair {l} {r}"))
                            .clicked()
                        {
                            remove = Some((l.clone(), r.clone()));
                        }
                        let mut val = *v;
                        let resp = ui.add(
                            egui::DragValue::new(&mut val)
                                .range(-2000.0..=2000.0)
                                .speed(1.0)
                                .suffix(" u"),
                        );
                        if resp.drag_started() {
                            self.with_project(|p| p.begin_gesture("Change kerning"));
                        }
                        if resp.changed() && val != *v {
                            change = Some((l.clone(), r.clone(), val));
                        }
                        if resp.drag_stopped() {
                            self.with_project(|p| p.end_gesture());
                        }
                    });
                });
            }
            for (l, r, v) in &group_pairs {
                hint(ui, &format!("{l} · {r}: {v} (group pair)"));
            }
            if let Some((l, r)) = remove
                && let Some(p) = self.session.project.as_mut()
            {
                let res = p.remove_kerning(&l, &r);
                self.report(res);
            }
            if let Some((l, r, v)) = change
                && let Some(p) = self.session.project.as_mut()
            {
                let res = p.set_kerning(&l, &r, v);
                self.report(res);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{name} +")).monospace());
                egui::ComboBox::from_id_salt("kern-right")
                    .selected_text(if self.kern_right.is_empty() {
                        "glyph…".to_string()
                    } else {
                        self.kern_right.clone()
                    })
                    .width(96.0)
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for n in &names {
                            ui.selectable_value(&mut self.kern_right, n.clone(), n);
                        }
                    });
                ui.add(
                    egui::DragValue::new(&mut self.kern_value)
                        .range(-2000.0..=2000.0)
                        .speed(1.0)
                        .suffix(" u"),
                );
            });
            if ui
                .add_enabled(!self.kern_right.is_empty(), egui::Button::new("Add pair"))
                .clicked()
            {
                let (r, v) = (self.kern_right.clone(), self.kern_value);
                if self.edit_glyph(|p, g| p.set_kerning(g, &r, v)).is_some() {
                    self.info(Level::Success, format!("Kerning {name} {r} set to {v}"));
                }
            }
        });
    }

    fn font_section(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.project().map(|p| p.metrics()) else {
            return;
        };
        if self.metrics_buf.is_none() {
            self.metrics_buf = Some(current.clone());
        }
        section(ui, "Font info", false, |ui| {
            let m = self.metrics_buf.as_mut().unwrap();
            egui::Grid::new("font-grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    let l = ui.label("Family");
                    ui.add(egui::TextEdit::singleline(&mut m.family).desired_width(150.0))
                        .labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label("Style");
                    ui.add(egui::TextEdit::singleline(&mut m.style).desired_width(150.0))
                        .labelled_by(l.id);
                    ui.end_row();
                    for (label, v) in [
                        ("Units per em", &mut m.units_per_em),
                        ("Ascender", &mut m.ascender),
                        ("Cap height", &mut m.cap_height),
                        ("x-height", &mut m.x_height),
                        ("Descender", &mut m.descender),
                    ] {
                        let l = ui.label(label);
                        ui.add(egui::DragValue::new(v).speed(1.0)).labelled_by(l.id);
                        ui.end_row();
                    }
                });
            let changed = self.metrics_buf.as_ref() != Some(&current);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(changed, egui::Button::new("Apply"))
                    .clicked()
                {
                    let m = self.metrics_buf.clone().unwrap();
                    if let Some(p) = self.session.project.as_mut() {
                        let r = p.set_metrics(&m);
                        if self.report(r).is_some() {
                            self.info(Level::Success, "Font info updated");
                            self.view.needs_fit = true;
                            self.thumbs.clear();
                        }
                    }
                }
                if ui
                    .add_enabled(changed, egui::Button::new("Revert"))
                    .clicked()
                {
                    self.metrics_buf = Some(current.clone());
                }
            });
        });
    }

    fn notes_section(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.project() else { return };
        let notes = p.notes().to_vec();
        let report = self.session.last_report.clone();
        section(
            ui,
            &format!("Source notes ({})", notes.len()),
            !notes.is_empty(),
            |ui| {
                if notes.is_empty() {
                    hint(
                        ui,
                        "Nothing unusual in this source: everything in it is editable or written back unchanged.",
                    );
                }
                for n in &notes {
                    ui.add(
                        egui::Label::new(RichText::new(format!("⚠ {n}")).small().color(WARN))
                            .wrap(),
                    );
                }
                if let Some(r) = report {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Last export").strong());
                    ui.add(
                        egui::Label::new(RichText::new(r.summary()).small().color(SUCCESS)).wrap(),
                    );
                    for n in &r.notes {
                        hint(ui, n);
                    }
                }
            },
        );
    }

    // ----- canvas --------------------------------------------------------

    fn to_screen(&self, p: Point) -> Pos2 {
        pos2(
            self.view.origin.x + p.x as f32 * self.view.zoom,
            self.view.origin.y - p.y as f32 * self.view.zoom,
        )
    }
    fn to_units(&self, s: Pos2) -> Point {
        Point::new(
            ((s.x - self.view.origin.x) / self.view.zoom) as f64,
            ((self.view.origin.y - s.y) / self.view.zoom) as f64,
        )
    }

    fn fit(&mut self, rect: Rect) {
        let Some(p) = self.project() else { return };
        let m = p.metrics();
        let width = self
            .selected
            .as_ref()
            .and_then(|n| p.width(n).ok())
            .unwrap_or(0.0)
            .max(m.units_per_em * 0.4);
        let bounds = self.selected.as_ref().and_then(|n| p.glyph_bounds(n));
        let top = bounds.map(|b| b.y1.max(m.ascender)).unwrap_or(m.ascender);
        let bottom = bounds.map(|b| b.y0.min(m.descender)).unwrap_or(m.descender);
        let h = (top - bottom) as f32;
        let zoom = ((rect.height() - 80.0) / h)
            .min((rect.width() - 160.0) / width as f32)
            .clamp(0.05, 8.0);
        self.view.zoom = zoom;
        self.view.origin = pos2(
            rect.center().x - width as f32 * zoom / 2.0,
            rect.center().y + ((top + bottom) / 2.0) as f32 * zoom,
        );
        self.view.needs_fit = false;
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, CANVAS);
        let Some(name) = self.selected.clone() else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Add a glyph to start drawing",
                FontId::proportional(16.0),
                MUTED,
            );
            return;
        };
        if self.view.needs_fit {
            self.fit(rect);
        }
        let Some((m, glyph, path, rev, comp_path)) = self.project().and_then(|p| {
            let g = p.glyph(&name)?.clone();
            let mut only = g.clone();
            only.contours.clear();
            let comp = outline::resolved_path(p.font(), &only);
            Some((
                p.metrics(),
                g,
                p.glyph_path(&name).unwrap_or_default(),
                p.revision(),
                comp,
            ))
        }) else {
            return;
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                format!(
                    "Glyph canvas for {name}: {} contours, {} selected points",
                    glyph.contours.len(),
                    self.selection.len()
                ),
            )
        });

        // Zoom (wheel / pinch) around the pointer.
        if response.hovered() {
            let (scroll, zoom_delta, pointer) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            let factor = zoom_delta * (scroll.y / 400.0).exp();
            if (factor - 1.0).abs() > 1e-4
                && let Some(ptr) = pointer
            {
                let new_zoom = (self.view.zoom * factor).clamp(0.03, 12.0);
                let k = new_zoom / self.view.zoom;
                self.view.origin = ptr + (self.view.origin - ptr) * k;
                self.view.zoom = new_zoom;
            }
        }
        self.cursor_units = response.hover_pos().map(|p| self.to_units(p));

        // Metric guides and the advance box.
        let x0 = self.to_screen(Point::new(0.0, 0.0)).x;
        let xw = self.to_screen(Point::new(glyph.width, 0.0)).x;
        let asc_y = self.to_screen(Point::new(0.0, m.ascender)).y;
        let desc_y = self.to_screen(Point::new(0.0, m.descender)).y;
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, asc_y), pos2(xw.max(x0 + 1.0), desc_y)),
            0.0,
            Color32::from_rgb(25, 30, 36),
        );
        for (units, label, color) in [
            (m.ascender, "Ascender", GUIDE),
            (m.cap_height, "Cap height", GUIDE),
            (m.x_height, "x-height", GUIDE),
            (0.0, "Baseline", ACCENT_DIM),
            (m.descender, "Descender", GUIDE),
        ] {
            let y = self.to_screen(Point::new(0.0, units)).y;
            painter.line_segment(
                [pos2(rect.min.x, y), pos2(rect.max.x, y)],
                Stroke::new(1.0, color),
            );
            painter.text(
                pos2(rect.min.x + 12.0, y - 3.0),
                Align2::LEFT_BOTTOM,
                format!("{label}  {units:.0}"),
                FontId::proportional(11.0),
                MUTED,
            );
        }
        for x in [x0, xw] {
            painter.line_segment(
                [pos2(x, rect.min.y), pos2(x, rect.max.y)],
                Stroke::new(1.0, GUIDE),
            );
        }
        painter.text(
            pos2(xw + 6.0, rect.max.y - 14.0),
            Align2::LEFT_CENTER,
            format!("width {:.0}", glyph.width),
            FontId::monospace(11.0),
            MUTED,
        );
        if let Some(b) = outline::bounds(self.project().unwrap().font(), &glyph) {
            let y = rect.max.y - 30.0;
            painter.text(
                pos2((x0 + self.to_screen(Point::new(b.x0, 0.0)).x) / 2.0, y),
                Align2::CENTER_CENTER,
                format!("{:.0}", b.x0),
                FontId::monospace(11.0),
                MUTED,
            );
            painter.text(
                pos2((xw + self.to_screen(Point::new(b.x1, 0.0)).x) / 2.0, y),
                Align2::CENTER_CENTER,
                format!("{:.0}", glyph.width - b.x1),
                FontId::monospace(11.0),
                MUTED,
            );
        }

        // Filled preview (non-zero winding, like the exported font).
        if self.show_fill && !path.elements().is_empty() {
            let ppp = ui.ctx().pixels_per_point();
            let (pw, ph) = (
                (rect.width() * ppp) as usize,
                (rect.height() * ppp) as usize,
            );
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (
                    rev,
                    pw,
                    ph,
                    self.view.zoom.to_bits(),
                    self.view.origin.x.to_bits(),
                    self.view.origin.y.to_bits(),
                    &name,
                )
                    .hash(&mut h);
                h.finish()
            };
            if self.canvas_fill.as_ref().map(|c| c.0) != Some(key) && pw > 0 && ph > 0 {
                let z = (self.view.zoom * ppp) as f64;
                let ox = ((self.view.origin.x - rect.min.x) * ppp) as f64;
                let oy = ((self.view.origin.y - rect.min.y) * ppp) as f64;
                let cov = raster::rasterize(&path, Affine::new([z, 0.0, 0.0, -z, ox, oy]), pw, ph);
                let img = coverage_image(&cov, Color32::from_rgb(66, 78, 90));
                match &mut self.canvas_fill {
                    Some((k, tex)) => {
                        tex.set(img, TextureOptions::LINEAR);
                        *k = key;
                    }
                    None => {
                        self.canvas_fill = Some((
                            key,
                            ui.ctx()
                                .load_texture("canvas-fill", img, TextureOptions::LINEAR),
                        ))
                    }
                }
            }
            if let Some((_, tex)) = &self.canvas_fill {
                painter.image(
                    tex.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }

        if !comp_path.elements().is_empty() {
            self.stroke_path(&painter, &comp_path, Stroke::new(1.4, COMPONENT));
        }
        self.stroke_path(
            &painter,
            &outline::contours_path(&glyph),
            Stroke::new(1.8, ACCENT),
        );

        // Points and handles.
        let mut hits: Vec<(PointId, Pos2)> = Vec::new();
        for (ci, contour) in glyph.contours.iter().enumerate() {
            let n = contour.points.len();
            for (pi, pt) in contour.points.iter().enumerate() {
                let pos = self.to_screen(Point::new(pt.x, pt.y));
                hits.push((PointId::new(ci, pi), pos));
                if pt.typ == PointType::OffCurve {
                    for nb in [(pi + n - 1) % n, (pi + 1) % n] {
                        let q = &contour.points[nb];
                        if q.typ != PointType::OffCurve {
                            painter.line_segment(
                                [pos, self.to_screen(Point::new(q.x, q.y))],
                                Stroke::new(1.0, MUTED.gamma_multiply(0.7)),
                            );
                        }
                    }
                }
            }
        }
        let hover = response.hover_pos().and_then(|hp| {
            hits.iter()
                .filter(|(_, p)| p.distance(hp) <= 9.0)
                .min_by(|a, b| a.1.distance(hp).total_cmp(&b.1.distance(hp)))
                .map(|(id, _)| *id)
        });
        for (ci, contour) in glyph.contours.iter().enumerate() {
            for (pi, pt) in contour.points.iter().enumerate() {
                let id = PointId::new(ci, pi);
                let pos = self.to_screen(Point::new(pt.x, pt.y));
                let sel = self.selection.contains(&id);
                let hov = hover == Some(id);
                let fill = if sel {
                    SELECT
                } else if hov {
                    TEXT
                } else {
                    CANVAS
                };
                let color = if sel {
                    SELECT
                } else if pt.typ == PointType::OffCurve {
                    MUTED
                } else {
                    ACCENT
                };
                let stroke = Stroke::new(if hov || sel { 2.0 } else { 1.5 }, color);
                match pt.typ {
                    PointType::OffCurve => {
                        painter.circle(pos, 3.5, fill, stroke);
                    }
                    _ if pt.smooth => {
                        painter.circle(pos, 4.8, fill, stroke);
                    }
                    _ => {
                        painter.rect(
                            Rect::from_center_size(pos, vec2(8.5, 8.5)),
                            1.5,
                            fill,
                            stroke,
                        );
                    }
                }
                if pi == 0 && contour.points.len() > 1 {
                    painter.circle_stroke(pos, 8.5, Stroke::new(1.0, ACCENT.gamma_multiply(0.6)));
                }
            }
        }
        if glyph.contours.is_empty() && glyph.components.is_empty() {
            painter.text(
                rect.center_top() + vec2(0.0, 40.0),
                Align2::CENTER_CENTER,
                "Empty glyph: draw with the Pen (P) or use Add shape in the inspector",
                FontId::proportional(14.0),
                MUTED,
            );
        }

        // ----- interaction -----
        let space = ui.input(|i| i.key_down(Key::Space));
        if response.drag_started_by(egui::PointerButton::Middle)
            || (response.drag_started() && space)
        {
            self.drag = Some(Drag::Pan);
        }
        if let Some(Drag::Pan) = self.drag {
            self.view.origin += response.drag_delta();
            if response.drag_stopped() {
                self.drag = None;
            }
            return;
        }
        let shift = ui.input(|i| i.modifiers.shift);
        match self.tool {
            Tool::Select => self.select_tool(&response, &painter, &glyph, &hits, hover, shift),
            Tool::Pen => {
                if let Some(hp) = response.hover_pos() {
                    painter.circle_stroke(hp, 5.0, Stroke::new(1.0, ACCENT));
                }
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let u = self.to_units(pos);
                    let (x, y) = (u.x.round(), u.y.round());
                    match self.pen_contour {
                        None => {
                            if let Some(c) = self.edit_glyph(|p, g| p.start_contour(g, x, y)) {
                                self.pen_contour = Some(c);
                                self.selection = [PointId::new(c, 0)].into();
                                self.info(Level::Info, "Contour started: click to add points, click the first point to close");
                            }
                        }
                        Some(c) => {
                            if hover == Some(PointId::new(c, 0)) {
                                self.finish_pen();
                            } else if let Some(id) =
                                self.edit_glyph(|p, g| p.append_point(g, c, x, y))
                            {
                                self.selection = [id].into();
                            }
                        }
                    }
                }
                if let Some(c) = self.pen_contour
                    && let (Some(last), Some(hp)) = (
                        glyph.contours.get(c).and_then(|c| c.points.last()),
                        response.hover_pos(),
                    )
                {
                    painter.line_segment(
                        [self.to_screen(Point::new(last.x, last.y)), hp],
                        Stroke::new(1.0, ACCENT.gamma_multiply(0.5)),
                    );
                }
            }
        }
    }

    fn select_tool(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        glyph: &Glyph,
        hits: &[(PointId, Pos2)],
        hover: Option<PointId>,
        shift: bool,
    ) {
        if response.double_clicked() {
            if let Some(id) = hover {
                if glyph.contours[id.contour].points[id.point].typ != PointType::OffCurve {
                    self.edit_glyph(|p, g| p.toggle_smooth(g, id));
                }
            } else if let Some(hp) = response.interact_pointer_pos() {
                let u = self.to_units(hp);
                if let Some((c, end, t, d)) = outline::nearest_segment(glyph, u)
                    && d * (self.view.zoom as f64) <= 8.0
                    && let Some(id) = self.edit_glyph(|p, g| p.insert_point(g, c, end, t))
                {
                    self.selection = [id].into();
                    self.info(Level::Info, "Point added");
                }
            }
            return;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let start_pos = response.interact_pointer_pos().unwrap_or_default();
            if let Some(id) = hover {
                if !self.selection.contains(&id) {
                    if !shift {
                        self.selection.clear();
                    }
                    self.selection.insert(id);
                }
                self.with_project(|p| p.begin_gesture("Move points"));
                self.drag = Some(Drag::Points {
                    start: self.to_units(start_pos),
                    applied: (0.0, 0.0),
                });
            } else {
                self.drag = Some(Drag::Marquee {
                    start: start_pos,
                    additive: shift,
                });
            }
        } else if response.clicked() {
            match hover {
                Some(id) if shift => {
                    if !self.selection.remove(&id) {
                        self.selection.insert(id);
                    }
                }
                Some(id) => self.selection = [id].into(),
                None if !shift => self.selection.clear(),
                None => {}
            }
        }
        let pointer = response.interact_pointer_pos();
        let stopped = response.drag_stopped();
        match self.drag {
            Some(Drag::Points { start, applied }) => {
                if let Some(pos) = pointer {
                    let u = self.to_units(pos);
                    let mut total = ((u.x - start.x).round(), (u.y - start.y).round());
                    if shift {
                        if total.0.abs() > total.1.abs() {
                            total.1 = 0.0;
                        } else {
                            total.0 = 0.0;
                        }
                    }
                    let delta = (total.0 - applied.0, total.1 - applied.1);
                    self.drag = Some(Drag::Points {
                        start,
                        applied: total,
                    });
                    if delta != (0.0, 0.0) {
                        let ids: Vec<_> = self.selection.iter().copied().collect();
                        self.edit_glyph(|p, g| p.move_points(g, &ids, delta.0, delta.1));
                    }
                }
                if stopped {
                    self.with_project(|p| p.end_gesture());
                    self.drag = None;
                }
            }
            Some(Drag::Marquee { start, additive }) => {
                if let Some(pos) = pointer {
                    let r = Rect::from_two_pos(start, pos);
                    painter.rect(
                        r,
                        2.0,
                        SELECT.gamma_multiply(0.08),
                        Stroke::new(1.0, SELECT),
                    );
                    if stopped {
                        if !additive {
                            self.selection.clear();
                        }
                        self.selection.extend(
                            hits.iter()
                                .filter(|(_, p)| r.contains(*p))
                                .map(|(id, _)| *id),
                        );
                    }
                }
                if stopped {
                    self.drag = None;
                }
            }
            _ => {}
        }
    }

    fn stroke_path(&self, painter: &egui::Painter, path: &BezPath, stroke: Stroke) {
        let mut current: Vec<Pos2> = Vec::new();
        let flush = |pts: &mut Vec<Pos2>| {
            if pts.len() > 1 {
                painter.add(egui::Shape::line(std::mem::take(pts), stroke));
            }
            pts.clear();
        };
        kurbo::flatten(path, 0.25 / self.view.zoom as f64, |el| match el {
            kurbo::PathEl::MoveTo(p) => {
                flush(&mut current);
                current.push(self.to_screen(p));
            }
            kurbo::PathEl::LineTo(p) => current.push(self.to_screen(p)),
            kurbo::PathEl::ClosePath => {
                if let Some(first) = current.first().copied() {
                    current.push(first);
                }
                flush(&mut current);
            }
            _ => {}
        });
        flush(&mut current);
    }

    // ----- text preview --------------------------------------------------

    fn preview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Preview").strong());
            let w = ui.available_width() - 4.0;
            ui.add(
                egui::TextEdit::singleline(&mut self.sample)
                    .hint_text("Type sample text; /name inserts an unencoded glyph")
                    .desired_width(w),
            );
        });
        let Some(p) = self.session.project.as_ref() else {
            return;
        };
        let mut glyphs: Vec<String> = Vec::new();
        let mut chars = self.sample.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '/' {
                let mut n = String::new();
                while let Some(&d) = chars.peek() {
                    if d == ' ' || d == '/' {
                        break;
                    }
                    n.push(d);
                    chars.next();
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                if p.glyph(&n).is_some() {
                    glyphs.push(n);
                }
            } else if let Some(n) = p.glyph_for_char(c) {
                glyphs.push(n);
            } else if p.glyph(".notdef").is_some() {
                glyphs.push(".notdef".into());
            }
        }
        let m = p.metrics();
        let avail = ui.available_size();
        let height = avail.y.clamp(48.0, 160.0);
        let (rect, response) = ui.allocate_exact_size(vec2(avail.x, height), Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, Color32::from_rgb(238, 234, 226));
        let em = (m.ascender - m.descender).max(1.0);
        let scale = (height as f64 - 22.0) / em;
        let mut x = 16.0f64;
        let mut path = BezPath::new();
        let mut boxes = Vec::new();
        let mut kerns = Vec::new();
        for (i, n) in glyphs.iter().enumerate() {
            if i > 0 {
                let k = p.effective_kerning(&glyphs[i - 1], n);
                if k != 0.0 {
                    kerns.push((x + k * scale / 2.0, k));
                }
                x += k * scale;
            }
            if let Some(mut gp) = p.glyph_path(n) {
                gp.apply_affine(Affine::new([
                    scale,
                    0.0,
                    0.0,
                    -scale,
                    x,
                    12.0 + m.ascender * scale,
                ]));
                path.extend(gp.elements().iter().copied());
            }
            let w = p.width(n).unwrap_or(0.0) * scale;
            boxes.push((n.clone(), x, w));
            x += w;
        }
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (
                &glyphs,
                p.revision(),
                rect.width() as u32,
                rect.height() as u32,
            )
                .hash(&mut h);
            h.finish()
        };
        let ppp = ui.ctx().pixels_per_point();
        if self.preview_tex.as_ref().map(|t| t.0) != Some(key) {
            let (pw, ph) = (
                (rect.width() * ppp) as usize,
                (rect.height() * ppp) as usize,
            );
            if pw > 0 && ph > 0 {
                let cov = raster::rasterize(&path, Affine::scale(ppp as f64), pw, ph);
                let img = coverage_image(&cov, Color32::from_rgb(24, 22, 20));
                match &mut self.preview_tex {
                    Some((k, tex)) => {
                        tex.set(img, TextureOptions::LINEAR);
                        *k = key;
                    }
                    None => {
                        self.preview_tex = Some((
                            key,
                            ui.ctx()
                                .load_texture("preview", img, TextureOptions::LINEAR),
                        ))
                    }
                }
            }
        }
        if let Some((_, tex)) = &self.preview_tex {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        for (n, bx, w) in &boxes {
            if Some(n) == self.selected.as_ref() {
                let r = Rect::from_min_max(
                    pos2(rect.min.x + *bx as f32, rect.max.y - 5.0),
                    pos2(rect.min.x + (*bx + *w) as f32, rect.max.y - 2.0),
                );
                painter.rect_filled(r, 1.0, Color32::from_rgb(196, 120, 40));
            }
        }
        for (kx, k) in kerns {
            painter.text(
                pos2(rect.min.x + kx as f32, rect.min.y + 3.0),
                Align2::CENTER_TOP,
                format!("{k:+.0}"),
                FontId::proportional(10.0),
                Color32::from_rgb(150, 82, 20),
            );
        }
        let response = response.on_hover_text(
            "Rendered from the source with kerning applied. Click a glyph to edit it.",
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                format!("Text preview: {}", self.sample),
            )
        });
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let px = (pos.x - rect.min.x) as f64;
            if let Some((n, _, _)) = boxes.iter().find(|(_, bx, w)| px >= *bx && px < bx + w) {
                let n = n.clone();
                self.select_glyph(&n);
            }
        }
    }

    // ----- splash, about, dialogs ---------------------------------------

    fn brand_badge(&self, ui: &mut egui::Ui, width: f32) {
        if let Some(pwds) = &self.pwds {
            let size = vec2(width, width * pwds.size()[1] as f32 / pwds.size()[0] as f32);
            let (rect, response) = ui.allocate_exact_size(size + vec2(16.0, 8.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, 5.0, Color32::from_rgb(240, 238, 232));
            ui.painter().image(
                pwds.id(),
                rect.shrink2(vec2(8.0, 4.0)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "ProWebDevSolutions logo")
            });
        }
    }

    fn splash(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        let card_w = 440.0f32.min(screen.width() - 32.0);
        egui::Area::new(egui::Id::new("splash"))
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(PANEL)
                    .stroke(Stroke::new(1.0, LINE))
                    .rounding(Rounding::same(14.0))
                    .inner_margin(egui::Margin::same(28.0))
                    .show(ui, |ui| {
                        ui.set_width(card_w);
                        ui.vertical_centered(|ui| {
                            if let Some(logo) = &self.logo {
                                ui.add(egui::Image::new((logo.id(), vec2(108.0, 108.0))))
                                    .widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Label,
                                            true,
                                            "FontStrike product logo",
                                        )
                                    });
                            }
                            ui.add_space(4.0);
                            ui.label(RichText::new("FontStrike").size(28.0).strong().color(TEXT));
                            ui.label(
                                RichText::new("Draw the shape of a letter, then keep it editable.")
                                    .color(MUTED),
                            );
                            ui.add_space(16.0);
                            let w = card_w - 40.0;
                            if ui.add_sized([w, 36.0], primary("New font…")).clicked() {
                                self.request(Pending::New);
                            }
                            if ui
                                .add_sized([w, 34.0], egui::Button::new("Open a .ufo folder…"))
                                .clicked()
                            {
                                self.request(Pending::Open);
                            }
                            if ui
                                .add_sized(
                                    [w, 34.0],
                                    egui::Button::new("Try the synthetic test font…"),
                                )
                                .clicked()
                            {
                                self.request(Pending::Demo);
                            }
                            ui.add_space(10.0);
                            hint(
                                ui,
                                "Edits UFO 2 and 3 sources and exports verified TrueType fonts.",
                            );
                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(6.0);
                            ui.label(RichText::new("A PWDS Strike app").small().color(MUTED));
                            self.brand_badge(ui, 70.0);
                        });
                    });
            });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.vertical_centered(|ui| {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new((logo.id(), vec2(84.0, 84.0)))).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "FontStrike product logo"));
                }
                ui.label(RichText::new("FontStrike").size(22.0).strong());
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(MUTED));
                ui.add_space(6.0);
                ui.label("A desktop editor for UFO font sources: outlines, metrics, kerning, and verified TrueType export.");
                ui.add_space(8.0);
                self.brand_badge(ui, 110.0);
                ui.label(RichText::new("Part of the PWDS Strike suite by ProWebDevSolutions").small().color(MUTED));
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Built with").strong());
            hint(ui, "norad (UFO I/O), fontc (compiler), kurbo (curves), ttf-parser and rustybuzz (independent export checks), egui/eframe with AccessKit (interface), rfd (file dialogs), png. Path model from VectorCraft (MIT). Full terms in THIRD_PARTY.md.");
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.button("Close").clicked()).inner
        });
        if modal.inner || modal.should_close() {
            self.about = false;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut replacement: Option<Dialog> = None;
        let mut then: Option<Pending> = None;
        let modal = egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            match &dialog {
                Dialog::Discard(action) => {
                    ui.label(RichText::new("Unsaved changes").heading());
                    ui.label("This font has edits that are not saved yet.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary("Save")).clicked() {
                            if self.session.save() {
                                then = Some(action.clone());
                            }
                            keep = false;
                        }
                        if ui.button("Don't save").clicked() {
                            if let Some(p) = &self.session.project {
                                let _ = FontProject::discard_autosave(p.root());
                            }
                            if !matches!(action, Pending::Quit) {
                                self.session.close(true);
                            }
                            then = Some(action.clone());
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::NewFont { family, style } => {
                    let (mut f, mut s) = (family.clone(), style.clone());
                    ui.label(RichText::new("New font").heading());
                    egui::Grid::new("new-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        let l = ui.label("Family");
                        ui.add(egui::TextEdit::singleline(&mut f)).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label("Style");
                        ui.add(egui::TextEdit::singleline(&mut s)).labelled_by(l.id);
                        ui.end_row();
                    });
                    hint(ui, "Creates a UFO 3 folder with .notdef and space at 1000 units per em.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let ok = !f.trim().is_empty() && !s.trim().is_empty();
                        if ui.add_enabled(ok, primary("Choose location…")).clicked() {
                            let picked = rfd::FileDialog::new()
                                .set_title("Create UFO")
                                .set_file_name(format!("{}-{}.ufo", f.replace(' ', ""), s.replace(' ', "")))
                                .save_file();
                            if self.session.new_font(picked, &f, &s, true) {
                                self.select_default();
                            }
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    replacement = Some(Dialog::NewFont { family: f, style: s });
                }
                Dialog::Recovery(path, problem) => {
                    ui.label(RichText::new("Interrupted save found").heading());
                    ui.label(format!("{problem}."));
                    hint(ui, "Recover finishes a fully written save or keeps the last good version. Unfinished copies are kept beside the UFO, never deleted.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary("Recover and open")).clicked() {
                            if self.session.recover_and_open(path) == OpenOutcome::Opened {
                                self.select_default();
                            }
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::Autosave(path) => {
                    ui.label(RichText::new("Restore unsaved edits?").heading());
                    ui.label("FontStrike found edits from a session that ended without saving.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary("Restore edits")).clicked() {
                            if self.session.resolve_autosave(path, true) == OpenOutcome::Opened {
                                self.select_default();
                            }
                            keep = false;
                        }
                        if ui.button("Discard them").clicked() {
                            if self.session.resolve_autosave(path, false) == OpenOutcome::Opened {
                                self.select_default();
                            }
                            keep = false;
                        }
                    });
                }
                Dialog::AddGlyph { name, unicode, width } => {
                    let (mut n, mut u, mut w) = (name.clone(), unicode.clone(), *width);
                    ui.label(RichText::new("Add glyph").heading());
                    egui::Grid::new("add-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        let l = ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut n).hint_text("e.g. B or a.alt")).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label("Unicode");
                        ui.add(egui::TextEdit::singleline(&mut u).hint_text("B or 0042 (optional)")).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label("Advance width");
                        ui.add(egui::DragValue::new(&mut w).range(0.0..=10000.0)).labelled_by(l.id);
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!n.trim().is_empty(), primary("Add")).clicked() {
                            match parse_unicodes(&u) {
                                Ok(cps) => {
                                    let glyph = n.trim().to_string();
                                    if let Some(p) = self.session.project.as_mut() {
                                        let r = p.add_glyph(&glyph, &cps, w);
                                        if self.report(r).is_some() {
                                            self.select_glyph(&glyph);
                                            self.info(Level::Success, format!("Added {glyph}"));
                                            keep = false;
                                        }
                                    }
                                }
                                Err(e) => self.info(Level::Error, e),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    replacement = Some(Dialog::AddGlyph { name: n, unicode: u, width: w });
                }
                Dialog::DeleteGlyph(name) => {
                    ui.label(RichText::new(format!("Delete {name}?")).heading());
                    ui.label("Its kerning pairs and group memberships are removed too. Undo restores everything.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(170, 60, 52))).clicked() {
                            if let Some(p) = self.session.project.as_mut() {
                                let r = p.delete_glyph(name);
                                if self.report(r).is_some() {
                                    self.info(Level::Info, format!("Deleted {name} (Ctrl+Z to undo)"));
                                    self.select_default();
                                }
                            }
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::Shortcuts => {
                    ui.label(RichText::new("Keyboard shortcuts").heading());
                    egui::Grid::new("keys").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                        for (k, d) in [
                            ("V / P", "Select tool / Pen tool"),
                            ("Ctrl+Z, Ctrl+Shift+Z", "Undo, redo"),
                            ("Ctrl+S, Ctrl+Shift+S", "Save, save as"),
                            ("Ctrl+E", "Export TTF"),
                            ("Arrows (+Shift)", "Nudge points 1 (10) units"),
                            ("Delete", "Delete selected points"),
                            ("Double-click segment", "Insert a point"),
                            ("Double-click point", "Toggle smooth"),
                            ("Shift+drag", "Constrain to one axis"),
                            ("Wheel / Ctrl+0", "Zoom at pointer / fit glyph"),
                            ("Middle or Space+drag", "Pan"),
                            ("Esc", "Cancel drag, close pen contour, clear selection"),
                        ] {
                            ui.label(RichText::new(k).monospace());
                            ui.label(d);
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                }
            }
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.dialog = Some(replacement.unwrap_or(dialog));
        }
        if let Some(a) = then {
            self.perform(a);
        }
    }

    fn capture(&mut self, ctx: &egui::Context) {
        let Some(path) = self.capture.clone() else {
            return;
        };
        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let result = (|| -> Result<(), String> {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
                let mut png = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
                png.set_color(png::ColorType::Rgba);
                png.set_depth(png::BitDepth::Eight);
                png.write_header()
                    .map_err(|e| e.to_string())?
                    .write_image_data(&pixels)
                    .map_err(|e| e.to_string())?;
                Ok(())
            })();
            match result {
                Ok(()) => println!("Captured {}", path.display()),
                Err(e) => eprintln!("Capture failed: {e}"),
            }
            self.capture = None;
            self.allow_close = true;
        } else if !self.capture_requested && self.frames > 20 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.capture_requested = true;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        self.style(ctx);
        self.capture(ctx);
        self.poll_export(ctx);
        if self.dialog.is_none() && !self.about {
            self.shortcuts(ctx);
        }
        // A close request with unsaved edits asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.session.has_unsaved() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog = Some(Dialog::Discard(Pending::Quit));
            } else {
                self.allow_close = true;
            }
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Periodic autosave of unsaved edits (beside the UFO, never inside it).
        if self.last_autosave.elapsed() > Duration::from_secs(30) {
            self.last_autosave = Instant::now();
            if let Some(p) = &self.session.project
                && !p.in_gesture()
                && let Err(e) = p.autosave()
            {
                self.info(Level::Warning, format!("Autosave failed: {e}"));
            }
        }
        ctx.request_repaint_after(Duration::from_secs(30));

        self.top_bar(ctx);
        self.status_bar(ctx);
        if self.project().is_some() {
            self.glyph_panel(ctx);
            self.inspector(ctx);
            egui::TopBottomPanel::bottom("preview")
                .resizable(true)
                .default_height(132.0)
                .height_range(84.0..=240.0)
                .frame(
                    egui::Frame::none()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .stroke(Stroke::new(1.0, LINE)),
                )
                .show(ctx, |ui| self.preview(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(CANVAS))
                .show(ctx, |ui| self.canvas(ui));
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(BG))
                .show(ctx, |_| {});
            self.splash(ctx);
        }
        self.about_window(ctx);
        self.dialogs(ctx);
        if self.capture.is_some() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let capture = value("--capture").map(PathBuf::from);
    let select = value("--select");
    let about = args.iter().any(|a| a == "--about");
    let point = value("--point").and_then(|s| {
        let (c, p) = s.split_once(':')?;
        Some(PointId::new(c.parse().ok()?, p.parse().ok()?))
    });
    let size = value("--size")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?])
        })
        .unwrap_or([1360.0, 840.0]);
    let project = if let Some(demo) = value("--demo") {
        let path = PathBuf::from(demo);
        let r = if path.exists() {
            FontProject::open(&path)
        } else {
            fontstrike::demo::build(&path)
        };
        r.map_err(|e| eprintln!("Test font: {e}")).ok()
    } else if let Some(open) = value("--project") {
        FontProject::open(Path::new(&open))
            .map_err(|e| eprintln!("Open font: {e}"))
            .ok()
    } else {
        None
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("FontStrike")
        .with_inner_size(size)
        .with_min_inner_size([900.0, 600.0]);
    if let Some(icon) = decode_png(LOGO_PNG, 64) {
        viewport = viewport.with_icon(egui::IconData {
            rgba: icon
                .pixels
                .iter()
                .flat_map(|c| c.to_srgba_unmultiplied())
                .collect(),
            width: icon.size[0] as u32,
            height: icon.size[1] as u32,
        });
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "FontStrike",
        options,
        Box::new(move |_cc| {
            let mut app = App::new(project, capture, select);
            app.about = about;
            app.selection.extend(point);
            Ok(Box::new(app))
        }),
    )
}

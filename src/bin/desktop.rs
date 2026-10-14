use eframe::egui::{self, Align, Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use fontstrike::FontProject;
use fontstrike::vector_path::{Anchor, PathData, SubPath};
use kurbo::{ParamCurve, Point, Vec2};
use std::path::{Path, PathBuf};

const BG: Color32 = Color32::from_rgb(19, 23, 29);
const PANEL: Color32 = Color32::from_rgb(29, 35, 42);
const LINE: Color32 = Color32::from_rgb(57, 67, 76);
const TEXT: Color32 = Color32::from_rgb(232, 238, 237);
const MUTED: Color32 = Color32::from_rgb(165, 180, 183);
const ACCENT: Color32 = Color32::from_rgb(246, 184, 102);

struct FontApp {
    project: Option<FontProject>,
    root: Option<PathBuf>,
    family: String,
    style: String,
    selected: String,
    glyph_name: String,
    unicode: String,
    width: f64,
    outline: PathData,
    selected_point: Option<(usize, usize)>,
    drag_origin: Option<((usize, usize), Point)>,
    kern_right: String,
    kern_amount: f64,
    rect_x0: f64,
    rect_y0: f64,
    rect_x1: f64,
    rect_y1: f64,
    status: String,
    capture: Option<PathBuf>,
    capture_requested: bool,
    frames: usize,
}

impl FontApp {
    fn new(project: Option<FontProject>, root: Option<PathBuf>, capture: Option<PathBuf>) -> Self {
        let mut app = Self {
            project,
            root,
            family: "Untitled".into(),
            style: "Regular".into(),
            selected: String::new(),
            glyph_name: "A".into(),
            unicode: "A".into(),
            width: 620.0,
            outline: PathData::default(),
            selected_point: None,
            drag_origin: None,
            kern_right: "V".into(),
            kern_amount: -80.0,
            rect_x0: 80.0,
            rect_y0: 0.0,
            rect_x1: 520.0,
            rect_y1: 700.0,
            status: "Local UFO source ready".into(),
            capture,
            capture_requested: false,
            frames: 0,
        };
        app.refresh_glyph_list();
        app
    }
    fn refresh_glyph_list(&mut self) {
        if let Some(project) = &self.project {
            self.family = project.family().unwrap_or("Unnamed").into();
            let names = project.glyph_names();
            let preferred = if names.iter().any(|n| n == "A") {
                "A".to_string()
            } else {
                names.first().cloned().unwrap_or_default()
            };
            self.select(&preferred);
        }
    }
    fn select(&mut self, name: &str) {
        let Some(project) = &self.project else { return };
        self.selected = name.into();
        self.glyph_name = name.into();
        self.width = project.width(name).unwrap_or(600.0);
        self.outline = project.outline(name).unwrap_or_default();
        self.unicode = project
            .codepoint(name)
            .ok()
            .flatten()
            .map(|c| c.to_string())
            .unwrap_or_default();
        self.selected_point = None;
    }
    fn parse_unicode(&self) -> Result<Option<char>, String> {
        if self.unicode.is_empty() { return Ok(None) }
        let mut chars = self.unicode.chars();
        let one = chars.next();
        if chars.next().is_some() {
            Err("Unicode must contain exactly one character".into())
        } else {
            Ok(one)
        }
    }
    fn save_outline(&mut self) {
        let result = (|| {
            let codepoint = self.parse_unicode()?;
            let project = self.project.as_mut().ok_or("Open a font first")?;
            project
                .set_outline(&self.glyph_name, codepoint, self.width, &self.outline)
                .map_err(|e| e.to_string())?;
            project.save().map_err(|e| e.to_string())?;
            Ok::<_, String>(())
        })();
        match result {
            Ok(()) => {
                self.selected = self.glyph_name.clone();
                self.status = format!("Saved glyph {}", self.selected);
            }
            Err(e) => self.status = format!("Outline: {e}"),
        }
    }
    fn change_history(&mut self, redo: bool) {
        if let Some(project) = &mut self.project {
            let result = if redo { project.redo() } else { project.undo() };
            match result.and_then(|()| project.save()) {
                Ok(()) => {
                    self.status = if redo { "Redo saved" } else { "Undo saved" }.into();
                    let selected = self.selected.clone();
                    self.select(&selected);
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    fn set_kerning(&mut self) {
        if let Some(project) = &mut self.project {
            match project
                .set_kerning(&self.selected, &self.kern_right, self.kern_amount)
                .and_then(|()| project.save())
            {
                Ok(()) => self.status = format!(
                    "Saved {}{} kerning: {} units",
                    self.selected, self.kern_right, self.kern_amount
                ),
                Err(e) => self.status = format!("Kerning: {e}"),
            }
        }
    }
    fn export_ttf(&mut self) {
        let Some(project) = &self.project else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("TrueType font", &["ttf"])
            .set_file_name(format!("{}.ttf", self.family.replace(' ', "")))
            .save_file() else { return };
        match project.export_ttf(&path) {
            Ok(()) => self.status = format!("Compiled and checked {}", path.display()),
            Err(e) => self.status = format!("TTF export: {e}"),
        }
    }
    fn rectangle(&mut self) {
        if self.rect_x0 >= self.rect_x1 || self.rect_y0 >= self.rect_y1 {
            self.status = "Rectangle min must be below max".into();
            return;
        }
        self.outline = PathData::single(SubPath::new(
            [
                (self.rect_x0, self.rect_y0),
                (self.rect_x1, self.rect_y0),
                (self.rect_x1, self.rect_y1),
                (self.rect_x0, self.rect_y1),
            ]
            .into_iter()
            .map(|(x, y)| Anchor::corner(Point::new(x, -y)))
            .collect(),
            true,
        ));
        self.selected_point = None;
        self.save_outline();
    }
    fn draw_canvas(&mut self, ui: &mut egui::Ui) -> bool {
        let size = ui.available_size().max(vec2(400.0, 400.0));
        let (canvas, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 0.0, BG);
        let scale = (canvas.width() / 1150.0).min(canvas.height() / 1250.0).max(0.15);
        let baseline = canvas.center().y + 180.0 * scale;
        let left = canvas.center().x - 350.0 * scale;
        let to_screen = |p: Point| pos2(left + p.x as f32 * scale, baseline + p.y as f32 * scale);
        for (units, label, color) in [
            (800.0, "ASCENDER", LINE),
            (700.0, "CAP HEIGHT", Color32::from_rgb(84, 97, 102)),
            (500.0, "X HEIGHT", LINE),
            (0.0, "BASELINE", ACCENT),
            (-200.0, "DESCENDER", LINE),
        ] {
            let y = baseline - units as f32 * scale;
            painter.line_segment(
                [pos2(canvas.min.x + 24.0, y), pos2(canvas.max.x - 24.0, y)],
                Stroke::new(1.0_f32, color),
            );
            painter.text(
                pos2(canvas.min.x + 30.0, y - 8.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::monospace(10.0),
                MUTED,
            );
        }
        for units in [0.0, self.width] {
            let x = left + units as f32 * scale;
            painter.line_segment(
                [pos2(x, canvas.min.y + 24.0), pos2(x, canvas.max.y - 24.0)],
                Stroke::new(1.0_f32, Color32::from_rgb(94, 107, 112)),
            );
        }
        painter.text(
            canvas.left_top() + vec2(24.0, 20.0),
            Align2::LEFT_CENTER,
            format!("{}  /  {} units", self.selected, self.width),
            FontId::proportional(17.0),
            TEXT,
        );
        let mut commit = false;
        for sub_index in 0..self.outline.subpaths.len() {
            let sub = &self.outline.subpaths[sub_index];
            for segment in 0..sub.segment_count() {
                let curve = sub.segment(segment);
                let points = (0..=24)
                    .map(|i| to_screen(curve.eval(i as f64 / 24.0)))
                    .collect::<Vec<_>>();
                painter.add(egui::Shape::line(points, Stroke::new(2.4_f32, ACCENT)));
            }
            for anchor_index in 0..sub.anchors.len() {
                let anchor = &self.outline.subpaths[sub_index].anchors[anchor_index];
                let pos = to_screen(anchor.p);
                for handle in [anchor.h_in, anchor.h_out] {
                    if handle != anchor.p {
                        let h = to_screen(handle);
                        painter.line_segment([pos, h], Stroke::new(1.0_f32, MUTED));
                        painter.circle_filled(h, 3.0, MUTED);
                    }
                }
                let key = (sub_index, anchor_index);
                let response = ui.interact(
                    Rect::from_center_size(pos, vec2(18.0, 18.0)),
                    ui.id().with(key),
                    Sense::click_and_drag(),
                );
                if response.clicked() { self.selected_point = Some(key); }
                if response.drag_started() {
                    self.selected_point = Some(key);
                    self.drag_origin = Some((key, anchor.p));
                }
                if response.dragged()
                    && let Some((drag_key, initial)) = self.drag_origin
                    && drag_key == key
                {
                    let delta = response.drag_delta();
                    let anchor = &mut self.outline.subpaths[sub_index].anchors[anchor_index];
                    let translation = Vec2::new(delta.x as f64 / scale as f64, delta.y as f64 / scale as f64);
                    let shift = initial + translation - anchor.p;
                    anchor.translate(shift);
                }
                if response.drag_stopped() {
                    self.drag_origin = None;
                    commit = true;
                }
                painter.circle_filled(
                    pos,
                    if self.selected_point == Some(key) { 7.0 } else { 5.0 },
                    if self.selected_point == Some(key) { TEXT } else { ACCENT },
                );
                response.on_hover_text(format!("Contour {}, point {}", sub_index + 1, anchor_index + 1));
            }
        }
        if self.outline.is_empty() {
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Choose a glyph or create a contour in the inspector",
                FontId::proportional(16.0),
                MUTED,
            );
        }
        commit
    }
    fn capture(&mut self, ctx: &egui::Context) {
        let Some(path) = self.capture.clone() else { return };
        let image = ctx.input(|i| i.events.iter().find_map(|event| match event {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        }));
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
                png.write_header().map_err(|e| e.to_string())?
                    .write_image_data(&pixels).map_err(|e| e.to_string())?;
                Ok(())
            })();
            match result {
                Ok(()) => println!("Captured {}", path.display()),
                Err(e) => eprintln!("Capture failed: {e}"),
            }
            self.capture = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !self.capture_requested && self.frames > 12 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.capture_requested = true;
        }
    }
}

impl eframe::App for FontApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        if self.frames == 1 {
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(TEXT);
            visuals.widgets.noninteractive.fg_stroke.color = TEXT;
            visuals.widgets.inactive.fg_stroke.color = TEXT;
            ctx.set_visuals(visuals);
        }
        self.capture(ctx);
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::none().fill(PANEL)
            .inner_margin(egui::Margin::symmetric(16.0, 10.0))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("FONTSTRIKE").size(19.0).strong().color(ACCENT));
                ui.label(egui::RichText::new("by PWDS").small().color(MUTED));
                ui.add_space(16.0);
                if ui.button("New UFO").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_file_name("Untitled.ufo").save_file()
                {
                    match FontProject::create(&path, &self.family, &self.style) {
                        Ok(project) => { self.project = Some(project); self.root = Some(path); self.refresh_glyph_list(); self.status = "New UFO saved".into(); }
                        Err(e) => self.status = e.to_string(),
                    }
                }
                if ui.button("Open UFO").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    match FontProject::open(&path) {
                        Ok(project) => { self.project = Some(project); self.root = Some(path); self.refresh_glyph_list(); self.status = "UFO opened".into(); }
                        Err(e) => self.status = e.to_string(),
                    }
                }
                if ui.add_enabled(self.project.is_some(), egui::Button::new("Undo")).clicked() { self.change_history(false); }
                if ui.add_enabled(self.project.is_some(), egui::Button::new("Redo")).clicked() { self.change_history(true); }
                if ui.add_enabled(self.project.is_some(), egui::Button::new("Export TTF")).clicked() { self.export_ttf(); }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new(self.root.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "No font open".into())).small().color(MUTED));
                });
            });
        });
        egui::SidePanel::left("glyphs").default_width(190.0).resizable(true)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::same(14.0)))
            .show(ctx, |ui| {
                ui.heading("Glyphs");
                ui.label(egui::RichText::new(&self.family).color(MUTED));
                ui.separator();
                if let Some(project) = &self.project {
                    let names = project.glyph_names();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for name in names {
                            if ui.selectable_label(self.selected == name, &name).clicked() { self.select(&name); }
                        }
                    });
                } else {
                    ui.label("Create or open a local UFO source.");
                }
            });
        egui::SidePanel::right("inspector").default_width(280.0).min_width(245.0).resizable(true)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                ui.heading("Outline & metrics");
                ui.label(egui::RichText::new("1000 units per em · source geometry").small().color(MUTED));
                ui.separator();
                if self.project.is_none() {
                    ui.label("Family"); ui.text_edit_singleline(&mut self.family);
                    ui.label("Style"); ui.text_edit_singleline(&mut self.style);
                    return;
                }
                ui.label("Glyph name"); ui.text_edit_singleline(&mut self.glyph_name);
                ui.horizontal(|ui| {
                    ui.label("Unicode"); ui.add(egui::TextEdit::singleline(&mut self.unicode).desired_width(42.0));
                    ui.label("Advance"); ui.add(egui::DragValue::new(&mut self.width).range(1.0..=4000.0));
                });
                if ui.button("Save outline & width").clicked() { self.save_outline(); }
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Quick contour").strong().color(ACCENT));
                ui.label(egui::RichText::new("Creates a closed rectangle in font units; drag its points on the canvas.").small().color(MUTED));
                egui::Grid::new("rect-fields").num_columns(2).show(ui, |ui| {
                    ui.label("Left"); ui.add(egui::DragValue::new(&mut self.rect_x0)); ui.end_row();
                    ui.label("Bottom"); ui.add(egui::DragValue::new(&mut self.rect_y0)); ui.end_row();
                    ui.label("Right"); ui.add(egui::DragValue::new(&mut self.rect_x1)); ui.end_row();
                    ui.label("Top"); ui.add(egui::DragValue::new(&mut self.rect_y1)); ui.end_row();
                });
                if ui.button("Create rectangle glyph").clicked() { self.rectangle(); }
                ui.separator();
                if let Some((sub, point)) = self.selected_point
                    && let Some(anchor) = self.outline.subpaths.get_mut(sub).and_then(|s| s.anchors.get_mut(point))
                {
                    ui.label(egui::RichText::new(format!("Point {}:{}", sub + 1, point + 1)).strong());
                    let mut x = anchor.p.x;
                    let mut y = -anchor.p.y;
                    ui.horizontal(|ui| {
                        ui.label("X"); ui.add(egui::DragValue::new(&mut x).speed(1.0));
                        ui.label("Y"); ui.add(egui::DragValue::new(&mut y).speed(1.0));
                    });
                    let delta = Point::new(x, -y) - anchor.p;
                    if delta != Vec2::ZERO { anchor.translate(delta); }
                } else {
                    ui.label(egui::RichText::new("Select a point for numeric editing.").small().color(MUTED));
                }
                ui.separator();
                ui.label(egui::RichText::new("Kerning pair").strong().color(ACCENT));
                ui.horizontal(|ui| {
                    ui.label(format!("{} +", self.selected));
                    ui.add(egui::TextEdit::singleline(&mut self.kern_right).desired_width(48.0));
                    ui.add(egui::DragValue::new(&mut self.kern_amount).range(-500.0..=500.0).suffix(" u"));
                });
                if ui.button("Save pair").clicked() { self.set_kerning(); }
                ui.label(egui::RichText::new("TTF export compiles this UFO and checks tables, mappings, and widths. Shaped proof is pending.").small().color(MUTED));
            });
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::none().fill(PANEL)
            .inner_margin(egui::Margin::symmetric(14.0, 7.0))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new("EDITABLE UFO SOURCE").small().color(ACCENT));
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG)
            .inner_margin(egui::Margin::same(8.0))).show(ctx, |ui| {
            if self.draw_canvas(ui) { self.save_outline(); }
        });
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_outline();
        }
        if self.capture.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

fn make_demo(root: &Path) -> Result<FontProject, String> {
    if root.exists() { return FontProject::open(root).map_err(|e| e.to_string()); }
    let mut project = FontProject::create(root, "Strike Sans", "Regular").map_err(|e| e.to_string())?;
    for (name, width, points) in [
        ("A", 620.0, vec![(55.0,0.0),(260.0,700.0),(350.0,700.0),(565.0,0.0),(445.0,0.0),(310.0,490.0),(175.0,0.0)]),
        ("V", 620.0, vec![(55.0,700.0),(182.0,700.0),(310.0,170.0),(438.0,700.0),(565.0,700.0),(370.0,0.0),(250.0,0.0)]),
    ] {
        let path = PathData::single(SubPath::new(points.into_iter().map(|(x,y)| Anchor::corner(Point::new(x,-y))).collect(), true));
        project.set_outline(name, name.chars().next(), width, &path).map_err(|e| e.to_string())?;
    }
    project.set_kerning("A", "V", -80.0).map_err(|e| e.to_string())?;
    project.save().map_err(|e| e.to_string())?;
    Ok(project)
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().position(|a| a == "--demo").and_then(|i| args.get(i + 1));
    let open = args.iter().position(|a| a == "--project").and_then(|i| args.get(i + 1));
    let capture = args.iter().position(|a| a == "--capture").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let root = demo.or(open).map(PathBuf::from);
    let project = root.as_ref().and_then(|path| {
        let result = if demo.is_some() { make_demo(path) } else { FontProject::open(path).map_err(|e| e.to_string()) };
        match result { Ok(project) => Some(project), Err(e) => { eprintln!("Open font: {e}"); None } }
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1320.0, 790.0]).with_min_inner_size([980.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native("FontStrike", options, Box::new(move |_cc| Ok(Box::new(FontApp::new(project, root, capture)))))
}

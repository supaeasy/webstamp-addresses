#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod fonts;
mod graphic;
#[cfg(windows)]
mod print;
mod stamp;

use config::{BlockCfg, Config, Destination};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use graphic::Graphic;
use stamp::{Block, PT_PER_MM, Placed, Raster, Template};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const PREVIEW_DPI: f32 = 150.0;
const BUTTON_BLUE: Color32 = Color32::from_rgb(37, 99, 235);

/// Aktuelles Farbschema (wird pro Frame aus dem System-Theme übernommen).
static DARK: AtomicBool = AtomicBool::new(false);

fn pick(light: (u8, u8, u8), dark: (u8, u8, u8)) -> Color32 {
    let (r, g, b) = if DARK.load(Ordering::Relaxed) { dark } else { light };
    Color32::from_rgb(r, g, b)
}
fn accent() -> Color32 { pick((37, 99, 235), (96, 165, 250)) }
fn muted() -> Color32 { pick((100, 116, 139), (148, 163, 184)) }
fn border() -> Color32 { pick((226, 232, 240), (51, 65, 85)) }
fn sidebar_bg() -> Color32 { pick((241, 245, 249), (15, 23, 42)) }
fn card_bg() -> Color32 { pick((255, 255, 255), (30, 41, 59)) }
fn desk_bg() -> Color32 { pick((203, 213, 225), (51, 65, 85)) }
fn warn() -> Color32 { pick((180, 83, 9), (251, 191, 36)) }

const PLACEHOLDER_RECIPIENT: &str = "Max Mustermann\nMusterstraße 1\n12345 Musterstadt";
const PLACEHOLDER_SENDER: &str = "Absender\nStraße 1\n12345 Ort";

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 900.0])
            .with_icon(window_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Webstamp Addresses",
        opts,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct Loaded {
    path: PathBuf,
    template: Template,
    #[cfg_attr(not(windows), allow(dead_code))] // nur der Windows-Druck braucht das Raster
    raster: Raster,
    texture: egui::TextureHandle,
}

struct LoadedImage {
    gfx: Graphic,
    texture: egui::TextureHandle,
}

/// Das einzusetzende Bild als `Placed` (nur wenn geladen und aktiviert).
fn placed<'a>(img: &'a Option<LoadedImage>, cfg: &config::ImageCfg) -> Vec<Placed<'a>> {
    match img {
        Some(im) if cfg.show => vec![Placed { img: &im.gfx, pos: cfg.pos, width_mm: cfg.width_mm }],
        _ => vec![],
    }
}

/// Zustand eines Schriftauswahl-Feldes (Eingabepuffer, Vorschlagsliste offen).
#[derive(Default)]
struct FontPicker {
    query: String,
    typed: bool,
    open: bool,
}

struct App {
    cfg: Config,
    saved_json: String,
    sender_text: String,
    recipient_text: String,
    loaded: Option<Loaded>,
    image: Option<LoadedImage>,
    #[cfg(windows)]
    print_state: print::PrintState,
    status: String,
    applied_dark: Option<bool>,
    /// Höhe des Inhalts der linken Leiste (für die Start-Fenstergröße).
    content_h: f32,
    sized: bool,
    frames: u32,
    system_fonts: Option<fonts::SystemFonts>,
    fonts_rx: Option<std::sync::mpsc::Receiver<fonts::SystemFonts>>,
    /// Schriftauswahl für Empfänger (0) und Absender (1).
    pickers: [FontPicker; 2],
    fonts_dirty: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_fonts(fonts::base_fonts());
        let cfg = Config::load();
        // Installierte Schriften im Hintergrund einlesen.
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(fonts::SystemFonts::load());
            ctx.request_repaint();
        });
        let mut app = Self {
            sender_text: cfg.sender.clone(),
            saved_json: serde_json::to_string(&cfg).unwrap(),
            cfg,
            recipient_text: String::new(),
            loaded: None,
            image: None,
            #[cfg(windows)]
            print_state: Default::default(),
            applied_dark: None,
            content_h: 0.0,
            sized: false,
            frames: 0,
            system_fonts: None,
            fonts_rx: Some(rx),
            pickers: Default::default(),
            fonts_dirty: false,
            status: "Stempel-PDF per Drag & Drop oder über „Öffnen“ laden.".into(),
        };
        if let Some(p) = app.cfg.image.path.clone() {
            app.load_image(&cc.egui_ctx, &p);
        }
        let arg = std::env::args_os().nth(1).map(PathBuf::from);
        if let Some(p) = arg.or_else(|| app.cfg.last_template.clone()) {
            if p.exists() {
                app.load_template(&cc.egui_ctx, &p);
            }
        }
        app
    }

    fn load_template(&mut self, ctx: &egui::Context, path: &Path) {
        let res = stamp::load(path).and_then(|t| {
            let r = stamp::render_template(&t, PREVIEW_DPI)?;
            Ok((t, r))
        });
        match res {
            Ok((template, raster)) => {
                let img = egui::ColorImage::from_rgba_unmultiplied([raster.w, raster.h], &raster.rgba);
                let texture = ctx.load_texture("template", img, egui::TextureOptions::LINEAR);
                self.status = format!(
                    "Geladen: {} ({:.0} × {:.0} mm)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    template.width_mm,
                    template.height_mm
                );
                self.cfg.last_template = Some(path.to_path_buf());
                self.loaded = Some(Loaded { path: path.to_path_buf(), template, raster, texture });
            }
            Err(e) => self.status = format!("Fehler: {e}"),
        }
    }

    fn load_image(&mut self, ctx: &egui::Context, path: &Path) {
        match Graphic::load(path) {
            Ok(gfx) => {
                // Für die Vorschau reicht eine Textur mit begrenzter Größe (SVG: höher, damit es scharf bleibt).
                let tex = gfx.preview(if gfx.is_vector() { 2048 } else { 1024 });
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [tex.width() as usize, tex.height() as usize],
                    tex.as_raw(),
                );
                let texture = ctx.load_texture("image", color, egui::TextureOptions::LINEAR);
                self.cfg.image.path = Some(path.to_path_buf());
                self.status = format!(
                    "Grafik geladen: {} ({})",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    gfx.describe()
                );
                self.image = Some(LoadedImage { gfx, texture });
            }
            Err(e) => {
                self.image = None;
                self.status = format!("Bild nicht lesbar: {e}");
            }
        }
    }

    fn pick_image(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Bilder und Vektorgrafiken", &["png", "jpg", "jpeg", "bmp", "gif", "svg", "svgz"])
            .pick_file()
        {
            self.load_image(ctx, &p);
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            self.load_template(ctx, &p);
        }
    }

    /// Adressblock mit Zeilenabstand aus den Schriftmetriken: Abstand zwischen den Unterlängen der
    /// oberen und den Oberlängen der unteren Zeile = `line_gap_mm` (Vorgabe der Post: 1 bis 1,5 mm).
    fn block(&self, idx: usize, text: &str) -> Block {
        let c = if idx == 0 { &self.cfg.recipient } else { &self.cfg.sender_block };
        let (up, down) = self
            .system_fonts
            .as_ref()
            .and_then(|sf| sf.line_metrics(&c.font))
            .unwrap_or(fonts::FALLBACK_METRICS);
        let em_mm = c.size_pt / PT_PER_MM;
        Block {
            text: text.to_string(),
            pos: c.pos,
            size_pt: c.size_pt,
            font: c.font.clone(),
            ascent_mm: up * em_mm,
            pitch_mm: (up + down) * em_mm + c.line_gap_mm,
        }
    }

    /// Die zu druckenden Blöcke (leere werden ausgelassen).
    fn blocks(&self) -> Vec<Block> {
        let mut v = vec![];
        if self.cfg.print_sender && !self.sender_text.trim().is_empty() {
            v.push(self.block(1, &self.sender_text));
        }
        if !self.recipient_text.trim().is_empty() {
            v.push(self.block(0, &self.recipient_text));
        }
        v
    }

    /// macOS/Linux: PDF erzeugen und über CUPS (`lp`) an den Standarddrucker senden.
    #[cfg(not(windows))]
    fn do_print(&mut self) {
        if self.loaded.is_none() {
            return;
        }
        let tmp = std::env::temp_dir().join("webstamp-addresses-print.pdf");
        let result = self.write_pdf(&tmp).and_then(|missing| {
            let out = std::process::Command::new("lp")
                .args(["-o", "print-scaling=none"])
                .arg(&tmp)
                .output()
                .map_err(|e| format!("„lp“ nicht verfügbar: {e}"))?;
            if out.status.success() {
                Ok(format!("{}{}", String::from_utf8_lossy(&out.stdout).trim(), font_note(&missing)))
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        });
        self.status = match result {
            Ok(msg) => format!("An den Standarddrucker gesendet ({msg})."),
            Err(e) => format!("Druckfehler: {e}"),
        };
    }

    #[cfg(windows)]
    fn do_print(&mut self) {
        let Some(l) = &self.loaded else { return };
        let blocks = self.blocks();
        let images = placed(&self.image, &self.cfg.image);
        let job = print::Job {
            images: &images,
            name: "Umschlag",
            env_w_mm: l.template.width_mm,
            env_h_mm: l.template.height_mm,
            stamp: &l.raster,
            stamp_dpi: PREVIEW_DPI,
            blocks: &blocks,
            flip_180: self.cfg.flip_180,
            offset_mm: self.cfg.print_offset,
        };
        // Für den Druck den Stempel in höherer Auflösung rendern.
        let hi = stamp::render_template(&l.template, 600.0);
        self.status = match hi {
            Ok(hi) => {
                let job = print::Job { stamp: &hi, stamp_dpi: 600.0, ..job };
                match print::print(&mut self.print_state, &job) {
                    Ok(true) => "An den Drucker gesendet.".into(),
                    Ok(false) => "Druck abgebrochen.".into(),
                    Err(e) => format!("Druckfehler: {e}"),
                }
            }
            Err(e) => format!("Fehler beim Rendern: {e}"),
        };
    }

    fn save_pdf(&mut self) {
        let Some(l) = &self.loaded else { return };
        let default = l
            .path
            .file_stem()
            .map(|s| format!("{}_umschlag.pdf", s.to_string_lossy()))
            .unwrap_or_else(|| "umschlag.pdf".into());
        if let Some(out) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(default).save_file() {
            self.status = match self.write_pdf(&out) {
                Ok(missing) => format!("Gespeichert: {}{}", out.display(), font_note(&missing)),
                Err(e) => format!("Fehler: {e}"),
            };
        }
    }

    /// Schreibt Vorlage + Adressen + Bild als PDF.
    /// Rückgabe: Schriften, die nicht eingebettet werden konnten (dafür gilt Helvetica).
    fn write_pdf(&self, out: &Path) -> Result<Vec<String>, String> {
        let l = self.loaded.as_ref().ok_or("Keine Vorlage geladen")?;
        let sf = self.system_fonts.as_ref();
        let face = |family: &str| sf.and_then(|s| s.font_data(family));
        let images = placed(&self.image, &self.cfg.image);
        stamp::export_pdf(&l.template, &self.blocks(), &images, out, &face)
    }

    /// Eingaben und Einstellungen eines Adressblocks (0 = Empfänger, 1 = Absender).
    fn block_controls(&mut self, ui: &mut egui::Ui, idx: usize) {
        let (text, cfg) = if idx == 0 {
            (&mut self.recipient_text, &mut self.cfg.recipient)
        } else {
            (&mut self.sender_text, &mut self.cfg.sender_block)
        };
        let enabled = idx == 0 || self.cfg.print_sender;
        if idx == 1 {
            ui.checkbox(&mut self.cfg.print_sender, "Absender drucken");
        }

        let (id, rows, hint) = if idx == 0 {
            ("recipient_text", 4, "Name\nStraße Nr.\nPLZ Ort")
        } else {
            ("sender_text", 3, "Absender-Adresse")
        };
        ui.add_enabled(
            enabled,
            egui::TextEdit::multiline(text)
                .id(egui::Id::new(id))
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .hint_text(hint),
        );

        // Hinweise zu den Formvorgaben der Post (drei bis sechs Zeilen, keine Leerzeilen).
        if enabled {
            let all = text.lines().count();
            let used = text.lines().filter(|l| !l.trim().is_empty()).count();
            let mut notes: Vec<String> = vec![];
            if used > 0 && !(config::MIN_LINES..=config::MAX_LINES).contains(&used) {
                notes.push(format!(
                    "Die Post verlangt {} bis {} Zeilen (aktuell {used}).",
                    config::MIN_LINES,
                    config::MAX_LINES
                ));
            }
            if text.trim_end().lines().skip_while(|l| l.trim().is_empty()).any(|l| l.trim().is_empty()) || all > used + 1 {
                notes.push("Leerzeilen sind nicht erlaubt und werden nicht gedruckt.".into());
            }
            for n in notes {
                ui.label(egui::RichText::new(format!("⚠ {n}")).small().color(warn()));
            }
        }

        ui.horizontal(|ui| {
            ui.label("Schrift");
            let changed = font_picker(
                ui,
                id,
                &mut self.pickers[idx],
                &mut cfg.font,
                self.system_fonts.as_ref(),
            );
            self.fonts_dirty |= changed;
        });
        ui.horizontal(|ui| {
            ui.label("Größe");
            egui::ComboBox::from_id_salt(("size", id))
                .width(86.0)
                .selected_text(size_label(cfg.size_pt))
                .show_ui(ui, |ui| {
                    for pt in (config::MIN_SIZE_PT as u32)..=(config::MAX_SIZE_PT as u32) {
                        ui.selectable_value(&mut cfg.size_pt, pt as f32, size_label(pt as f32));
                    }
                });
            ui.label("Zeilenabstand");
            ui.add(
                egui::DragValue::new(&mut cfg.line_gap_mm)
                    .speed(0.01)
                    .range(config::MIN_GAP_MM..=config::MAX_GAP_MM)
                    .fixed_decimals(2)
                    .suffix(" mm"),
            );
        });
        ui.label(
            egui::RichText::new("Post-Vorgaben: Grotesk-Schrift, 9–28 pt (ideal 10), linksbündig, nicht fett, Abstand Unter-/Oberlängen 1–1,5 mm.")
                .small()
                .color(muted()),
        );

        if idx == 1 {
            ui.horizontal(|ui| {
                let is_default = self.sender_text == self.cfg.sender;
                if ui.add_enabled(!is_default, egui::Button::new("Als Standard speichern")).clicked() {
                    self.cfg.sender = self.sender_text.clone();
                }
                if ui.add_enabled(!is_default, egui::Button::new("Standard laden")).clicked() {
                    self.sender_text = self.cfg.sender.clone();
                }
            });
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Kopfbereich
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("✉").size(26.0).color(accent()));
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Webstamp Addresses").strong().size(17.0));
                let name = self
                    .loaded
                    .as_ref()
                    .and_then(|l| l.path.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Keine Vorlage geladen".into());
                ui.label(egui::RichText::new(name).small().color(muted()));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Öffnen…").clicked() {
                    self.open_dialog(&ctx);
                }
            });
        });
        ui.add_space(4.0);

        card(ui, "Empfänger", |ui| self.block_controls(ui, 0));
        card(ui, "Absender", |ui| self.block_controls(ui, 1));

        card(ui, "Bild", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Bild wählen…").clicked() {
                    self.pick_image(&ctx);
                }
                if ui.add_enabled(self.image.is_some(), egui::Button::new("Entfernen")).clicked() {
                    self.image = None;
                    self.cfg.image.path = None;
                }
            });
            let name = self
                .cfg
                .image
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Kein Bild gewählt (PNG, JPG, SVG …)".into());
            ui.label(egui::RichText::new(name).small().color(muted()));
            ui.add_enabled_ui(self.image.is_some(), |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cfg.image.show, "Drucken");
                    ui.label("Breite");
                    ui.add(egui::DragValue::new(&mut self.cfg.image.width_mm).speed(0.5).range(5.0..=200.0).suffix(" mm"));
                });
            });
        });

        card(ui, "Positionen", |ui| {
            ui.label(egui::RichText::new("Blöcke lassen sich auch in der Vorschau ziehen.").small().color(muted()));
            egui::Grid::new("pos").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Empfänger");
                pos_drag(ui, &mut self.cfg.recipient.pos);
                ui.end_row();
                ui.label("Absender");
                pos_drag(ui, &mut self.cfg.sender_block.pos);
                ui.end_row();
                if self.image.is_some() {
                    ui.label("Bild");
                    pos_drag(ui, &mut self.cfg.image.pos);
                    ui.end_row();
                }
            });
            ui.checkbox(&mut self.cfg.show_zones, "Zonen der Schweizer Post (nur Vorschau)");
            ui.checkbox(&mut self.cfg.show_guides, "Hilfslinien (nur Vorschau)");
            if self.cfg.show_guides {
                ui.horizontal(|ui| {
                    ui.label("Horizontal");
                    ui.add(egui::DragValue::new(&mut self.cfg.guide_h).speed(0.2).suffix(" mm").fixed_decimals(1));
                    ui.label("Vertikal");
                    ui.add(egui::DragValue::new(&mut self.cfg.guide_v).speed(0.2).suffix(" mm").fixed_decimals(1));
                });
            }
            if ui.button("Positionen zurücksetzen").clicked() {
                self.cfg.recipient.pos = BlockCfg::recipient().pos;
                self.cfg.sender_block.pos = BlockCfg::sender().pos;
            }
        });

        egui::Frame::new()
            .fill(card_bg())
            .stroke(Stroke::new(1.0, border()))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let title = egui::RichText::new("Druckeinstellungen").strong().size(14.0).color(accent());
                egui::CollapsingHeader::new(title).show(ui, |ui| {
                    ui.checkbox(&mut self.cfg.flip_180, "Um 180° drehen (Einzug)");
                    ui.horizontal(|ui| {
                        ui.label("Versatz");
                        ui.add(egui::DragValue::new(&mut self.cfg.print_offset[0]).speed(0.1).prefix("x ").suffix(" mm"));
                        ui.add(egui::DragValue::new(&mut self.cfg.print_offset[1]).speed(0.1).prefix("y ").suffix(" mm"));
                    });
                    ui.label(egui::RichText::new("Im Druckdialog unter „Eigenschaften“ Papierformat C5 wählen.").small().color(muted()));
                });
            });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Webstamp-PDF hierher ziehen\noder links „Öffnen…“ wählen")
                        .size(18.0)
                        .color(muted()),
                )
            });
            return;
        };
        let (ew, eh) = (l.template.width_mm, l.template.height_mm);
        let avail = ui.available_rect_before_wrap().shrink(12.0);
        let s = (avail.width() / ew).min(avail.height() / eh); // px pro mm
        let env = Rect::from_center_size(avail.center(), Vec2::new(ew * s, eh * s));

        let painter = ui.painter_at(ui.available_rect_before_wrap());
        for (grow, a) in [(10.0, 10u8), (6.0, 14), (3.0, 20)] {
            painter.rect_filled(env.expand(grow).translate(Vec2::new(0.0, 4.0)), 6.0, Color32::from_black_alpha(a));
        }
        painter.image(
            l.texture.id(),
            env,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(env, 0.0, Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Middle);

        if self.cfg.show_zones {
            // Swiss Post, „Spezifikationen Briefgestaltung“ (Format bis B5, quer), Maße in mm.
            let blue = Color32::from_rgb(56, 140, 235);
            let fill = Color32::from_rgba_unmultiplied(56, 140, 235, 45);
            let rect_mm = |x0: f32, y0: f32, x1: f32, y1: f32| {
                Rect::from_min_max(env.min + Vec2::new(x0, y0) * s, env.min + Vec2::new(x1, y1) * s)
            };
            let mut zones = vec![
                ("Frankierzone", rect_mm(ew - 74.0, 0.0, ew, 38.0), true),
                ("Codierzone (frei lassen)", rect_mm(ew - 140.0, eh - 15.0, ew, eh), true),
            ];
            match self.cfg.destination {
                Destination::Domestic => {
                    zones.push(("Absenderzone", rect_mm(0.0, 0.0, 120.0, 40.0), true));
                    zones.push(("Lesezone: Empfängeradresse hier hinein", rect_mm(12.0, 40.0, ew - 12.0, eh - 15.0), false));
                }
                Destination::Foreign => {
                    // Ausland, „rechts adressiert“: L-förmige Absenderzone, Adressfeld rechts daneben
                    // (20 mm Abstand zur Absenderzone, 10 mm unter ihrem oberen Teil, 12 mm Rand rechts).
                    // Die Ausdehnung der Absenderzone ist aus den Proportionen der Grafik abgeleitet.
                    let (top, strip) = (rect_mm(0.0, 0.0, 80.0, 50.0), rect_mm(0.0, 50.0, 58.0, 98.0));
                    painter.rect_filled(top, 0.0, fill);
                    painter.rect_filled(strip, 0.0, fill);
                    let at = |x: f32, y: f32| env.min + Vec2::new(x, y) * s;
                    painter.add(egui::Shape::closed_line(
                        vec![at(0.0, 0.0), at(80.0, 0.0), at(80.0, 50.0), at(58.0, 50.0), at(58.0, 98.0), at(0.0, 98.0)],
                        Stroke::new(1.0, blue),
                    ));
                    painter.text(top.min + Vec2::new(5.0, 4.0), Align2::LEFT_TOP, "Absenderzone (Richtwert)", FontId::proportional(11.0), blue);
                    zones.push(("Adressfeld: Empfängeradresse hier hinein", rect_mm(78.0, 60.0, ew - 12.0, eh - 15.0), false));
                }
            }
            for (label, r, filled) in zones {
                if filled {
                    painter.rect_filled(r, 0.0, fill);
                }
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, blue), egui::StrokeKind::Inside);
                painter.text(r.min + Vec2::new(5.0, 4.0), Align2::LEFT_TOP, label, FontId::proportional(11.0), blue);
            }
        }

        if self.cfg.show_guides {
            let col = Color32::from_rgb(230, 60, 200);
            let y = env.min.y + self.cfg.guide_h * s;
            painter.line_segment([Pos2::new(env.min.x, y), Pos2::new(env.max.x, y)], Stroke::new(1.0, col));
            let r = Rect::from_min_max(Pos2::new(env.min.x, y - 4.0), Pos2::new(env.max.x, y + 4.0));
            let resp = ui.interact(r, egui::Id::new("guide_h"), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if resp.dragged() {
                self.cfg.guide_h = (self.cfg.guide_h + resp.drag_delta().y / s).clamp(0.0, eh);
            }
            painter.text(Pos2::new(env.min.x + 4.0, y - 2.0), Align2::LEFT_BOTTOM, format!("{:.1} mm", self.cfg.guide_h), FontId::proportional(11.0), col);

            let x = env.min.x + self.cfg.guide_v * s;
            painter.line_segment([Pos2::new(x, env.min.y), Pos2::new(x, env.max.y)], Stroke::new(1.0, col));
            let r = Rect::from_min_max(Pos2::new(x - 4.0, env.min.y), Pos2::new(x + 4.0, env.max.y));
            let resp = ui.interact(r, egui::Id::new("guide_v"), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.dragged() {
                self.cfg.guide_v = (self.cfg.guide_v + resp.drag_delta().x / s).clamp(0.0, ew);
            }
            painter.text(Pos2::new(x + 4.0, env.min.y + 2.0), Align2::LEFT_TOP, format!("{:.1} mm", self.cfg.guide_v), FontId::proportional(11.0), col);
        }

        // Bild: verschieben (Fläche) und skalieren (Eckgriff unten rechts, Seitenverhältnis bleibt).
        if let (Some(im), true) = (&self.image, self.cfg.image.show) {
            let w_mm = self.cfg.image.width_mm;
            let h_mm = w_mm * im.gfx.aspect();
            let rect = Rect::from_min_size(
                env.min + Vec2::new(self.cfg.image.pos[0], self.cfg.image.pos[1]) * s,
                Vec2::new(w_mm * s, h_mm * s),
            );
            painter.image(im.texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let body = ui.interact(rect, egui::Id::new("image_body"), Sense::click_and_drag());
            let handle = Rect::from_center_size(rect.right_bottom(), Vec2::splat(14.0));
            let grip = ui.interact(handle, egui::Id::new("image_resize"), Sense::drag());
            let blue = Color32::from_rgb(40, 120, 220);
            if body.hovered() || body.dragged() || grip.hovered() || grip.dragged() {
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, blue), egui::StrokeKind::Middle);
                painter.rect_filled(handle.shrink(2.0), 2.0, blue);
            }
            if grip.hovered() || grip.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            } else if body.hovered() || body.dragged() {
                ui.ctx().set_cursor_icon(if body.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            }
            if grip.dragged() {
                self.cfg.image.width_mm = (w_mm + grip.drag_delta().x / s).clamp(5.0, ew);
            } else if body.dragged() {
                let d = body.drag_delta() / s;
                let pos = &mut self.cfg.image.pos;
                pos[0] = (pos[0] + d.x).clamp(0.0, ew - 5.0);
                pos[1] = (pos[1] + d.y).clamp(0.0, eh - 5.0);
            }
        }

        let px_per_pt = s / PT_PER_MM;

        // (Block-Index, Block, Platzhalter?)
        let mut blocks: Vec<(usize, Block, bool)> = vec![];
        let empty = self.recipient_text.trim().is_empty();
        blocks.push((0, self.block(0, if empty { PLACEHOLDER_RECIPIENT } else { &self.recipient_text }), empty));
        if self.cfg.print_sender {
            let empty = self.sender_text.trim().is_empty();
            blocks.push((1, self.block(1, if empty { PLACEHOLDER_SENDER } else { &self.sender_text }), empty));
        }

        for (idx, b, ghost) in blocks {
            let color = if ghost { Color32::from_gray(160) } else { Color32::BLACK };

            let mut width_px = 0.0f32;
            for r in &stamp::layout(&b) {
                let g = painter.layout_no_wrap(
                    r.text.clone(),
                    FontId::new(b.size_pt * px_per_pt, fonts::family(idx)),
                    color,
                );
                let baseline = g.rows.first().and_then(|row| row.glyphs.first()).map_or(0.905 * b.size_pt * px_per_pt, |gl| gl.pos.y);
                width_px = width_px.max(g.size().x);
                let at = env.min + Vec2::new(r.x_mm * s, r.baseline_mm * s - baseline);
                painter.galley(at, g, color);
            }

            // Anfasser: von der Oberkante der Oberlängen bis zur Unterkante der letzten Unterlänge.
            let n = b.lines().len().max(1) as f32;
            let height_mm = b.ascent_mm + (n - 1.0) * b.pitch_mm + (b.pitch_mm - b.ascent_mm).max(2.0);
            let top = env.min + Vec2::new(b.pos[0], b.pos[1]) * s;
            let hit = Rect::from_min_size(top, Vec2::new(width_px.max(24.0), height_mm * s)).expand(4.0);
            let resp = ui.interact(hit, egui::Id::new(("block", idx)), Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                painter.rect_stroke(hit, 2.0, Stroke::new(1.0, Color32::from_rgb(40, 120, 220)), egui::StrokeKind::Middle);
                ui.ctx().set_cursor_icon(if resp.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                });
            }
            if resp.dragged() {
                let d = resp.drag_delta() / s;
                let pos = if idx == 1 { &mut self.cfg.sender_block.pos } else { &mut self.cfg.recipient.pos };
                pos[0] = (pos[0] + d.x).clamp(0.0, ew - 5.0);
                pos[1] = (pos[1] + d.y).clamp(0.0, eh - 5.0);
            }
        }
    }
}

/// Hinweis für die Statusleiste, wenn Schriften nicht eingebettet werden konnten.
fn font_note(missing: &[String]) -> String {
    if missing.is_empty() {
        " – Schrift eingebettet".into()
    } else {
        format!(" – Ersatzschrift Helvetica für: {}", missing.join(", "))
    }
}

/// Anzeige einer Schriftgröße; 10 pt ist die von der Post empfohlene.
fn size_label(pt: f32) -> String {
    if (pt - config::IDEAL_SIZE_PT).abs() < 0.01 { format!("{pt:.0} pt (ideal)") } else { format!("{pt:.0} pt") }
}

/// Eingabefeld mit Autovervollständigung über alle installierten Schriftarten.
/// Liefert `true`, wenn eine neue Schrift gewählt wurde.
fn font_picker(
    ui: &mut egui::Ui,
    id: &str,
    st: &mut FontPicker,
    current: &mut String,
    sf: Option<&fonts::SystemFonts>,
) -> bool {
    let ctx = ui.ctx().clone();
    let Some(sf) = sf else {
        ui.label(egui::RichText::new("Schriften werden geladen…").small().color(muted()));
        return false;
    };
    if !st.open && st.query.is_empty() {
        st.query = current.clone();
    }
    let resp = ui.add(
        egui::TextEdit::singleline(&mut st.query)
            .id_salt(("font_edit", id))
            .desired_width(190.0)
            .hint_text("Grotesk-Schrift suchen…"),
    );
    if resp.gained_focus() {
        st.open = true;
        st.typed = false;
    }
    if resp.changed() {
        st.open = true;
        st.typed = true;
    }

    let mut chosen: Option<String> = None;
    let q = st.query.trim().to_lowercase();
    let matches: Vec<&String> = if st.typed && !q.is_empty() {
        let (mut pre, mut rest): (Vec<&String>, Vec<&String>) = sf
            .allowed
            .iter()
            .filter(|f| f.to_lowercase().contains(&q))
            .partition(|f| f.to_lowercase().starts_with(&q));
        pre.append(&mut rest);
        pre
    } else {
        sf.allowed.iter().collect()
    };

    if resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        chosen = sf
            .canonical_allowed(&st.query)
            .map(str::to_owned)
            .or_else(|| matches.first().map(|s| (*s).clone()));
        if chosen.is_none() {
            st.open = false;
        }
    }

    if st.open {
        let area = egui::Area::new(egui::Id::new(("font_popup", id)))
            .order(egui::Order::Foreground)
            .fixed_pos(resp.rect.left_bottom() + Vec2::new(0.0, 2.0))
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(resp.rect.width());
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        if matches.is_empty() {
                            ui.label(egui::RichText::new("Keine passende erlaubte Schrift installiert").color(muted()));
                        }
                        for name in &matches {
                            if ui.selectable_label(name.as_str() == current, name.as_str()).clicked() {
                                chosen = Some((*name).clone());
                            }
                        }
                    });
                });
            });
        // Klick außerhalb schließt die Liste.
        let outside = ctx.input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !resp.rect.contains(p) && !area.response.rect.contains(p))
        });
        if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            st.open = false;
        }
    }

    if let Some(name) = chosen {
        st.query = name.clone();
        let changed = *current != name;
        *current = name;
        st.open = false;
        st.typed = false;
        return changed;
    }
    if !st.open && !resp.has_focus() {
        // Ungültige Eingabe verwerfen, aktuelle Schrift wieder anzeigen.
        st.query = current.clone();
    }
    false
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(card_bg())
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).strong().size(14.0).color(accent()));
            ui.add_space(2.0);
            add(ui);
        });
    ui.add_space(4.0);
}

fn setup_style(ctx: &egui::Context, dark: bool) {
    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.interact_size.y = 26.0;
        let v = &mut style.visuals;
        v.selection.bg_fill = BUTTON_BLUE;
        v.hyperlink_color = accent();
        v.extreme_bg_color = if dark { Color32::from_rgb(15, 23, 42) } else { Color32::from_rgb(248, 250, 252) };
        let r = egui::CornerRadius::same(7);
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.inactive.corner_radius = r;
        v.widgets.hovered.corner_radius = r;
        v.widgets.active.corner_radius = r;
        v.widgets.inactive.weak_bg_fill = if dark { Color32::from_rgb(51, 65, 85) } else { Color32::from_rgb(241, 245, 249) };
        v.widgets.hovered.weak_bg_fill = if dark { Color32::from_rgb(71, 85, 105) } else { Color32::from_rgb(226, 232, 240) };
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, border());
        for (ts, size) in [
            (egui::TextStyle::Body, 14.0),
            (egui::TextStyle::Button, 14.0),
            (egui::TextStyle::Small, 12.0),
        ] {
            if let Some(f) = style.text_styles.get_mut(&ts) {
                f.size = size;
            }
        }
    });
}

fn pos_drag(ui: &mut egui::Ui, p: &mut [f32; 2]) {
    ui.add(egui::DragValue::new(&mut p[0]).speed(0.2).prefix("x ").suffix(" mm").fixed_decimals(1));
    ui.add(egui::DragValue::new(&mut p[1]).speed(0.2).prefix("y ").suffix(" mm").fixed_decimals(1));
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dark = ui.visuals().dark_mode;
        DARK.store(dark, Ordering::Relaxed);

        // Installierte Schriften sind geladen: Namen normalisieren, Vorschau-Schriften setzen.
        if let Some(rx) = &self.fonts_rx {
            if let Ok(sf) = rx.try_recv() {
                for (cfg, picker) in [&mut self.cfg.recipient, &mut self.cfg.sender_block].into_iter().zip(&mut self.pickers) {
                    // Nur erlaubte Grotesk-Schriften; sonst die erste installierte erlaubte (meist Arial).
                    if let Some(name) = sf
                        .canonical_allowed(&cfg.font)
                        .map(str::to_owned)
                        .or_else(|| sf.allowed.first().cloned())
                    {
                        cfg.font = name;
                    }
                    picker.query = cfg.font.clone();
                }
                self.system_fonts = Some(sf);
                self.fonts_rx = None;
                self.fonts_dirty = true;
            }
        }
        if self.fonts_dirty {
            fonts::apply_preview_fonts(
                &ctx,
                self.system_fonts.as_ref(),
                [&self.cfg.recipient.font, &self.cfg.sender_block.font],
            );
            self.fonts_dirty = false;
        }

        // Einmalig: Fenster so hoch öffnen, dass die linke Leiste ohne Scrollen passt.
        self.frames += 1;
        if !self.sized && self.frames < 4 {
            ctx.request_repaint();
        }
        if !self.sized && self.frames >= 4 && self.content_h > 0.0 {
            self.sized = true;
            let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1280.0, 720.0));
            let want = self.content_h + 24.0 + 56.0; // Rand + untere Leiste
            let size = Vec2::new(1180.0f32.min(monitor.x - 40.0), want.min(monitor.y - 80.0).max(600.0));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            // Oben mittig platzieren (Titelleiste ~40 pt).
            let pos = Pos2::new(((monitor.x - size.x) / 2.0).max(0.0), ((monitor.y - size.y - 40.0) / 2.0).max(0.0));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
        if self.applied_dark != Some(dark) {
            setup_style(&ctx, dark);
            self.applied_dark = Some(dark);
        }
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.iter().next().map(|f| f.path().to_path_buf())) {
            self.load_template(&ctx, &path);
        }

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(card_bg())
                    .stroke(Stroke::new(1.0, border()))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let have = self.loaded.is_some();
                    let print_btn = egui::Button::new(egui::RichText::new("Drucken…").strong().color(Color32::WHITE))
                        .fill(BUTTON_BLUE)
                        .min_size(Vec2::new(130.0, 32.0));
                    if ui.add_enabled(have, print_btn).clicked() {
                        self.do_print();
                    }
                    if ui.add_enabled(have, egui::Button::new("Als PDF speichern…").min_size(Vec2::new(0.0, 32.0))).clicked() {
                        self.save_pdf();
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&self.status).small().color(muted()));
                });
            });
        egui::Panel::left("controls")
            .min_size(340.0)
            .frame(egui::Frame::new().fill(sidebar_bg()).inner_margin(12))
            .show(ui, |ui| {
                let out = egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
                self.content_h = out.content_size.y;
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(desk_bg()))
            .show(ui, |ui| {
                // Art der Sendung: bestimmt die Zonen der Post in der Vorschau.
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Sendung:").strong());
                    ui.selectable_value(&mut self.cfg.destination, Destination::Domestic, "Inland");
                    ui.selectable_value(&mut self.cfg.destination, Destination::Foreign, "Ausland");
                });
                self.preview(ui)
            });

        // Einstellungen automatisch sichern, sobald sich etwas geändert hat.
        let json = serde_json::to_string(&self.cfg).unwrap();
        if json != self.saved_json && !ctx.input(|i| i.pointer.any_down()) {
            if let Err(e) = self.cfg.save() {
                self.status = format!("Einstellungen nicht gespeichert: {e}");
            }
            self.saved_json = json;
        }
    }
}

fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("icon.png")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod fonts;
mod graphic;
mod print;
mod stamp;

use config::{BlockCfg, Config};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use graphic::Graphic;
use stamp::{Align, Block, PT_PER_MM, Placed, Raster, Template};
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

    fn block(&self, idx: usize, text: &str) -> Block {
        let c = if idx == 0 { &self.cfg.recipient } else { &self.cfg.sender_block };
        Block {
            text: text.to_string(),
            pos: c.pos,
            width_mm: c.width_mm,
            size_pt: c.size_pt,
            align: c.align,
            font: c.font.clone(),
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
            let sf = self.system_fonts.as_ref();
            // Das PDF nutzt Helvetica (Arial-Metrik) – Breiten daher mit Arial messen.
            let mut measure = |b: &Block, text: &str, bold: bool| match sf {
                Some(sf) => sf.text_width_mm("Arial", bold, text, b.size_pt),
                None => text.chars().count() as f32 * 0.5 * b.size_pt / PT_PER_MM,
            };
            let images = placed(&self.image, &self.cfg.image);
            self.status = match stamp::export_pdf(&l.template, &self.blocks(), &images, &out, &mut measure) {
                Ok(()) => format!("Gespeichert: {} (Schrift: Helvetica)", out.display()),
                Err(e) => format!("Fehler: {e}"),
            };
        }
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
        address_editor(ui, egui::Id::new(id), text, rows, hint, enabled, &mut cfg.align);

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
            ui.add(egui::DragValue::new(&mut cfg.size_pt).speed(0.1).range(5.0..=40.0).suffix(" pt"));
            ui.label("Breite");
            ui.add(egui::DragValue::new(&mut cfg.width_mm).speed(0.5).range(20.0..=200.0).suffix(" mm"));
        });

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
            let zones = [
                ("Frankierzone", rect_mm(ew - 74.0, 0.0, ew, 38.0), true),
                ("Absenderzone", rect_mm(0.0, 0.0, 120.0, 40.0), true),
                ("Codierzone (frei lassen)", rect_mm(ew - 140.0, eh - 15.0, ew, eh), true),
                ("Lesezone: Empfängeradresse hier hinein", rect_mm(12.0, 40.0, ew - 12.0, eh - 15.0), false),
            ];
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

            // Textbreiten aus den Vorschau-Schriften (bei 10-facher Größe gemessen, genauer).
            let runs = stamp::layout(&b, &mut |text, bold| {
                let g = painter.layout_no_wrap(
                    text.to_string(),
                    FontId::new(b.size_pt * 10.0, fonts::family(idx, bold)),
                    Color32::BLACK,
                );
                g.size().x / 10.0 / PT_PER_MM
            });
            for r in &runs {
                let g = painter.layout_no_wrap(
                    r.text.clone(),
                    FontId::new(b.size_pt * px_per_pt, fonts::family(idx, r.bold)),
                    color,
                );
                let baseline = g.rows.first().and_then(|row| row.glyphs.first()).map_or(0.905 * b.size_pt * px_per_pt, |gl| gl.pos.y);
                let at = env.min + Vec2::new(r.x_mm * s, r.baseline_mm * s - baseline);
                painter.galley(at, g, color);
            }

            let lines = b.text.lines().count().max(1) as f32;
            let top = env.min + Vec2::new(b.pos[0], b.pos[1]) * s;
            let hit = Rect::from_min_size(top, Vec2::new(b.width_mm * s, lines * b.line_height_mm() * s)).expand(4.0);
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

/// Mehrzeiliges Adressfeld mit Werkzeugleiste: Fett (Strg+B) und Ausrichtung.
fn address_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    rows: usize,
    hint: &str,
    enabled: bool,
    align: &mut Align,
) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        let bold_btn = ui
            .add_enabled(enabled, egui::Button::new(egui::RichText::new("B").strong()).min_size(Vec2::new(28.0, 0.0)))
            .on_hover_text("Markierten Text fett drucken (Strg+B)");
        if bold_btn.clicked() {
            bold_selection(&ctx, id, text);
        }
        ui.separator();
        ui.selectable_value(align, Align::Left, "Links");
        ui.selectable_value(align, Align::Justify, "Blocksatz");
        ui.selectable_value(align, Align::Right, "Rechts");
    });
    if enabled && ctx.memory(|m| m.has_focus(id)) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
        bold_selection(&ctx, id, text);
    }
    ui.add_enabled(
        enabled,
        egui::TextEdit::multiline(text)
            .id(id)
            .desired_rows(rows)
            .desired_width(f32::INFINITY)
            .hint_text(hint),
    );
}

/// Setzt/entfernt `**` um die aktuell markierte Auswahl des Textfeldes.
fn bold_selection(ctx: &egui::Context, id: egui::Id, text: &mut String) {
    let Some(mut state) = egui::TextEdit::load_state(ctx, id) else { return };
    let Some(range) = state.cursor.char_range() else { return };
    let (a, b) = stamp::toggle_bold(text, (range.primary.index.into(), range.secondary.index.into()));
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(a),
        egui::text::CCursor::new(b),
    )));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
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
            .hint_text("Schriftart suchen…"),
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
            .families
            .iter()
            .filter(|f| f.to_lowercase().contains(&q))
            .partition(|f| f.to_lowercase().starts_with(&q));
        pre.append(&mut rest);
        pre
    } else {
        sf.families.iter().collect()
    };

    if resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        chosen = sf
            .canonical(&st.query)
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
                            ui.label(egui::RichText::new("Keine passende Schrift").color(muted()));
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
                    if let Some(name) = sf.canonical(&cfg.font).map(str::to_owned) {
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
            .show(ui, |ui| self.preview(ui));

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

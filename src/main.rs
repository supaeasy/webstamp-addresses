#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod print;
mod stamp;

use config::Config;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use stamp::{Block, PT_PER_MM, Raster, Template};
use std::path::{Path, PathBuf};

const PREVIEW_DPI: f32 = 150.0;
const ACCENT: Color32 = Color32::from_rgb(37, 99, 235);
const MUTED: Color32 = Color32::from_rgb(100, 116, 139);
const BORDER: Color32 = Color32::from_rgb(226, 232, 240);
const SIDEBAR_BG: Color32 = Color32::from_rgb(241, 245, 249);
const DESK_BG: Color32 = Color32::from_rgb(203, 213, 225);
const PLACEHOLDER: &str = "Max Mustermann\nMusterstraße 1\n12345 Musterstadt";

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
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

struct App {
    cfg: Config,
    saved_json: String,
    sender_text: String,
    recipient_text: String,
    loaded: Option<Loaded>,
    print_state: print::PrintState,
    status: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_style(&cc.egui_ctx);
        let cfg = Config::load();
        let mut app = Self {
            sender_text: cfg.sender.clone(),
            saved_json: serde_json::to_string(&cfg).unwrap(),
            cfg,
            recipient_text: String::new(),
            loaded: None,
            print_state: Default::default(),
            status: "Stempel-PDF per Drag & Drop oder über „Öffnen“ laden.".into(),
        };
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

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            self.load_template(ctx, &p);
        }
    }

    fn blocks(&self) -> Vec<Block> {
        let mut v = vec![];
        if self.cfg.print_sender && !self.sender_text.trim().is_empty() {
            v.push(Block {
                text: self.sender_text.clone(),
                pos: self.cfg.sender_pos,
                size_pt: self.cfg.sender_size_pt,
            });
        }
        if !self.recipient_text.trim().is_empty() {
            v.push(Block {
                text: self.recipient_text.clone(),
                pos: self.cfg.recipient_pos,
                size_pt: self.cfg.recipient_size_pt,
            });
        }
        v
    }

    fn do_print(&mut self) {
        let Some(l) = &self.loaded else { return };
        let blocks = self.blocks();
        let job = print::Job {
            name: "Umschlag",
            env_w_mm: l.template.width_mm,
            env_h_mm: l.template.height_mm,
            stamp: &l.raster,
            stamp_dpi: PREVIEW_DPI,
            blocks: &blocks,
            font: &self.cfg.font,
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
            self.status = match stamp::export_pdf(&l.template, &self.blocks(), &out) {
                Ok(()) => format!("Gespeichert: {}", out.display()),
                Err(e) => format!("Fehler: {e}"),
            };
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let have = self.loaded.is_some();

        // Kopfbereich
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("✉").size(26.0).color(ACCENT));
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Webstamp Addresses").strong().size(17.0));
                let name = self
                    .loaded
                    .as_ref()
                    .and_then(|l| l.path.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Keine Vorlage geladen".into());
                ui.label(egui::RichText::new(name).small().color(MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Öffnen…").clicked() {
                    self.open_dialog(&ctx);
                }
            });
        });
        ui.add_space(4.0);

        card(ui, "Empfänger", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.recipient_text)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .hint_text("Name\nStraße Nr.\nPLZ Ort"),
            );
            size_row(ui, "Schriftgröße", &mut self.cfg.recipient_size_pt);
        });

        card(ui, "Absender", |ui| {
            ui.checkbox(&mut self.cfg.print_sender, "Absender drucken");
            ui.add_enabled(
                self.cfg.print_sender,
                egui::TextEdit::multiline(&mut self.sender_text)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("Absender-Adresse"),
            );
            size_row(ui, "Schriftgröße", &mut self.cfg.sender_size_pt);
            ui.horizontal(|ui| {
                let is_default = self.sender_text == self.cfg.sender;
                if ui.add_enabled(!is_default, egui::Button::new("Als Standard speichern")).clicked() {
                    self.cfg.sender = self.sender_text.clone();
                }
                if ui.add_enabled(!is_default, egui::Button::new("Standard laden")).clicked() {
                    self.sender_text = self.cfg.sender.clone();
                }
            });
        });

        card(ui, "Positionen", |ui| {
            ui.label(egui::RichText::new("Blöcke lassen sich auch in der Vorschau ziehen.").small().color(MUTED));
            egui::Grid::new("pos").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Empfänger");
                pos_drag(ui, &mut self.cfg.recipient_pos);
                ui.end_row();
                ui.label("Absender");
                pos_drag(ui, &mut self.cfg.sender_pos);
                ui.end_row();
            });
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
                let d = Config::default();
                self.cfg.recipient_pos = d.recipient_pos;
                self.cfg.sender_pos = d.sender_pos;
            }
        });

        card(ui, "Druckeinstellungen", |ui| {
            ui.horizontal(|ui| {
                ui.label("Schriftart");
                ui.add(egui::TextEdit::singleline(&mut self.cfg.font).desired_width(140.0));
            });
            ui.checkbox(&mut self.cfg.flip_180, "Um 180° drehen (Einzug)");
            ui.horizontal(|ui| {
                ui.label("Versatz");
                ui.add(egui::DragValue::new(&mut self.cfg.print_offset[0]).speed(0.1).prefix("x ").suffix(" mm"));
                ui.add(egui::DragValue::new(&mut self.cfg.print_offset[1]).speed(0.1).prefix("y ").suffix(" mm"));
            });
            ui.label(egui::RichText::new("Im Druckdialog unter „Eigenschaften“ Papierformat C5 wählen.").small().color(MUTED));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let print_btn = egui::Button::new(egui::RichText::new("Drucken…").strong().color(Color32::WHITE))
                .fill(ACCENT)
                .min_size(Vec2::new(140.0, 36.0));
            if ui.add_enabled(have, print_btn).clicked() {
                self.do_print();
            }
            if ui.add_enabled(have, egui::Button::new("Als PDF speichern…").min_size(Vec2::new(0.0, 36.0))).clicked() {
                self.save_pdf();
            }
        });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Webstamp-PDF hierher ziehen\noder links „Öffnen…“ wählen")
                        .size(18.0)
                        .color(MUTED),
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
        let px_per_pt = s / PT_PER_MM;
        let to_screen = |mm: [f32; 2]| env.min + Vec2::new(mm[0], mm[1]) * s;

        let mut blocks: Vec<(&'static str, Block, bool)> = vec![];
        let r_text = if self.recipient_text.trim().is_empty() { PLACEHOLDER } else { &self.recipient_text };
        blocks.push((
            "recipient",
            Block { text: r_text.into(), pos: self.cfg.recipient_pos, size_pt: self.cfg.recipient_size_pt },
            self.recipient_text.trim().is_empty(),
        ));
        if self.cfg.print_sender {
            let empty = self.sender_text.trim().is_empty();
            let t = if empty { "Absender\nStraße 1\n12345 Ort" } else { &self.sender_text };
            blocks.push((
                "sender",
                Block { text: t.into(), pos: self.cfg.sender_pos, size_pt: self.cfg.sender_size_pt },
                empty,
            ));
        }

        for (id, b, ghost) in blocks {
            let font = FontId::proportional(b.size_pt * px_per_pt);
            let color = if ghost { Color32::from_gray(160) } else { Color32::BLACK };
            let mut width = 0.0f32;
            let top = to_screen(b.pos);
            for (i, line) in b.text.lines().enumerate() {
                let y = top.y + i as f32 * b.pitch_pt() * px_per_pt;
                let r = painter.text(Pos2::new(top.x, y), Align2::LEFT_TOP, line, font.clone(), color);
                width = width.max(r.width());
            }
            let height = b.text.lines().count() as f32 * b.pitch_pt() * px_per_pt;
            let hit = Rect::from_min_size(top, Vec2::new(width.max(20.0), height)).expand(4.0);
            let resp = ui.interact(hit, egui::Id::new(id), Sense::click_and_drag());
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
                let pos = if id == "sender" { &mut self.cfg.sender_pos } else { &mut self.cfg.recipient_pos };
                pos[0] = (pos[0] + d.x).clamp(0.0, ew - 5.0);
                pos[1] = (pos[1] + d.y).clamp(0.0, eh - 5.0);
            }
        }
    }
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).strong().size(14.0).color(ACCENT));
            ui.add_space(2.0);
            add(ui);
        });
    ui.add_space(4.0);
}

fn setup_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.interact_size.y = 26.0;
        let v = &mut style.visuals;
        v.selection.bg_fill = ACCENT;
        v.hyperlink_color = ACCENT;
        v.extreme_bg_color = Color32::from_rgb(248, 250, 252);
        let r = egui::CornerRadius::same(7);
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.inactive.corner_radius = r;
        v.widgets.hovered.corner_radius = r;
        v.widgets.active.corner_radius = r;
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(241, 245, 249);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(226, 232, 240);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
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

fn size_row(ui: &mut egui::Ui, label: &str, v: &mut f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(0.1).range(5.0..=40.0));
    });
}

fn pos_drag(ui: &mut egui::Ui, p: &mut [f32; 2]) {
    ui.add(egui::DragValue::new(&mut p[0]).speed(0.2).prefix("x ").suffix(" mm").fixed_decimals(1));
    ui.add(egui::DragValue::new(&mut p[1]).speed(0.2).prefix("y ").suffix(" mm").fixed_decimals(1));
}

fn setup_fonts(ctx: &egui::Context) {
    // Arial für die Vorschau, damit sie dem Druck (GDI/Arial) entspricht.
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\arial.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("arial".into(), egui::FontData::from_owned(data).into());
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "arial".into());
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.iter().next().map(|f| f.path().to_path_buf())) {
            self.load_template(&ctx, &path);
        }

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                ui.label(egui::RichText::new(&self.status).small().color(MUTED));
            });
        egui::Panel::left("controls")
            .min_size(340.0)
            .frame(egui::Frame::new().fill(SIDEBAR_BG).inner_margin(12))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(DESK_BG))
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

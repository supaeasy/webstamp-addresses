//! Installierte Schriftarten: Liste für die Auswahl, Schriftdaten für die Vorschau, Textbreiten.

use crate::stamp::PT_PER_MM;
use eframe::egui;
use fontdb::{Database, Family, Query, Weight};

pub struct SystemFonts {
    db: Database,
    /// Alphabetisch sortierte, eindeutige Familiennamen.
    pub families: Vec<String>,
}

impl SystemFonts {
    pub fn load() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        let mut families: Vec<String> = db
            .faces()
            .filter_map(|f| f.families.first().map(|(name, _)| name.clone()))
            .filter(|n| !n.starts_with('@'))
            .collect();
        families.sort_by_key(|n| n.to_lowercase());
        families.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Self { db, families }
    }

    /// Schreibweise des installierten Namens (unabhängig von Groß-/Kleinschreibung).
    pub fn canonical(&self, name: &str) -> Option<&str> {
        self.families.iter().map(String::as_str).find(|f| f.eq_ignore_ascii_case(name.trim()))
    }

    fn query(&self, family: &str, bold: bool) -> Option<fontdb::ID> {
        self.db.query(&Query {
            families: &[Family::Name(family)],
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            ..Default::default()
        })
    }

    /// Schriftdaten samt Index innerhalb einer Font-Collection.
    pub fn font_data(&self, family: &str, bold: bool) -> Option<(Vec<u8>, u32)> {
        let id = self.query(family, bold)?;
        self.db.with_face_data(id, |data, index| (data.to_vec(), index))
    }

    /// Textbreite in mm aus den Schriftmetriken (ohne Kerning). Ersatzweise grob geschätzt.
    pub fn text_width_mm(&self, family: &str, bold: bool, text: &str, size_pt: f32) -> f32 {
        let em = self
            .query(family, bold)
            .and_then(|id| {
                self.db.with_face_data(id, |data, index| {
                    let face = ttf_parser::Face::parse(data, index).ok()?;
                    let upm = face.units_per_em() as f32;
                    Some(
                        text.chars()
                            .map(|c| {
                                face.glyph_index(c)
                                    .and_then(|g| face.glyph_hor_advance(g))
                                    .map_or(0.5, |a| a as f32 / upm)
                            })
                            .sum::<f32>(),
                    )
                })?
            })
            .unwrap_or(text.chars().count() as f32 * 0.5);
        em * size_pt / PT_PER_MM
    }
}

/// egui-Schriftfamilie für Block `idx` (0 = Empfänger, 1 = Absender), normal oder fett.
pub fn family(idx: usize, bold: bool) -> egui::FontFamily {
    const NAMES: [&str; 4] = ["recipient", "recipient_bold", "sender", "sender_bold"];
    egui::FontFamily::Name(NAMES[idx * 2 + bold as usize].into())
}

/// Schriften der Oberfläche (Arial) und leere Adress-Familien. Die GUI-Schrift bleibt immer gleich.
pub fn base_fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\arial.ttf") {
        fonts.font_data.insert("gui-arial".into(), egui::FontData::from_owned(data).into());
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "gui-arial".into());
    }
    let fallback = fonts.families[&egui::FontFamily::Proportional].clone();
    for idx in 0..2 {
        for bold in [false, true] {
            fonts.families.insert(family(idx, bold), fallback.clone());
        }
    }
    fonts
}

/// Setzt die Vorschau-Schriften der beiden Adressblöcke (die GUI-Schrift bleibt unverändert).
pub fn apply_preview_fonts(ctx: &egui::Context, sf: Option<&SystemFonts>, names: [&str; 2]) {
    let mut fonts = base_fonts();
    if let Some(sf) = sf {
        for (idx, name) in names.iter().enumerate() {
            for bold in [false, true] {
                let Some((bytes, index)) = sf.font_data(name, bold) else { continue };
                let key = format!("{name}|{bold}");
                if !fonts.font_data.contains_key(&key) {
                    let mut fd = egui::FontData::from_owned(bytes);
                    fd.index = index;
                    fonts.font_data.insert(key.clone(), fd.into());
                }
                fonts.families.get_mut(&family(idx, bold)).unwrap().insert(0, key);
            }
        }
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_system_fonts() {
        let sf = SystemFonts::load();
        assert!(sf.families.len() > 20);
        assert_eq!(sf.canonical("arial"), Some("Arial"));
        assert!(sf.font_data("Arial", false).unwrap().0.len() > 10_000);
        let (reg, bold) = (sf.text_width_mm("Arial", false, "Hamburgefonts", 12.0), sf.text_width_mm("Arial", true, "Hamburgefonts", 12.0));
        println!("regular {reg:.2} mm, bold {bold:.2} mm");
        assert!(bold > reg && reg > 10.0 && reg < 40.0);
    }
}

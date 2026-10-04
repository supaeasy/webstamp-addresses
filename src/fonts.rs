//! Installierte Schriftarten: Liste für die Auswahl und Schriftdaten für die Vorschau.

use eframe::egui;
use fontdb::{Database, Family, Query};

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

    /// Schriftdaten (Regular) samt Index innerhalb einer Font-Collection.
    pub fn font_data(&self, family: &str) -> Option<(Vec<u8>, u32)> {
        let id = self.db.query(&Query { families: &[Family::Name(family)], ..Default::default() })?;
        self.db.with_face_data(id, |data, index| (data.to_vec(), index))
    }
}

/// Setzt die Schrift der Vorschau, damit sie dem Druck (GDI) entspricht.
pub fn apply_preview_font(ctx: &egui::Context, data: Option<(Vec<u8>, u32)>) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some((bytes, index)) = data {
        let mut fd = egui::FontData::from_owned(bytes);
        fd.index = index;
        fonts.font_data.insert("preview".into(), fd.into());
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "preview".into());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_system_fonts() {
        let sf = SystemFonts::load();
        println!("{} Familien, z.B. {:?}", sf.families.len(), &sf.families[..5.min(sf.families.len())]);
        assert!(sf.families.len() > 20);
        assert_eq!(sf.canonical("arial"), Some("Arial"));
        let (data, _) = sf.font_data("Arial").expect("Arial");
        assert!(data.len() > 10_000);
    }
}

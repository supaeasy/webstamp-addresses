//! Installierte Schriftarten: erlaubte Schriften für die Auswahl, Schriftdaten für Vorschau und PDF,
//! Schriftmetriken für den vorgeschriebenen Zeilenabstand.

use eframe::egui;
use fontdb::{Database, Family, Query, Weight};

/// Grotesk-Schriften, wie sie die Post für Adressen wünscht („Grotesk wie Frutiger, Arial,
/// Helvetica, Univers usw.“), dazu metrikgleiche Verwandte und Verdana (laut Post für den
/// PP-Vermerk erlaubt). Es zählt der Anfang des Familiennamens.
const ALLOWED_BASES: &[&str] = &[
    "arial",
    "arial nova",
    "helvetica",
    "helvetica neue",
    "frutiger",
    "univers",
    "akzidenz-grotesk",
    "neue haas grotesk",
    "nimbus sans",
    "liberation sans",
    "arimo",
    "verdana",
];

/// Varianten, die verzerrt, zusammengestaucht oder Zierschriften sind, bleiben ausgeschlossen.
const EXCLUDED_WORDS: &[&str] = &["narrow", "condensed", "cond", "compressed", "black", "rounded", "mono", "extra", "ultra", "outline"];

pub fn is_allowed_family(name: &str) -> bool {
    let n = name.to_lowercase();
    if EXCLUDED_WORDS.iter().any(|w| n.split(|c: char| !c.is_alphanumeric()).any(|p| p == *w)) {
        return false;
    }
    ALLOWED_BASES.iter().any(|b| n == *b || (n.starts_with(b) && n[b.len()..].starts_with(' ')))
}

pub struct SystemFonts {
    db: Database,
    /// Davon die für Adressen erlaubten Grotesk-Schriften.
    pub allowed: Vec<String>,
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
        let allowed = families.iter().filter(|n| is_allowed_family(n)).cloned().collect();
        Self { db, allowed }
    }

    /// Schreibweise des installierten, erlaubten Namens (unabhängig von Groß-/Kleinschreibung).
    pub fn canonical_allowed(&self, name: &str) -> Option<&str> {
        self.allowed.iter().map(String::as_str).find(|f| f.eq_ignore_ascii_case(name.trim()))
    }

    fn query(&self, family: &str) -> Option<fontdb::ID> {
        self.db.query(&Query { families: &[Family::Name(family)], weight: Weight::NORMAL, ..Default::default() })
    }

    /// Schriftdaten (normaler Schnitt) samt Index innerhalb einer Font-Collection.
    pub fn font_data(&self, family: &str) -> Option<(Vec<u8>, u32)> {
        let id = self.query(family)?;
        self.db.with_face_data(id, |data, index| (data.to_vec(), index))
    }

    /// (Höhe der Oberlängen, Tiefe der Unterlängen) in Schriftgrößen-Einheiten (em), gemessen an
    /// den Glyphen: Oberlängen von b d f h k l (und H, Ziffern), Unterlängen von g j p q y.
    /// Daraus ergibt sich der Zeilenabstand zwischen Unter- und Oberlängen.
    pub fn line_metrics(&self, family: &str) -> Option<(f32, f32)> {
        let id = self.query(family)?;
        self.db.with_face_data(id, |data, index| {
            let face = ttf_parser::Face::parse(data, index).ok()?;
            let upm = face.units_per_em() as f32;
            let extent = |chars: &str, f: &dyn Fn(ttf_parser::Rect) -> f32| {
                chars
                    .chars()
                    .filter_map(|c| face.glyph_index(c).and_then(|g| face.glyph_bounding_box(g)))
                    .map(f)
                    .fold(0.0f32, f32::max)
            };
            let up = extent("bdfhklH0123456789", &|r| r.y_max as f32) / upm;
            let down = extent("gjpqy", &|r| -(r.y_min as f32)) / upm;
            (up > 0.0 && down > 0.0).then_some((up, down))
        })?
    }
}

/// Ersatzwerte (Arial) für Oberlängen und Unterlängen, falls die Schrift nicht gemessen werden kann.
pub const FALLBACK_METRICS: (f32, f32) = (0.716, 0.210);

/// egui-Schriftfamilie für Block `idx` (0 = Empfänger, 1 = Absender).
pub fn family(idx: usize) -> egui::FontFamily {
    const NAMES: [&str; 2] = ["recipient", "sender"];
    egui::FontFamily::Name(NAMES[idx].into())
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
        fonts.families.insert(family(idx), fallback.clone());
    }
    fonts
}

/// Setzt die Vorschau-Schriften der beiden Adressblöcke (die GUI-Schrift bleibt unverändert).
pub fn apply_preview_fonts(ctx: &egui::Context, sf: Option<&SystemFonts>, names: [&str; 2]) {
    let mut fonts = base_fonts();
    if let Some(sf) = sf {
        for (idx, name) in names.iter().enumerate() {
            let Some((bytes, index)) = sf.font_data(name) else { continue };
            let key = name.to_string();
            if !fonts.font_data.contains_key(&key) {
                let mut fd = egui::FontData::from_owned(bytes);
                fd.index = index;
                fonts.font_data.insert(key.clone(), fd.into());
            }
            fonts.families.get_mut(&family(idx)).unwrap().insert(0, key);
        }
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_families() {
        for ok in ["Arial", "Arial Nova", "Helvetica", "Helvetica Neue", "Frutiger LT Std", "Univers LT Std", "Liberation Sans", "Verdana"] {
            assert!(is_allowed_family(ok), "{ok}");
        }
        for no in ["Times New Roman", "Arial Black", "Arial Narrow", "Arial Rounded MT Bold", "Consolas", "Comic Sans MS", "Segoe Script", "Helvetica Neue Condensed"] {
            assert!(!is_allowed_family(no), "{no}");
        }
    }

    #[test]
    fn measures_ascender_and_descender() {
        let sf = SystemFonts::load();
        let Some(fam) = sf.allowed.first().cloned() else { return };
        assert!(sf.font_data(&fam).unwrap().0.len() > 10_000);
        let (up, down) = sf.line_metrics(&fam).unwrap();
        println!("{fam}: Oberlänge {up:.3} em, Unterlänge {down:.3} em");
        assert!((0.55..0.95).contains(&up) && (0.1..0.4).contains(&down));
    }
}

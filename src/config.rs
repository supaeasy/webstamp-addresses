use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Vorgaben der Schweizerischen Post für Adressen („Korrekte Adressierung“, Seite 31):
/// Schriftgröße mindestens 3 mm (ca. 9 pt), höchstens 9,8 mm (ca. 28 pt), ideal 10 pt;
/// Zeilenabstand zwischen den Unterlängen der oberen und den Oberlängen der unteren Zeile
/// mindestens 1 mm, höchstens 1,5 mm; drei bis sechs Zeilen.
pub const MIN_SIZE_PT: f32 = 9.0;
pub const MAX_SIZE_PT: f32 = 28.0;
pub const IDEAL_SIZE_PT: f32 = 10.0;
pub const MIN_GAP_MM: f32 = 1.0;
pub const MAX_GAP_MM: f32 = 1.5;
/// Standardschrift; ist sie nicht installiert, gilt Arial (siehe `SystemFonts::default_font`).
pub const DEFAULT_FONT: &str = "Helvetica";
pub const MIN_LINES: usize = 3;
pub const MAX_LINES: usize = 6;

/// Einstellungen eines Adressblocks (Position links oben in mm).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct BlockCfg {
    /// Oberkante der Oberlängen der ersten Zeile.
    pub pos: [f32; 2],
    pub size_pt: f32,
    /// Abstand zwischen Unterlängen der oberen und Oberlängen der unteren Zeile in mm.
    pub line_gap_mm: f32,
    /// Schriftart (muss installiert und eine erlaubte Grotesk-Schrift sein).
    pub font: String,
}

impl BlockCfg {
    pub fn recipient() -> Self {
        Self { pos: [155.0, 108.0], size_pt: IDEAL_SIZE_PT, line_gap_mm: 1.25, font: DEFAULT_FONT.into() }
    }

    pub fn sender() -> Self {
        Self { pos: [12.0, 25.2], size_pt: MIN_SIZE_PT, line_gap_mm: 1.25, font: DEFAULT_FONT.into() }
    }

    /// Bringt Werte aus einer älteren oder von Hand bearbeiteten Konfiguration in den erlaubten Bereich.
    pub fn clamp(&mut self) {
        self.size_pt = self.size_pt.clamp(MIN_SIZE_PT, MAX_SIZE_PT).round();
        self.line_gap_mm = self.line_gap_mm.clamp(MIN_GAP_MM, MAX_GAP_MM);
    }
}

impl Default for BlockCfg {
    fn default() -> Self {
        Self::recipient()
    }
}

/// Art der Sendung: bestimmt die Zonen der Post in der Vorschau.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Destination {
    #[default]
    Domestic,
    Foreign,
}

/// Einsetzbares Bild (z. B. Logo); Position links oben und Breite in mm.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct ImageCfg {
    pub path: Option<PathBuf>,
    pub show: bool,
    pub pos: [f32; 2],
    pub width_mm: f32,
}

impl Default for ImageCfg {
    fn default() -> Self {
        Self { path: None, show: true, pos: [12.0, 45.0], width_mm: 40.0 }
    }
}

/// Eine Vorlage („Slot“): alles, was zu einem Umschlag gehört – außer dem verwendeten Webstamp.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Slot {
    pub name: String,
    pub recipient_text: String,
    pub sender_text: String,
    pub print_sender: bool,
    pub recipient: BlockCfg,
    pub sender_block: BlockCfg,
    pub image: ImageCfg,
    pub destination: Destination,
    /// Webstamp mitdrucken (aus: Marke wird ausgeblendet und nicht gedruckt).
    pub print_stamp: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self::named("Vorlage 1")
    }
}

impl Slot {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            recipient_text: String::new(),
            sender_text: String::new(),
            print_sender: true,
            recipient: BlockCfg::recipient(),
            sender_block: BlockCfg::sender(),
            image: ImageCfg::default(),
            destination: Destination::Domestic,
            print_stamp: true,
        }
    }
}

/// Persistente Einstellungen. Positionen in mm, gemessen von links oben auf dem Umschlag.
/// Vorlagen-spezifisches steht in `slots`; hier nur Drucker- und Anzeigeeinstellungen.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub slots: Vec<Slot>,
    pub active: usize,
    /// Umschlag beim Druck um 180° drehen (je nach Einzug des Druckers).
    pub flip_180: bool,
    /// Feinjustierung des Druckers in mm.
    pub print_offset: [f32; 2],
    /// Zuletzt geladener Webstamp (gehört bewusst zu keiner Vorlage).
    pub last_template: Option<PathBuf>,
    /// Hilfslinien (nur Vorschau, werden nicht gedruckt), Position in mm.
    pub show_guides: bool,
    pub guide_h: f32,
    pub guide_v: f32,
    /// Zonen der Schweizer Post (nur Vorschau).
    pub show_zones: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            slots: vec![Slot::default()],
            active: 0,
            flip_180: false,
            print_offset: [0.0, 0.0],
            last_template: None,
            show_guides: false,
            guide_h: 80.0,
            guide_v: 110.0,
            show_zones: true,
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("webstamp-addresses").join("config.json"))
}

/// Einstellungen aus der früheren Version (Projektname „envelope-printer“).
fn legacy_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("envelope-printer").join("config.json"))
}

impl Config {
    pub fn slot(&self) -> &Slot {
        &self.slots[self.active]
    }

    pub fn slot_mut(&mut self) -> &mut Slot {
        &mut self.slots[self.active]
    }

    /// Stellt sicher, dass es mindestens einen Slot gibt, der aktive Index gültig ist und alle Werte
    /// im erlaubten Bereich liegen.
    fn normalize(&mut self) {
        if self.slots.is_empty() {
            self.slots.push(Slot::default());
        }
        self.active = self.active.min(self.slots.len() - 1);
        for s in &mut self.slots {
            s.recipient.clamp();
            s.sender_block.clamp();
        }
    }

    /// Liest eine Konfiguration; Einstellungen aus Version ≤ 0.3 (ohne Slots) werden als erster Slot übernommen.
    pub fn from_json(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let has_slots = value.get("slots").is_some();
        let mut c: Config = serde_json::from_value(value.clone()).ok()?;
        if !has_slots {
            let mut slot = Slot::default();
            let take = |key: &str| value.get(key).cloned();
            if let Some(v) = take("recipient").and_then(|v| serde_json::from_value(v).ok()) {
                slot.recipient = v;
            }
            if let Some(v) = take("sender_block").and_then(|v| serde_json::from_value(v).ok()) {
                slot.sender_block = v;
            }
            if let Some(v) = take("image").and_then(|v| serde_json::from_value(v).ok()) {
                slot.image = v;
            }
            if let Some(v) = take("destination").and_then(|v| serde_json::from_value(v).ok()) {
                slot.destination = v;
            }
            if let Some(v) = take("print_sender").and_then(|v| v.as_bool()) {
                slot.print_sender = v;
            }
            if let Some(v) = take("sender").and_then(|v| v.as_str().map(str::to_owned)) {
                slot.sender_text = v;
            }
            c.slots = vec![slot];
            c.active = 0;
        }
        c.normalize();
        Some(c)
    }

    pub fn load() -> Self {
        path()
            .filter(|p| p.exists())
            .or_else(legacy_path)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| Self::from_json(&s))
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = path() else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, serde_json::to_string_pretty(self).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_positions_and_font() {
        let c = Config::default();
        let s = c.slot();
        assert_eq!(s.recipient.pos, [155.0, 108.0]);
        assert_eq!(s.sender_block.pos, [12.0, 25.2]);
        assert_eq!((s.recipient.font.as_str(), s.sender_block.font.as_str()), ("Helvetica", "Helvetica"));
        // „Positionen zurücksetzen“ nutzt dieselben Werte
        assert_eq!(BlockCfg::recipient().pos, s.recipient.pos);
        assert_eq!(BlockCfg::sender().pos, s.sender_block.pos);
        assert!(s.print_stamp);
    }

    #[test]
    fn old_values_are_clamped() {
        let mut b = BlockCfg { size_pt: 5.0, line_gap_mm: 3.0, ..BlockCfg::sender() };
        b.clamp();
        assert_eq!((b.size_pt, b.line_gap_mm), (MIN_SIZE_PT, MAX_GAP_MM));
    }

    #[test]
    fn slots_roundtrip_and_stay_valid() {
        let mut c = Config::default();
        c.slot_mut().recipient_text = "Max\nMusterweg 1\n8000 Zürich".into();
        c.slots.push(Slot::named("Zweite"));
        c.active = 1;
        c.slot_mut().print_stamp = false;
        let back = Config::from_json(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        // kaputter Index und leere Liste werden repariert
        let fixed = Config::from_json(r#"{"slots": [], "active": 7}"#).unwrap();
        assert_eq!((fixed.slots.len(), fixed.active), (1, 0));
    }

    #[test]
    fn migrates_old_single_config_into_first_slot() {
        let old = r#"{
            "sender": "Absender AG\nWeg 1\n8000 Zürich",
            "print_sender": false,
            "recipient": {"pos": [100.0, 90.0], "size_pt": 11.0, "line_gap_mm": 1.3, "font": "Arial"},
            "sender_block": {"pos": [10.0, 20.0], "size_pt": 9.0, "line_gap_mm": 1.25, "font": "Arial"},
            "image": {"path": "C:/logo.png", "show": true, "pos": [5.0, 6.0], "width_mm": 33.0},
            "destination": "Foreign",
            "flip_180": true,
            "last_template": "C:/stamp.pdf"
        }"#;
        let c = Config::from_json(old).unwrap();
        assert_eq!(c.slots.len(), 1);
        let s = c.slot();
        assert_eq!(s.sender_text, "Absender AG\nWeg 1\n8000 Zürich");
        assert!(!s.print_sender);
        assert_eq!(s.recipient.pos, [100.0, 90.0]);
        assert_eq!(s.image.width_mm, 33.0);
        assert_eq!(s.destination, Destination::Foreign);
        assert!(c.flip_180); // Druckeinstellung bleibt global
        assert_eq!(c.last_template.as_deref(), Some(std::path::Path::new("C:/stamp.pdf")));
    }
}

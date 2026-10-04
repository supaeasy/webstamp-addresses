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
        Self { pos: [105.0, 85.0], size_pt: IDEAL_SIZE_PT, line_gap_mm: 1.25, font: "Arial".into() }
    }

    pub fn sender() -> Self {
        Self { pos: [12.0, 12.0], size_pt: MIN_SIZE_PT, line_gap_mm: 1.25, font: "Arial".into() }
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

/// Persistente Einstellungen (Positionen in mm, gemessen von links oben auf dem Umschlag).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    /// Standard-Absender (mehrzeilig).
    pub sender: String,
    pub print_sender: bool,
    pub recipient: BlockCfg,
    pub sender_block: BlockCfg,
    pub image: ImageCfg,
    /// Umschlag beim Druck um 180° drehen (je nach Einzug des Druckers).
    pub flip_180: bool,
    /// Feinjustierung des Druckers in mm.
    pub print_offset: [f32; 2],
    pub last_template: Option<PathBuf>,
    /// Hilfslinien (nur Vorschau, werden nicht gedruckt), Position in mm.
    pub show_guides: bool,
    pub guide_h: f32,
    pub guide_v: f32,
    /// Zonen der Schweizer Post (nur Vorschau).
    pub show_zones: bool,
    pub destination: Destination,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sender: String::new(),
            print_sender: true,
            recipient: BlockCfg::recipient(),
            sender_block: BlockCfg::sender(),
            image: ImageCfg::default(),
            flip_180: false,
            print_offset: [0.0, 0.0],
            last_template: None,
            show_guides: false,
            guide_h: 80.0,
            guide_v: 110.0,
            show_zones: true,
            destination: Destination::Domestic,
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
    pub fn load() -> Self {
        let mut c: Config = path()
            .filter(|p| p.exists())
            .or_else(legacy_path)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        c.recipient.clamp();
        c.sender_block.clamp();
        c
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = path() else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, serde_json::to_string_pretty(self).unwrap())
    }
}

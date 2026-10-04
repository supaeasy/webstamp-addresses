use crate::stamp::Align;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Einstellungen eines Adressblocks (Position links oben in mm).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct BlockCfg {
    pub pos: [f32; 2],
    /// Breite des Textfeldes in mm (Bezug für rechtsbündig und Blocksatz).
    pub width_mm: f32,
    pub size_pt: f32,
    /// Schriftart (muss unter Windows installiert sein).
    pub font: String,
    pub align: Align,
}

impl BlockCfg {
    pub fn recipient() -> Self {
        Self { pos: [105.0, 85.0], width_mm: 90.0, size_pt: 12.0, font: "Arial".into(), align: Align::Left }
    }

    pub fn sender() -> Self {
        Self { pos: [12.0, 12.0], width_mm: 70.0, size_pt: 9.0, font: "Arial".into(), align: Align::Left }
    }
}

impl Default for BlockCfg {
    fn default() -> Self {
        Self::recipient()
    }
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
    /// Standard-Absender (mehrzeilig, `**fett**` möglich).
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
        path()
            .filter(|p| p.exists())
            .or_else(legacy_path)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
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

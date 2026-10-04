use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::{Document, Object, Stream, dictionary};
use std::path::Path;

pub const PT_PER_MM: f32 = 72.0 / 25.4;

/// Ein Adressblock; Position (links oben) in mm, Schriftgröße in pt.
#[derive(Clone, Debug)]
pub struct Block {
    pub text: String,
    pub pos: [f32; 2],
    pub size_pt: f32,
}

impl Block {
    pub fn pitch_pt(&self) -> f32 {
        self.size_pt * 1.2
    }

    /// Grundlinie der Zeile `i` in mm.
    pub fn baseline_mm(&self, i: usize) -> f32 {
        self.pos[1] + (i as f32 * self.pitch_pt() + self.size_pt * 0.905) / PT_PER_MM
    }
}

pub struct Template {
    pub bytes: Vec<u8>,
    pub width_mm: f32,
    pub height_mm: f32,
}

pub struct Raster {
    pub w: usize,
    pub h: usize,
    /// Deckend (weißer Hintergrund), RGBA.
    pub rgba: Vec<u8>,
}

pub fn load(path: &Path) -> Result<Template, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pdf = Pdf::new(bytes.clone()).map_err(|e| format!("PDF nicht lesbar: {e:?}"))?;
    let pages = pdf.pages();
    let page = pages.first().ok_or("PDF enthält keine Seite")?;
    let (w, h) = page.render_dimensions();
    Ok(Template {
        bytes,
        width_mm: w / PT_PER_MM,
        height_mm: h / PT_PER_MM,
    })
}

pub fn render_template(t: &Template, dpi: f32) -> Result<Raster, String> {
    let pdf = Pdf::new(t.bytes.clone()).map_err(|e| format!("PDF nicht lesbar: {e:?}"))?;
    let pages = pdf.pages();
    let page = pages.first().ok_or("PDF enthält keine Seite")?;
    let s = dpi / 72.0;
    let pix = render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: s,
            y_scale: s,
            bg_color: WHITE,
        },
    );
    Ok(Raster {
        w: pix.width() as usize,
        h: pix.height() as usize,
        rgba: pix.data_as_u8_slice().to_vec(),
    })
}

/// Schreibt Vorlage + Adressen als PDF (Helvetica, entspricht Arial-Metrik).
pub fn export_pdf(t: &Template, blocks: &[Block], out: &Path) -> Result<(), String> {
    let mut doc = Document::load_mem(&t.bytes).map_err(|e| e.to_string())?;
    let page_id = *doc.get_pages().values().next().ok_or("keine Seite")?;
    let page_h_pt = t.height_mm * PT_PER_MM;

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });

    // Resources ggf. aus Referenz/Vererbung in die Seite holen und Font ergänzen.
    let mut res = match doc.get_page_resources(page_id) {
        Ok((Some(d), _)) => d.clone(),
        Ok((None, ids)) => ids
            .iter()
            .find_map(|id| doc.get_dictionary(*id).ok().cloned())
            .unwrap_or_default(),
        Err(_) => Default::default(),
    };
    let mut fonts = match res.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Default::default(),
    };
    fonts.set("FEnv", Object::Reference(font_id));
    res.set("Font", Object::Dictionary(fonts));
    doc.get_dictionary_mut(page_id)
        .map_err(|e| e.to_string())?
        .set("Resources", Object::Dictionary(res));

    let mut content = String::from("q\n0 g\n");
    for b in blocks {
        for (i, line) in b.text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            content.push_str(&format!(
                "BT /FEnv {} Tf {:.2} {:.2} Td ",
                b.size_pt,
                b.pos[0] * PT_PER_MM,
                page_h_pt - b.baseline_mm(i) * PT_PER_MM
            ));
            content.push_str(&pdf_string(line));
            content.push_str(" Tj ET\n");
        }
    }
    content.push_str("Q\n");

    let id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
    let mut contents = match page.get(b"Contents") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(o @ Object::Reference(_)) => vec![o.clone()],
        _ => vec![],
    };
    contents.push(Object::Reference(id));
    page.set("Contents", Object::Array(contents));

    doc.save(out).map_err(|e| e.to_string())?;
    Ok(())
}

fn pdf_string(s: &str) -> String {
    // Latin-1 / WinAnsi; alles andere wird als '?' ausgegeben.
    let mut out = String::from("<");
    for c in s.chars() {
        let b: u8 = match c {
            '€' => 0x80,
            c if (c as u32) < 256 => c as u32 as u8,
            _ => b'?',
        };
        out.push_str(&format!("{b:02X}"));
    }
    out.push('>');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r"sample-stamp.pdf";

    #[test]
    fn render_and_export() {
        let t = load(Path::new(SAMPLE)).unwrap();
        assert!((t.width_mm - 229.0).abs() < 1.0 && (t.height_mm - 162.0).abs() < 1.0);
        let r = render_template(&t, 150.0).unwrap();
        let dark = r.rgba.chunks(4).filter(|p| p[0] < 128).count();
        assert!(dark > 1000, "Stempel wurde nicht gerendert ({dark})");
        let blocks = vec![Block { text: "Ärger GmbH\nMüllerstraße 5\n12345 Köln".into(), pos: [105.0, 85.0], size_pt: 12.0 }];
        let out = std::env::temp_dir().join("env_test.pdf");
        export_pdf(&t, &blocks, &out).unwrap();
        println!("{}", out.display());
    }
}

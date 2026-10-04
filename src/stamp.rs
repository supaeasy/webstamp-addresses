use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::{Document, Object, Stream, dictionary};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
    /// Blocksatz: alle Zeilen außer der letzten werden auf die Feldbreite gestreckt.
    Justify,
}

/// Ein Adressblock; Position (links oben) und Breite in mm, Schriftgröße in pt.
/// Im Text markiert `**…**` fett gedruckte Teile (innerhalb einer Zeile).
#[derive(Clone, Debug)]
pub struct Block {
    pub text: String,
    pub pos: [f32; 2],
    pub width_mm: f32,
    pub size_pt: f32,
    pub align: Align,
    pub font: String,
}

/// Ein zu zeichnender Textabschnitt (absolute Position in mm, Grundlinie).
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub x_mm: f32,
    pub baseline_mm: f32,
    pub text: String,
    pub bold: bool,
}

/// Ein platziertes Bild (links oben `pos`, Breite in mm; die Höhe ergibt sich aus dem Seitenverhältnis).
pub struct Placed<'a> {
    pub img: &'a image::RgbaImage,
    pub pos: [f32; 2],
    pub width_mm: f32,
}

impl Placed<'_> {
    pub fn height_mm(&self) -> f32 {
        self.width_mm * self.img.height() as f32 / self.img.width() as f32
    }
}

impl Block {
    pub fn line_height_mm(&self) -> f32 {
        self.size_pt * 1.2 / PT_PER_MM
    }

    /// Grundlinie der Zeile `i` in mm.
    pub fn baseline_mm(&self, i: usize) -> f32 {
        self.pos[1] + (i as f32 * self.size_pt * 1.2 + self.size_pt * 0.905) / PT_PER_MM
    }
}

/// Teilt eine Zeile an `**` in (Text, fett)-Abschnitte.
pub fn spans(line: &str) -> Vec<(String, bool)> {
    let mut out = vec![];
    let mut bold = false;
    for part in line.split("**") {
        if !part.is_empty() {
            out.push((part.to_string(), bold));
        }
        bold = !bold;
    }
    out
}

/// Berechnet die Positionen aller Textabschnitte. `measure(text, fett)` liefert die Breite in mm.
pub fn layout(b: &Block, measure: &mut dyn FnMut(&str, bool) -> f32) -> Vec<Run> {
    let lines: Vec<&str> = b.text.lines().collect();
    let is_blank = |l: &str| spans(l).iter().all(|(t, _)| t.trim().is_empty());
    let last = lines.iter().rposition(|l| !is_blank(l));
    let right = b.pos[0] + b.width_mm;
    let mut runs = vec![];

    for (i, line) in lines.iter().enumerate() {
        if is_blank(line) {
            continue;
        }
        let y = b.baseline_mm(i);
        let mut sp = spans(line);

        if b.align == Align::Justify && Some(i) != last {
            let words: Vec<(String, bool)> = sp
                .iter()
                .flat_map(|(t, bold)| t.split_whitespace().map(move |w| (w.to_string(), *bold)))
                .collect();
            if words.len() >= 2 {
                let widths: Vec<f32> = words.iter().map(|(w, bold)| measure(w, *bold)).collect();
                let sum: f32 = widths.iter().sum();
                let space = measure(" ", false);
                let gap = ((b.width_mm - sum) / (words.len() - 1) as f32).max(space);
                let mut x = b.pos[0];
                for ((text, bold), w) in words.into_iter().zip(widths) {
                    runs.push(Run { x_mm: x, baseline_mm: y, text, bold });
                    x += w + gap;
                }
                continue;
            }
        }

        if b.align == Align::Right {
            if let Some(l) = sp.last_mut() {
                l.0 = l.0.trim_end().to_string();
            }
            sp.retain(|(t, _)| !t.is_empty());
        }
        let widths: Vec<f32> = sp.iter().map(|(t, bold)| measure(t, *bold)).collect();
        let mut x = if b.align == Align::Right { right - widths.iter().sum::<f32>() } else { b.pos[0] };
        for ((text, bold), w) in sp.into_iter().zip(widths) {
            runs.push(Run { x_mm: x, baseline_mm: y, text, bold });
            x += w;
        }
    }
    runs
}

fn char_to_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(i, _)| i).unwrap_or(s.len())
}

/// Schaltet Fettdruck (`**`) für die Auswahl (Zeichenindizes) um und liefert die neue Auswahl.
pub fn toggle_bold(text: &mut String, sel: (usize, usize)) -> (usize, usize) {
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    if a == b {
        return sel;
    }
    let (ba, bb) = (char_to_byte(text, a), char_to_byte(text, b));

    // Auswahl liegt direkt innerhalb von **…** → Sterne entfernen.
    if text[..ba].ends_with("**") && text[bb..].starts_with("**") {
        text.replace_range(bb..bb + 2, "");
        text.replace_range(ba - 2..ba, "");
        return (a - 2, b - 2);
    }

    let selected = text[ba..bb].to_string();
    let pieces: Vec<&str> = selected.split('\n').collect();
    let wrapped = |p: &str| {
        let t = p.trim();
        t.len() >= 4 && t.starts_with("**") && t.ends_with("**")
    };
    let content: Vec<&&str> = pieces.iter().filter(|p| !p.trim().is_empty()).collect();
    if content.is_empty() {
        return sel;
    }
    let unwrap = content.iter().all(|p| wrapped(p));

    let mut out = String::new();
    for (k, p) in pieces.iter().enumerate() {
        if k > 0 {
            out.push('\n');
        }
        if p.trim().is_empty() {
            out.push_str(p);
            continue;
        }
        let core = p.trim();
        out.push_str(&p[..p.len() - p.trim_start().len()]);
        if unwrap {
            out.push_str(&core[2..core.len() - 2]);
        } else if wrapped(p) {
            out.push_str(core);
        } else {
            out.push_str(&format!("**{core}**"));
        }
        out.push_str(&p[p.trim_end().len()..]);
    }
    text.replace_range(ba..bb, &out);
    (a, a + out.chars().count())
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

/// Schreibt Vorlage + Adressen als PDF. Verwendet Helvetica / Helvetica-Bold (nicht die gewählte
/// Schrift); `measure(block, text, fett)` liefert die Textbreite in mm.
pub fn export_pdf(
    t: &Template,
    blocks: &[Block],
    images: &[Placed],
    out: &Path,
    measure: &mut dyn FnMut(&Block, &str, bool) -> f32,
) -> Result<(), String> {
    let mut doc = Document::load_mem(&t.bytes).map_err(|e| e.to_string())?;
    let page_id = *doc.get_pages().values().next().ok_or("keine Seite")?;
    let page_h_pt = t.height_mm * PT_PER_MM;

    let font_regular = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let font_bold = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold", "Encoding" => "WinAnsiEncoding",
    });

    // Resources ggf. aus Referenz/Vererbung in die Seite holen und Fonts ergänzen.
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
    fonts.set("FEnv", Object::Reference(font_regular));
    fonts.set("FEnvB", Object::Reference(font_bold));
    res.set("Font", Object::Dictionary(fonts));

    // Bilder als RGB-XObjects (Transparenz auf Weiß gerechnet, auf max. 2400 px begrenzt).
    let mut xobjects = match res.get(b"XObject") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Default::default(),
    };
    let mut content = String::from("q\n0 g\n");
    for (i, p) in images.iter().enumerate() {
        let (w, h) = (p.img.width(), p.img.height());
        let big = w.max(h);
        let small;
        let src = if big > 2400 {
            let f = 2400.0 / big as f32;
            small = image::imageops::resize(
                p.img,
                ((w as f32 * f) as u32).max(1),
                ((h as f32 * f) as u32).max(1),
                image::imageops::FilterType::Triangle,
            );
            &small
        } else {
            p.img
        };
        let mut rgb = Vec::with_capacity(src.width() as usize * src.height() as usize * 3);
        for px in src.pixels() {
            let a = px[3] as u32;
            rgb.extend([px[0], px[1], px[2]].map(|v| ((v as u32 * a + 255 * (255 - a)) / 255) as u8));
        }
        let mut stream = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image",
                "Width" => src.width() as i64, "Height" => src.height() as i64,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
            },
            rgb,
        );
        let _ = stream.compress();
        let name = format!("ImEnv{i}");
        xobjects.set(name.as_bytes().to_vec(), Object::Reference(doc.add_object(stream)));
        content.push_str(&format!(
            "q {:.2} 0 0 {:.2} {:.2} {:.2} cm /{name} Do Q\n",
            p.width_mm * PT_PER_MM,
            p.height_mm() * PT_PER_MM,
            p.pos[0] * PT_PER_MM,
            page_h_pt - (p.pos[1] + p.height_mm()) * PT_PER_MM,
        ));
    }
    res.set("XObject", Object::Dictionary(xobjects));
    doc.get_dictionary_mut(page_id)
        .map_err(|e| e.to_string())?
        .set("Resources", Object::Dictionary(res));

    for b in blocks {
        for r in layout(b, &mut |text, bold| measure(b, text, bold)) {
            content.push_str(&format!(
                "BT /{} {} Tf {:.2} {:.2} Td {} Tj ET\n",
                if r.bold { "FEnvB" } else { "FEnv" },
                b.size_pt,
                r.x_mm * PT_PER_MM,
                page_h_pt - r.baseline_mm * PT_PER_MM,
                pdf_string(&r.text)
            ));
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

    fn block(text: &str, align: Align) -> Block {
        Block { text: text.into(), pos: [10.0, 20.0], width_mm: 100.0, size_pt: 12.0, align, font: "Arial".into() }
    }

    /// Jedes Zeichen 2 mm breit, fett 3 mm.
    fn m(s: &str, bold: bool) -> f32 {
        s.chars().count() as f32 * if bold { 3.0 } else { 2.0 }
    }

    #[test]
    fn bold_spans() {
        assert_eq!(spans("a **b** c"), vec![("a ".into(), false), ("b".into(), true), (" c".into(), false)]);
        assert_eq!(spans("**ab"), vec![("ab".into(), true)]);
    }

    #[test]
    fn layout_left_right_justify() {
        let r = layout(&block("ab **cd**", Align::Left), &mut m);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].x_mm, r[1].x_mm, r[1].bold), (10.0, 10.0 + 6.0, true));

        let r = layout(&block("abc  ", Align::Right), &mut m);
        assert_eq!(r[0].x_mm, 110.0 - 6.0);

        // Zeile 1 gestreckt (endet am rechten Rand), letzte Zeile linksbündig
        let r = layout(&block("ab cd\nef gh", Align::Justify), &mut m);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].x_mm, 10.0);
        assert_eq!(r[1].x_mm + 4.0, 110.0);
        assert_eq!((r[2].x_mm, r[2].text.as_str()), (10.0, "ef gh"));
    }

    #[test]
    fn toggle_bold_wraps_and_unwraps() {
        let mut t = String::from("Max Mustermann\nStraße 1");
        let sel = toggle_bold(&mut t, (0, 14));
        assert_eq!(t, "**Max Mustermann**\nStraße 1");
        // Auswahl ohne Sterne innerhalb von **…** → entfernt sie wieder
        let sel2 = toggle_bold(&mut t, (sel.0 + 2, sel.1 - 2));
        assert_eq!(t, "Max Mustermann\nStraße 1");
        assert_eq!(sel2, (0, 14));
        // mehrere Zeilen
        let mut t = String::from("a\nb");
        toggle_bold(&mut t, (0, 3));
        assert_eq!(t, "**a**\n**b**");
    }

    #[test]
    fn render_and_export() {
        let t = load(Path::new(SAMPLE)).unwrap();
        assert!((t.width_mm - 229.0).abs() < 1.0 && (t.height_mm - 162.0).abs() < 1.0);
        let r = render_template(&t, 150.0).unwrap();
        let dark = r.rgba.chunks(4).filter(|p| p[0] < 128).count();
        assert!(dark > 1000, "Stempel wurde nicht gerendert ({dark})");
        let blocks = vec![block("Ärger **GmbH**\nMüllerstraße 5\n12345 Köln", Align::Right)];
        let out = std::env::temp_dir().join("env_test.pdf");
        let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255]));
        let placed = [Placed { img: &img, pos: [12.0, 100.0], width_mm: 40.0 }];
        assert_eq!(placed[0].height_mm(), 20.0);
        export_pdf(&t, &blocks, &placed, &out, &mut |_, s, bold| m(s, bold)).unwrap();
        println!("{}", out.display());
    }
}

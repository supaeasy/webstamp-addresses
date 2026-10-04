use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::{Document, Object, Stream, dictionary};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    #[cfg_attr(not(windows), allow(dead_code))] // nur der Windows-Druck wählt die Schrift selbst
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
    pub img: &'a crate::graphic::Graphic,
    pub pos: [f32; 2],
    pub width_mm: f32,
}

impl Placed<'_> {
    pub fn height_mm(&self) -> f32 {
        self.width_mm * self.img.aspect()
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

/// Liefert die Schriftdaten (Bytes, Index in der Collection) zu Familie und Fett.
pub type FaceFn<'a> = &'a dyn Fn(&str, bool) -> Option<(Vec<u8>, u32)>;

/// Schreibt Vorlage + Adressen + Bilder als PDF. Die gewählten Schriften werden als Teilschrift
/// (nur die benutzten Zeichen) eingebettet; fehlt eine Schrift, wird Helvetica verwendet.
/// `measure(block, text, fett)` liefert die Textbreite in mm. Rückgabe: Familien ohne Schriftdaten.
pub fn export_pdf(
    t: &Template,
    blocks: &[Block],
    images: &[Placed],
    out: &Path,
    measure: &mut dyn FnMut(&Block, &str, bool) -> f32,
    face: FaceFn,
) -> Result<Vec<String>, String> {
    let mut doc = Document::load_mem(&t.bytes).map_err(|e| e.to_string())?;
    let page_id = *doc.get_pages().values().next().ok_or("keine Seite")?;
    let page_h_pt = t.height_mm * PT_PER_MM;

    // 1. Layout aller Blöcke und benutzte Zeichen je (Schrift, fett)
    let layouts: Vec<Vec<Run>> = blocks
        .iter()
        .map(|b| layout(b, &mut |text, bold| measure(b, text, bold)))
        .collect();
    let mut used: BTreeMap<(String, bool), BTreeSet<char>> = BTreeMap::new();
    for (b, runs) in blocks.iter().zip(&layouts) {
        for r in runs {
            used.entry((b.font.clone(), r.bold)).or_default().extend(r.text.chars());
        }
    }

    // 2. Schriften einbetten
    let mut fonts = dictionary! {};
    let mut embedded: HashMap<(String, bool), (String, HashMap<char, u16>)> = HashMap::new();
    let mut missing: Vec<String> = vec![];
    for (n, ((family, bold), chars)) in used.iter().enumerate() {
        let done = face(family, *bold)
            .and_then(|(data, index)| embed_font(&mut doc, &data, index, chars, family));
        match done {
            Some((id, map)) => {
                let name = format!("FE{n}");
                fonts.set(name.as_bytes().to_vec(), Object::Reference(id));
                embedded.insert((family.clone(), *bold), (name, map));
            }
            None if !missing.contains(family) => missing.push(family.clone()),
            None => {}
        }
    }

    // Helvetica als Rückfall
    let helv = |doc: &mut Document, base: &str| {
        doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => base, "Encoding" => "WinAnsiEncoding",
        })
    };
    let (helv_regular, helv_bold) = (helv(&mut doc, "Helvetica"), helv(&mut doc, "Helvetica-Bold"));
    fonts.set("FEnv", Object::Reference(helv_regular));
    fonts.set("FEnvB", Object::Reference(helv_bold));

    // 3. Resources der Seite ergänzen (Fonts, Bilder)
    let mut res = match doc.get_page_resources(page_id) {
        Ok((Some(d), _)) => d.clone(),
        Ok((None, ids)) => ids
            .iter()
            .find_map(|id| doc.get_dictionary(*id).ok().cloned())
            .unwrap_or_default(),
        Err(_) => Default::default(),
    };
    let mut page_fonts = match res.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Default::default(),
    };
    for (k, v) in fonts.iter() {
        page_fonts.set(k.clone(), v.clone());
    }
    res.set("Font", Object::Dictionary(page_fonts));

    // Bilder als RGB-XObjects (Transparenz auf Weiß gerechnet, auf max. 2400 px begrenzt).
    let mut xobjects = match res.get(b"XObject") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Default::default(),
    };
    let mut content = String::from("q\n0 g\n");
    for (i, p) in images.iter().enumerate() {
        let src = p.img.for_pdf(p.width_mm, p.height_mm());
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

    // 4. Text
    for (b, runs) in blocks.iter().zip(&layouts) {
        for r in runs {
            let (name, shown) = match embedded.get(&(b.font.clone(), r.bold)) {
                Some((name, map)) => {
                    let hex: String = r
                        .text
                        .chars()
                        .map(|c| format!("{:04X}", map.get(&c).copied().unwrap_or(0)))
                        .collect();
                    (name.as_str(), format!("<{hex}>"))
                }
                None => (if r.bold { "FEnvB" } else { "FEnv" }, pdf_string(&r.text)),
            };
            content.push_str(&format!(
                "BT /{name} {} Tf {:.2} {:.2} Td {shown} Tj ET\n",
                b.size_pt,
                r.x_mm * PT_PER_MM,
                page_h_pt - r.baseline_mm * PT_PER_MM,
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
    Ok(missing)
}

/// Bettet eine Teilschrift als Type0/Identity-H-Font ein. Liefert das Font-Objekt und die
/// Zuordnung Zeichen → neue Glyph-ID (= CID).
fn embed_font(
    doc: &mut Document,
    data: &[u8],
    index: u32,
    chars: &BTreeSet<char>,
    family: &str,
) -> Option<(lopdf::ObjectId, HashMap<char, u16>)> {
    let face = ttf_parser::Face::parse(data, index).ok()?;
    let k = 1000.0 / face.units_per_em() as f32;

    let mut remapper = subsetter::GlyphRemapper::new();
    remapper.remap(0); // .notdef
    let old: Vec<(char, u16)> = chars
        .iter()
        .map(|&c| (c, face.glyph_index(c).map_or(0, |g| g.0)))
        .collect();
    for (_, g) in &old {
        remapper.remap(*g);
    }
    let subset = subsetter::subset(data, index, &remapper).ok()?;

    let mut map = HashMap::new();
    let mut widths: BTreeMap<u16, i64> = BTreeMap::new();
    for (c, g) in &old {
        let new = remapper.get(*g)?;
        map.insert(*c, new);
        let adv = face.glyph_hor_advance(ttf_parser::GlyphId(*g)).unwrap_or(0) as f32 * k;
        widths.insert(new, adv.round() as i64);
    }
    let mut w = vec![];
    for (gid, adv) in widths {
        w.push(Object::Integer(gid as i64));
        w.push(Object::Array(vec![Object::Integer(adv)]));
    }

    let cff = face.tables().cff.is_some();
    let mut file = Stream::new(
        if cff { dictionary! { "Subtype" => "OpenType" } } else { dictionary! { "Length1" => subset.len() as i64 } },
        subset,
    );
    let _ = file.compress();
    let file_id = doc.add_object(file);

    let clean: String = family.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let base = format!("AAAAAA+{}", if clean.is_empty() { "Font".into() } else { clean });
    let bb = face.global_bounding_box();
    let scaled = |v: i16| Object::Integer((v as f32 * k).round() as i64);
    let ascent = face.ascender();
    let mut descriptor = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(base.clone().into_bytes()),
        "Flags" => 32,
        "FontBBox" => vec![scaled(bb.x_min), scaled(bb.y_min), scaled(bb.x_max), scaled(bb.y_max)],
        "ItalicAngle" => 0,
        "Ascent" => scaled(ascent),
        "Descent" => scaled(face.descender()),
        "CapHeight" => scaled(face.capital_height().unwrap_or(ascent)),
        "StemV" => 80,
    };
    descriptor.set(if cff { "FontFile3" } else { "FontFile2" }, Object::Reference(file_id));
    let descriptor_id = doc.add_object(descriptor);

    let mut cid_font = dictionary! {
        "Type" => "Font",
        "Subtype" => if cff { "CIDFontType0" } else { "CIDFontType2" },
        "BaseFont" => Object::Name(base.clone().into_bytes()),
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => Object::Reference(descriptor_id),
        "DW" => 0,
        "W" => Object::Array(w),
    };
    if !cff {
        cid_font.set("CIDToGIDMap", "Identity");
    }
    let cid_id = doc.add_object(cid_font);

    let mut to_unicode = Stream::new(dictionary! {}, to_unicode_cmap(&map).into_bytes());
    let _ = to_unicode.compress();
    let to_unicode_id = doc.add_object(to_unicode);

    let type0 = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => Object::Name(base.into_bytes()),
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid_id)],
        "ToUnicode" => Object::Reference(to_unicode_id),
    });
    Some((type0, map))
}

/// ToUnicode-CMap, damit Text im PDF durchsuchbar/kopierbar bleibt.
fn to_unicode_cmap(map: &HashMap<char, u16>) -> String {
    let mut pairs: Vec<(u16, char)> = map.iter().map(|(c, g)| (*g, *c)).filter(|(g, _)| *g != 0).collect();
    pairs.sort();
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def /CMapType 2 def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    for chunk in pairs.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, c) in chunk {
            let mut buf = [0u16; 2];
            let utf16: String = c.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
            s.push_str(&format!("<{gid:04X}> <{utf16}>\n"));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
    s
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

    /// Pfad zu einer echten Webstamp-PDF für manuelle Tests (Umgebungsvariable `WEBSTAMP_SAMPLE`).
    fn sample() -> Option<std::path::PathBuf> {
        std::env::var_os("WEBSTAMP_SAMPLE").map(Into::into)
    }

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

    fn blank_template() -> Template {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 649.into(), 459.into()],
            "Resources" => dictionary! {},
        });
        doc.objects.insert(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }.into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = vec![];
        doc.save_to(&mut bytes).unwrap();
        Template { bytes, width_mm: 229.0, height_mm: 162.0 }
    }

    #[test]
    fn embeds_subset_font() {
        let sf = crate::fonts::SystemFonts::load();
        let Some(family) = sf.helvetica_like().map(str::to_owned) else { return };
        let mut b = block("Ärger **GmbH**\nMüllerstraße 5", Align::Right);
        b.font = family.clone();
        let out = std::env::temp_dir().join("env_embed_test.pdf");
        let missing = export_pdf(
            &blank_template(),
            &[b],
            &[],
            &out,
            &mut |_, s, bold| m(s, bold),
            &|f, bold| sf.font_data(f, bold),
        )
        .unwrap();
        assert!(missing.is_empty(), "{missing:?}");

        // Die Teilschrift ist eingebettet und deutlich kleiner als die ganze Schrift.
        let doc = Document::load(&out).unwrap();
        let embedded: Vec<usize> = doc
            .objects
            .values()
            .filter_map(|o| match o {
                Object::Stream(s) if s.dict.has(b"Length1") || s.dict.has(b"Subtype") => Some(s.content.len()),
                _ => None,
            })
            .collect();
        assert!(!embedded.is_empty(), "keine eingebettete Schrift gefunden");
        assert!(embedded.iter().all(|&n| n < 200_000), "Teilschrift zu groß: {embedded:?}");
    }

    /// Manuelle Prüfung mit echter Vorlage und mehreren Systemschriften (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn export_real_fonts() {
        let sf = crate::fonts::SystemFonts::load();
        let Some(sample) = sample() else { return };
        let t = load(&sample).unwrap();
        let families = ["Times New Roman", "Verdana", "Georgia", "Consolas", "Segoe UI", "Bahnschrift"];
        let blocks: Vec<Block> = families
            .iter()
            .enumerate()
            .map(|(i, f)| Block {
                text: format!("{f}: **Ärger GmbH** €\nMüllerstraße 5, 12345 Köln"),
                pos: [12.0, 20.0 + i as f32 * 20.0],
                width_mm: 110.0,
                size_pt: 12.0,
                align: if i % 3 == 0 { Align::Left } else if i % 3 == 1 { Align::Right } else { Align::Justify },
                font: f.to_string(),
            })
            .collect();
        let out = std::env::temp_dir().join("env_fonts_test.pdf");
        let mut measure = |b: &Block, s: &str, bold: bool| sf.text_width_mm(&b.font, bold, s, b.size_pt);
        let missing = export_pdf(&t, &blocks, &[], &out, &mut measure, &|f, b| sf.font_data(f, b)).unwrap();
        println!("fehlend: {missing:?} → {}", out.display());
    }

    #[test]
    fn render_and_export() {
        let Some(sample) = sample() else { return };
        let t = load(&sample).unwrap();
        assert!((t.width_mm - 229.0).abs() < 1.0 && (t.height_mm - 162.0).abs() < 1.0);
        let r = render_template(&t, 150.0).unwrap();
        let dark = r.rgba.chunks(4).filter(|p| p[0] < 128).count();
        assert!(dark > 1000, "Stempel wurde nicht gerendert ({dark})");
        let blocks = vec![block("Ärger **GmbH**\nMüllerstraße 5\n12345 Köln", Align::Right)];
        let out = std::env::temp_dir().join("env_test.pdf");
        let img = crate::graphic::Graphic::Raster(image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255])));
        let placed = [Placed { img: &img, pos: [12.0, 100.0], width_mm: 40.0 }];
        assert_eq!(placed[0].height_mm(), 20.0);
        // ohne Schriftdaten → Helvetica-Rückfall, Familie wird gemeldet
        let missing = export_pdf(&t, &blocks, &placed, &out, &mut |_, s, bold| m(s, bold), &|_, _| None).unwrap();
        assert_eq!(missing, vec!["Arial".to_string()]);
        println!("{}", out.display());
    }
}

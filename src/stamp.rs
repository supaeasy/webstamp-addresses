use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::{Document, Object, Stream, dictionary};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

pub const PT_PER_MM: f32 = 72.0 / 25.4;

/// Ein Adressblock: linksbündig, eine einheitliche Schrift in Schwarz.
/// `pos` ist die Oberkante der Oberlängen der ersten Zeile (mm), `ascent_mm` der Abstand von dort
/// zur Grundlinie, `pitch_mm` der Zeilenabstand von Grundlinie zu Grundlinie.
#[derive(Clone, Debug)]
pub struct Block {
    pub text: String,
    pub pos: [f32; 2],
    pub size_pt: f32,
    pub font: String,
    pub ascent_mm: f32,
    pub pitch_mm: f32,
}

/// Ein zu zeichnender Textabschnitt (absolute Position in mm, Grundlinie).
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub x_mm: f32,
    pub baseline_mm: f32,
    pub text: String,
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
    /// Die Adresszeilen: ohne Leerzeilen (verboten) und ohne führende/nachfolgende Leerzeichen
    /// (Anschlag linksbündig).
    pub fn lines(&self) -> Vec<&str> {
        self.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
    }

    /// Grundlinie der Zeile `i` in mm.
    pub fn baseline_mm(&self, i: usize) -> f32 {
        self.pos[1] + self.ascent_mm + i as f32 * self.pitch_mm
    }
}

/// Positionen aller Zeilen. Alle beginnen am linken Rand des Blocks.
pub fn layout(b: &Block) -> Vec<Run> {
    b.lines()
        .into_iter()
        .enumerate()
        .map(|(i, l)| Run { x_mm: b.pos[0], baseline_mm: b.baseline_mm(i), text: l.to_string() })
        .collect()
}

pub struct Template {
    pub bytes: Vec<u8>,
    pub width_mm: f32,
    pub height_mm: f32,
}

/// Standardformat des Umschlags (C5, quer), solange keine Webstamp-PDF geladen ist.
pub const C5_MM: (f32, f32) = (229.0, 162.0);

/// Leere Seite in Umschlaggröße – Vorlage, wenn ohne Webstamp gedruckt wird.
pub fn blank_template(width_mm: f32, height_mm: f32) -> Template {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), (width_mm * PT_PER_MM).into(), (height_mm * PT_PER_MM).into()],
        "Resources" => dictionary! {},
    });
    doc.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut bytes = vec![];
    doc.save_to(&mut bytes).expect("leeres PDF");
    Template { bytes, width_mm, height_mm }
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

/// Liefert die Schriftdaten (Bytes, Index in der Collection) zu einer Familie.
pub type FaceFn<'a> = &'a dyn Fn(&str) -> Option<(Vec<u8>, u32)>;

/// Schreibt Vorlage + Adressen + Bilder als PDF. Die gewählten Schriften werden als Teilschrift
/// (nur die benutzten Zeichen) eingebettet; fehlt eine Schrift, wird Helvetica verwendet.
/// Rückgabe: Familien ohne Schriftdaten.
pub fn export_pdf(
    t: &Template,
    blocks: &[Block],
    images: &[Placed],
    out: &Path,
    face: FaceFn,
) -> Result<Vec<String>, String> {
    let mut doc = Document::load_mem(&t.bytes).map_err(|e| e.to_string())?;
    let page_id = *doc.get_pages().values().next().ok_or("keine Seite")?;
    let page_h_pt = t.height_mm * PT_PER_MM;

    // 1. Layout aller Blöcke und benutzte Zeichen je Schrift
    let layouts: Vec<Vec<Run>> = blocks.iter().map(layout).collect();
    let mut used: BTreeMap<String, BTreeSet<char>> = BTreeMap::new();
    for (b, runs) in blocks.iter().zip(&layouts) {
        for r in runs {
            used.entry(b.font.clone()).or_default().extend(r.text.chars());
        }
    }

    // 2. Schriften einbetten
    let mut fonts = dictionary! {};
    let mut embedded: HashMap<String, (String, HashMap<char, u16>)> = HashMap::new();
    let mut missing: Vec<String> = vec![];
    for (n, (family, chars)) in used.iter().enumerate() {
        match face(family).and_then(|(data, index)| embed_font(&mut doc, &data, index, chars, family)) {
            Some((id, map)) => {
                let name = format!("FE{n}");
                fonts.set(name.as_bytes().to_vec(), Object::Reference(id));
                embedded.insert(family.clone(), (name, map));
            }
            None => missing.push(family.clone()),
        }
    }

    // Helvetica als Rückfall
    let helvetica = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    fonts.set("FEnv", Object::Reference(helvetica));

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

    // 4. Text (schwarz)
    for (b, runs) in blocks.iter().zip(&layouts) {
        for r in runs {
            let (name, shown) = match embedded.get(&b.font) {
                Some((name, map)) => {
                    let hex: String = r
                        .text
                        .chars()
                        .map(|c| format!("{:04X}", map.get(&c).copied().unwrap_or(0)))
                        .collect();
                    (name.as_str(), format!("<{hex}>"))
                }
                None => ("FEnv", pdf_string(&r.text)),
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

    fn block(text: &str) -> Block {
        Block {
            text: text.into(),
            pos: [10.0, 20.0],
            size_pt: 10.0,
            font: "Arial".into(),
            ascent_mm: 2.5,
            pitch_mm: 4.75,
        }
    }

    #[test]
    fn lines_drop_blanks_and_indent() {
        let b = block("  Max Mustermann \n\n   Musterstraße 1\n\n12345 Musterstadt\n");
        assert_eq!(b.lines(), vec!["Max Mustermann", "Musterstraße 1", "12345 Musterstadt"]);
    }

    #[test]
    fn layout_is_left_aligned_with_fixed_pitch() {
        let runs = layout(&block("a\n\nb\nc"));
        assert_eq!(runs.len(), 3);
        assert!(runs.iter().all(|r| r.x_mm == 10.0));
        assert_eq!(runs[0].baseline_mm, 22.5);
        assert!((runs[1].baseline_mm - runs[0].baseline_mm - 4.75).abs() < 1e-4);
        assert!((runs[2].baseline_mm - runs[1].baseline_mm - 4.75).abs() < 1e-4);
    }

    fn blank() -> Template {
        blank_template(229.0, 162.0)
    }

    #[test]
    fn blank_template_has_requested_size() {
        let t = blank_template(100.0, 50.0);
        let doc = Document::load_mem(&t.bytes).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        let mb = doc.get_dictionary(page).unwrap().get(b"MediaBox").unwrap().as_array().unwrap().clone();
        let w = mb[2].as_float().unwrap();
        assert!((w - 100.0 * PT_PER_MM).abs() < 0.01);
    }

    #[test]
    fn embeds_subset_font() {
        let sf = crate::fonts::SystemFonts::load();
        let Some(family) = sf.allowed.first().cloned() else { return };
        let mut b = block("Ärger GmbH\nMüllerstraße 5\n12345 Köln");
        b.font = family;
        let out = std::env::temp_dir().join("env_embed_test.pdf");
        let missing = export_pdf(&blank(), &[b], &[], &out, &|f| sf.font_data(f)).unwrap();
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

    /// Manuelle Prüfung des Zeilenabstands: PDF mit Unter- und Oberlängen (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn write_line_gap_sample() {
        let sf = crate::fonts::SystemFonts::load();
        let family = sf.allowed.first().cloned().expect("keine erlaubte Schrift");
        let (up, down) = sf.line_metrics(&family).unwrap();
        let (size, gap) = (10.0f32, 1.25f32);
        let em_mm = size / PT_PER_MM;
        let b = Block {
            text: "gjpqy gjpqy\nbdfhkl bdfhkl\ngjpqy gjpqy".into(),
            pos: [20.0, 20.0],
            size_pt: size,
            font: family.clone(),
            ascent_mm: up * em_mm,
            pitch_mm: (up + down) * em_mm + gap,
        };
        let out = std::env::temp_dir().join("env_gap.pdf");
        export_pdf(&blank(), &[b], &[], &out, &|f| sf.font_data(f)).unwrap();
        println!("{family}: up {up:.3} down {down:.3} em, Soll-Abstand {gap} mm → {}", out.display());
    }

    #[test]
    fn export_falls_back_to_helvetica() {
        let out = std::env::temp_dir().join("env_fallback_test.pdf");
        let missing = export_pdf(&blank(), &[block("Ärger\nKöln")], &[], &out, &|_| None).unwrap();
        assert_eq!(missing, vec!["Arial".to_string()]);
    }

    #[test]
    fn render_and_export() {
        let Some(sample) = sample() else { return };
        let t = load(&sample).unwrap();
        assert!((t.width_mm - 229.0).abs() < 1.0 && (t.height_mm - 162.0).abs() < 1.0);
        let r = render_template(&t, 150.0).unwrap();
        let dark = r.rgba.chunks(4).filter(|p| p[0] < 128).count();
        assert!(dark > 1000, "Stempel wurde nicht gerendert ({dark})");
        let img = crate::graphic::Graphic::Raster(image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255])));
        let placed = [Placed { img: &img, pos: [12.0, 100.0], width_mm: 40.0 }];
        assert_eq!(placed[0].height_mm(), 20.0);
        let out = std::env::temp_dir().join("env_test.pdf");
        export_pdf(&t, &[block("Ärger GmbH\nMüllerstraße 5\n12345 Köln")], &placed, &out, &|_| None).unwrap();
    }
}

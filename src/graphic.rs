//! Einsetzbare Grafiken: Rasterbilder (PNG, JPG, …) und Vektorgrafiken (SVG).
//! SVGs werden erst beim Ausgeben in der benötigten Auflösung gerendert und bleiben dadurch scharf.

use image::RgbaImage;
use resvg::{tiny_skia, usvg};
use std::path::Path;

pub enum Graphic {
    Raster(RgbaImage),
    Svg(Box<usvg::Tree>),
}

pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

impl Graphic {
    pub fn load(path: &Path) -> Result<Self, String> {
        if is_svg(path) {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let mut opt = usvg::Options::default();
            opt.fontdb_mut().load_system_fonts();
            let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| format!("SVG nicht lesbar: {e}"))?;
            if tree.size().width() <= 0.0 || tree.size().height() <= 0.0 {
                return Err("SVG hat keine Größe".into());
            }
            Ok(Self::Svg(Box::new(tree)))
        } else {
            image::open(path).map(|i| Self::Raster(i.to_rgba8())).map_err(|e| e.to_string())
        }
    }

    pub fn is_vector(&self) -> bool {
        matches!(self, Self::Svg(_))
    }

    /// Seitenverhältnis Höhe / Breite.
    pub fn aspect(&self) -> f32 {
        match self {
            Self::Raster(i) => i.height() as f32 / i.width() as f32,
            Self::Svg(t) => t.size().height() / t.size().width(),
        }
    }

    /// Beschreibung der Originalgröße für die Statusleiste.
    pub fn describe(&self) -> String {
        match self {
            Self::Raster(i) => format!("{} × {} px", i.width(), i.height()),
            Self::Svg(_) => "Vektorgrafik".into(),
        }
    }

    /// Rendert/skaliert die Grafik auf genau `w` × `h` Pixel (unverrechnet, mit Alpha).
    pub fn render(&self, w: u32, h: u32) -> RgbaImage {
        let (w, h) = (w.max(1), h.max(1));
        match self {
            Self::Raster(i) => image::imageops::resize(i, w, h, image::imageops::FilterType::CatmullRom),
            Self::Svg(tree) => {
                let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("Pixmap");
                let t = tiny_skia::Transform::from_scale(w as f32 / tree.size().width(), h as f32 / tree.size().height());
                resvg::render(tree, t, &mut pixmap.as_mut());
                // tiny-skia liefert vormultiplizierte Farben → in normale Farben zurückrechnen.
                let mut out = RgbaImage::new(w, h);
                for (o, p) in out.pixels_mut().zip(pixmap.pixels()) {
                    let c = p.demultiply();
                    *o = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
                }
                out
            }
        }
    }

    /// Bild für die Vorschau (längste Kante höchstens `max` px).
    pub fn preview(&self, max: u32) -> RgbaImage {
        match self {
            Self::Raster(i) if i.width().max(i.height()) <= max => i.clone(),
            _ => {
                let a = self.aspect();
                let (w, h) = if a <= 1.0 { (max, (max as f32 * a) as u32) } else { ((max as f32 / a) as u32, max) };
                self.render(w, h)
            }
        }
    }

    /// Bild für den PDF-Export: Raster auf höchstens 2400 px, SVG mit 300 dpi (ebenfalls ≤ 2400 px).
    pub fn for_pdf(&self, w_mm: f32, h_mm: f32) -> RgbaImage {
        match self {
            Self::Raster(i) => {
                let big = i.width().max(i.height());
                if big > 2400 {
                    let f = 2400.0 / big as f32;
                    self.render(((i.width() as f32 * f) as u32).max(1), ((i.height() as f32 * f) as u32).max(1))
                } else {
                    i.clone()
                }
            }
            Self::Svg(_) => {
                let (w, h) = ((w_mm / 25.4 * 300.0) as u32, (h_mm / 25.4 * 300.0) as u32);
                let f = (2400.0 / w.max(h).max(1) as f32).min(1.0);
                self.render((w as f32 * f) as u32, (h as f32 * f) as u32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_renders_with_alpha() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
            <rect x="0" y="0" width="20" height="20" fill="#ff0000"/></svg>"##;
        let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).unwrap();
        let g = Graphic::Svg(Box::new(tree));
        assert_eq!(g.aspect(), 0.5);
        let img = g.render(400, 200);
        assert_eq!(img.dimensions(), (400, 200));
        assert_eq!(img.get_pixel(50, 100).0, [255, 0, 0, 255]); // linke Hälfte rot
        assert_eq!(img.get_pixel(300, 100).0[3], 0); // rechte Hälfte transparent
    }
}

//! Direktdruck über GDI: Druckdialog, Stempel als Bitmap, Adressen als Vektortext.

use crate::stamp::{Block, Placed, Raster};
use windows::Win32::Foundation::{COLORREF, HGLOBAL};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::core::PCWSTR;

/// Merkt sich die zuletzt gewählten Druckereinstellungen (Drucker, Papierformat …).
#[derive(Default)]
pub struct PrintState {
    dev_mode: isize,
    dev_names: isize,
}

pub struct Job<'a> {
    pub name: &'a str,
    pub env_w_mm: f32,
    pub env_h_mm: f32,
    pub stamp: &'a Raster,
    pub stamp_dpi: f32,
    pub blocks: &'a [Block],
    pub images: &'a [Placed<'a>],
    pub flip_180: bool,
    pub offset_mm: [f32; 2],
}

/// Rückgabe: Ok(false) = Benutzer hat den Dialog abgebrochen.
pub fn print(state: &mut PrintState, job: &Job) -> Result<bool, String> {
    unsafe {
        let mut pd = PRINTDLGW {
            lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
            hwndOwner: GetActiveWindow(),
            hDevMode: HGLOBAL(state.dev_mode as *mut _),
            hDevNames: HGLOBAL(state.dev_names as *mut _),
            Flags: PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS | PD_USEDEVMODECOPIESANDCOLLATE,
            nCopies: 1,
            ..Default::default()
        };
        if !PrintDlgW(&mut pd).as_bool() {
            // Abbruch oder Fehler
            let err = CommDlgExtendedError();
            return if err.0 == 0 {
                Ok(false)
            } else {
                Err(format!("Druckdialog-Fehler {:#x}", err.0))
            };
        }
        state.dev_mode = pd.hDevMode.0 as isize;
        state.dev_names = pd.hDevNames.0 as isize;
        let hdc = pd.hDC;
        if hdc.is_invalid() {
            return Err("Kein Druckerkontext erhalten".into());
        }
        let r = draw(hdc, job);
        let _ = DeleteDC(hdc);
        r.map(|_| true)
    }
}

unsafe fn draw(hdc: HDC, job: &Job) -> Result<(), String> {
    unsafe {
        let dpi_x = GetDeviceCaps(Some(hdc), LOGPIXELSX) as f32;
        let dpi_y = GetDeviceCaps(Some(hdc), LOGPIXELSY) as f32;
        let phys_w = GetDeviceCaps(Some(hdc), PHYSICALWIDTH) as f32;
        let phys_h = GetDeviceCaps(Some(hdc), PHYSICALHEIGHT) as f32;
        let off_x = GetDeviceCaps(Some(hdc), PHYSICALOFFSETX) as f32;
        let off_y = GetDeviceCaps(Some(hdc), PHYSICALOFFSETY) as f32;
        let mm_x = dpi_x / 25.4;
        let mm_y = dpi_y / 25.4;

        // Der Umschlag liegt quer (Breite > Höhe). Hochformat-Papier ⇒ um 90° drehen.
        let env_landscape = job.env_w_mm >= job.env_h_mm;
        let dev_landscape = phys_w >= phys_h;
        let mut rot = if env_landscape != dev_landscape { 90 } else { 0 };
        if job.flip_180 {
            rot = (rot + 180) % 360;
        }

        let (ew, eh) = (job.env_w_mm, job.env_h_mm);
        // Umschlag-mm (x, y) → Geräte-Pixel
        let map = |x: f32, y: f32| -> (f32, f32) {
            let (x, y) = (x + job.offset_mm[0], y + job.offset_mm[1]);
            let (dx, dy) = match rot {
                0 => (x, y),
                90 => (eh - y, x),
                180 => (ew - x, eh - y),
                _ => (y, ew - x),
            };
            (dx * mm_x - off_x, dy * mm_y - off_y)
        };

        let name_w = wide(job.name);
        let di = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(name_w.as_ptr()),
            ..Default::default()
        };
        if StartDocW(hdc, &di) <= 0 {
            return Err("StartDoc fehlgeschlagen".into());
        }
        if StartPage(hdc) <= 0 {
            let _ = EndDoc(hdc);
            return Err("StartPage fehlgeschlagen".into());
        }

        draw_stamp(hdc, job, rot, dpi_x, &map);
        for p in job.images {
            draw_image(hdc, p, rot, mm_x, mm_y, &map);
        }

        // Text
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0));
        SetTextAlign(hdc, TA_LEFT | TA_BASELINE);
        let escapement = match rot {
            0 => 0,
            90 => 2700,
            180 => 1800,
            _ => 900,
        };
        for b in job.blocks {
            let face = wide(&b.font);
            let height = -((b.size_pt / 72.0 * dpi_y).round() as i32);
            let make_font = |weight: i32| {
                CreateFontW(
                    height,
                    0,
                    escapement,
                    escapement,
                    weight,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_TT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    PCWSTR(face.as_ptr()),
                )
            };
            let (regular, bold) = (make_font(400), make_font(700));
            let old = SelectObject(hdc, regular.into());

            // Textbreiten mit den echten Druckerschriften messen, damit Rechtsbündig/Blocksatz stimmen.
            let runs = crate::stamp::layout(b, &mut |s, is_bold| {
                SelectObject(hdc, (if is_bold { bold } else { regular }).into());
                let w: Vec<u16> = s.encode_utf16().collect();
                let mut size = windows::Win32::Foundation::SIZE::default();
                let _ = GetTextExtentPoint32W(hdc, &w, &mut size);
                size.cx as f32 / mm_x
            });
            for r in &runs {
                SelectObject(hdc, (if r.bold { bold } else { regular }).into());
                let (px, py) = map(r.x_mm, r.baseline_mm);
                let w: Vec<u16> = r.text.encode_utf16().collect();
                let _ = TextOutW(hdc, px.round() as i32, py.round() as i32, &w);
            }
            SelectObject(hdc, old);
            let _ = DeleteObject(regular.into());
            let _ = DeleteObject(bold.into());
        }

        if EndPage(hdc) <= 0 {
            let _ = EndDoc(hdc);
            return Err("EndPage fehlgeschlagen".into());
        }
        if EndDoc(hdc) <= 0 {
            return Err("EndDoc fehlgeschlagen".into());
        }
        Ok(())
    }
}

/// Zeichnet nur den nicht-weißen Bereich des Stempels (klein, schnell zu spoolen).
unsafe fn draw_stamp(hdc: HDC, job: &Job, rot: u32, dpi_x: f32, map: &dyn Fn(f32, f32) -> (f32, f32)) {
    let r = job.stamp;
    let Some((x0, y0, x1, y1)) = bbox(r) else { return };
    let (w, h) = (x1 - x0, y1 - y0);
    // BGRA, top-down
    let mut crop = Vec::with_capacity(w * h * 4);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = &r.rgba[(y * r.w + x) * 4..][..4];
            crop.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    let (buf, rw, rh) = rotate(&crop, w, h, rot);

    // Zielrechteck: beide Ecken des Ausschnitts abbilden
    let px_mm = job.stamp_dpi / 25.4;
    let (ax, ay) = map(x0 as f32 / px_mm, y0 as f32 / px_mm);
    let (bx, by) = map(x1 as f32 / px_mm, y1 as f32 / px_mm);
    let (dx, dy) = (ax.min(bx), ay.min(by));
    let scale = dpi_x / job.stamp_dpi;
    let (dw, dh) = ((rw as f32 * scale).round() as i32, (rh as f32 * scale).round() as i32);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: rw as i32,
            biHeight: -(rh as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetStretchBltMode(hdc, COLORONCOLOR);
        StretchDIBits(
            hdc,
            dx.round() as i32,
            dy.round() as i32,
            dw,
            dh,
            0,
            0,
            rw as i32,
            rh as i32,
            Some(buf.as_ptr() as *const _),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

/// Zeichnet ein Bild in Druckerauflösung (Transparenz wird auf Weiß gerechnet).
unsafe fn draw_image(hdc: HDC, p: &Placed, rot: u32, mm_x: f32, mm_y: f32, map: &dyn Fn(f32, f32) -> (f32, f32)) {
    let (w_mm, h_mm) = (p.width_mm, p.height_mm());
    let (tw, th) = (((w_mm * mm_x).round() as u32).max(1), ((h_mm * mm_y).round() as u32).max(1));
    let resized = image::imageops::resize(p.img, tw, th, image::imageops::FilterType::CatmullRom);

    let mut bgra = Vec::with_capacity(tw as usize * th as usize * 4);
    for px in resized.pixels() {
        let a = px[3] as u32;
        let over_white = |v: u8| ((v as u32 * a + 255 * (255 - a)) / 255) as u8;
        bgra.extend_from_slice(&[over_white(px[2]), over_white(px[1]), over_white(px[0]), 255]);
    }
    let (buf, rw, rh) = rotate(&bgra, tw as usize, th as usize, rot);

    let (ax, ay) = map(p.pos[0], p.pos[1]);
    let (bx, by) = map(p.pos[0] + w_mm, p.pos[1] + h_mm);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: rw as i32,
            biHeight: -(rh as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetStretchBltMode(hdc, COLORONCOLOR);
        StretchDIBits(
            hdc,
            ax.min(bx).round() as i32,
            ay.min(by).round() as i32,
            rw as i32,
            rh as i32,
            0,
            0,
            rw as i32,
            rh as i32,
            Some(buf.as_ptr() as *const _),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

fn bbox(r: &Raster) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (r.w, r.h, 0, 0);
    for y in 0..r.h {
        for x in 0..r.w {
            let p = &r.rgba[(y * r.w + x) * 4..][..3];
            if p.iter().any(|&c| c < 250) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0).then(|| (x0.saturating_sub(2), y0.saturating_sub(2), (x1 + 2).min(r.w), (y1 + 2).min(r.h)))
}

/// Dreht ein 4-Byte-Pixel-Raster im Uhrzeigersinn um `rot` Grad (0/90/180/270).
fn rotate(src: &[u8], w: usize, h: usize, rot: u32) -> (Vec<u8>, usize, usize) {
    let (nw, nh) = if rot % 180 == 90 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match rot {
                0 => (x, y),
                90 => (h - 1 - y, x),
                180 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            out[(ny * nw + nx) * 4..][..4].copy_from_slice(&src[(y * w + x) * 4..][..4]);
        }
    }
    (out, nw, nh)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

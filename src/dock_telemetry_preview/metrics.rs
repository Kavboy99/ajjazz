//! Same coordinates, RGB565 blending and units as screens_metrics.h.
use super::*;
use crate::dock_pages::Kind;
fn value(p: &mut Painter, x: u32, y: u32, v: Option<u32>, font: usize, ink: Rgba<u8>) {
    p.modern(
        &v.map_or_else(|| "--".into(), |v| v.to_string()),
        x,
        y,
        font,
        ink,
    );
}
#[allow(clippy::too_many_arguments)]
fn temperature(
    p: &mut Painter,
    x: u32,
    y: u32,
    label: &str,
    v: Option<i16>,
    ink: Rgba<u8>,
    accent: Rgba<u8>,
) {
    p.modern(label, x, y, 0, accent);
    let s = v.map_or_else(
        || "--".into(),
        |v| {
            format!(
                "{}{}",
                if v < 0 { "-" } else { "" },
                (i32::from(v).abs() + 5) / 10
            )
        },
    );
    let font = if Painter::modern_width(&s, 1) <= 36 {
        1
    } else {
        0
    };
    p.modern(&s, x, y + 18, font, ink);
    let cx = x + Painter::modern_width(&s, font) + 2;
    p.rect(cx, y + 18, 2, 2, accent);
    p.text("C", cx + 3, y + 22, 1, accent);
}
pub(super) fn render(c: &Config, kind: Kind, s: Option<&Sample>) -> RgbaImage {
    let ink = color(c.foreground);
    let accent = color(c.accent);
    let bg = color(c.background);
    let mut p = Painter(RgbaImage::from_pixel(128, 128, bg));
    if c.border {
        p.rect(0, 0, 128, 1, accent);
        p.rect(0, 127, 128, 1, accent);
    }
    let g = s.map(|s| &s.graphics);
    let t = g.map(|g| &g.temperatures);
    let cpu = t.and_then(|t| t.cpu);
    let gpu = t.and_then(|t| t.gpu);
    match kind {
        Kind::Gaming => {
            p.modern("FPS", 8, 6, 0, accent);
            let fps = g.and_then(|g| g.fps_tenths);
            value(&mut p, 8, 27, fps.map(|v| (u32::from(v) + 5) / 10), 2, ink);
            if let Some(v) = fps.filter(|v| *v > 0) {
                let ms = 100000 / u32::from(v);
                let text = format!("{}.{:01}", ms / 10, ms % 10);
                p.modern(&text, 8, 64, 0, accent);
                p.modern("MS", 8 + Painter::modern_width(&text, 0) + 5, 64, 0, accent);
            }
            temperature(&mut p, 8, 85, "CPU", cpu, ink, accent);
            temperature(&mut p, 70, 85, "GPU", gpu, ink, accent);
        }
        Kind::Thermals => {
            temperature(&mut p, 8, 6, "CPU", cpu, ink, accent);
            temperature(&mut p, 70, 6, "GPU", gpu, ink, accent);
            temperature(
                &mut p,
                8,
                68,
                "DISK",
                t.and_then(|t| t.storage),
                ink,
                accent,
            );
            temperature(&mut p, 70, 68, "SYS", t.and_then(|t| t.board), ink, accent);
        }
        Kind::Gpu => {
            p.modern("GPU", 8, 6, 0, accent);
            p.modern("%", 105, 41, 0, accent);
            value(
                &mut p,
                8,
                27,
                g.and_then(|g| g.gpu_permille).map(|v| u32::from(v) / 10),
                2,
                ink,
            );
            if let Some(v) = g.and_then(|g| g.power_deciwatts) {
                let text = ((u32::from(v) + 5) / 10).to_string();
                p.modern(
                    &text,
                    110u32.saturating_sub(Painter::modern_width(&text, 0)),
                    6,
                    0,
                    ink,
                );
                p.text("W", 114, 10, 1, accent);
            }
            p.text("VRAM %", 8, 88, 1, accent);
            let vram = g
                .and_then(|g| Some((g.vram_used_mib?, g.vram_total_mib?)))
                .filter(|(_, t)| *t > 0)
                .map(|(u, t)| u * 100 / t);
            value(&mut p, 8, 103, vram, 1, ink);
            temperature(&mut p, 70, 85, "GPU", gpu, ink, accent);
        }
        Kind::Pulse => {
            p.modern("CPU %", 8, 5, 0, accent);
            value(
                &mut p,
                72,
                2,
                s.map(|s| u32::from(s.cpu_permille) / 10),
                1,
                ink,
            );
            p.modern("GPU %", 8, 68, 0, accent);
            value(
                &mut p,
                72,
                65,
                g.and_then(|g| g.gpu_permille).map(|v| u32::from(v) / 10),
                1,
                ink,
            );
            for (y, shift, ink) in [(29, 0, accent), (92, 4, ink)] {
                p.modern_bar(8, y + 26, 112, 1, None, ink, bg);
                if let Some(g) = g {
                    for (i, v) in g.load_history.iter().take(32).enumerate() {
                        let v = (v >> shift) & 15;
                        if v == 15 {
                            continue;
                        }
                        let h = u32::from(v) * 24 / 14 + 1;
                        p.rect(117 - i as u32 * 3, y + 25 - h, 2, h, ink);
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    p.0
}

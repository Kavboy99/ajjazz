use super::*;
use crate::dock_composition::{Composition, Metric};
use chrono::Timelike;
fn label(m: Metric) -> &'static str {
    match m {
        Metric::Cpu => "CPU %",
        Metric::Ram => "RAM %",
        Metric::Gpu => "GPU %",
        Metric::Vram => "VRAM%",
        Metric::Fps => "FPS",
        Metric::CpuTemp => "CPU",
        Metric::GpuTemp => "GPU",
        Metric::DiskTemp => "DISK",
        Metric::BoardTemp => "SYS",
        Metric::Power => "GPU W",
        Metric::Clock => "TIME",
        Metric::None => "",
    }
}
fn number(m: Metric, s: Option<&Sample>, now: chrono::DateTime<chrono::Local>) -> Option<String> {
    let g = s.map(|s| &s.graphics);
    Some(match m {
        Metric::Cpu => (s?.cpu_permille / 10).to_string(),
        Metric::Ram => (s?.ram_permille / 10).to_string(),
        Metric::Gpu => (g?.gpu_permille? / 10).to_string(),
        Metric::Vram => {
            let g = g?;
            let total = g.vram_total_mib?;
            if total == 0 {
                return None;
            }
            (g.vram_used_mib?.min(total) * 100 / total).to_string()
        }
        Metric::Fps => ((u32::from(g?.fps_tenths?) + 5) / 10).to_string(),
        Metric::Power => ((u32::from(g?.power_deciwatts?) + 5) / 10).to_string(),
        Metric::CpuTemp | Metric::GpuTemp | Metric::DiskTemp | Metric::BoardTemp => {
            let t = &g?.temperatures;
            let v = match m {
                Metric::CpuTemp => t.cpu,
                Metric::GpuTemp => t.gpu,
                Metric::DiskTemp => t.storage,
                _ => t.board,
            }?;
            format!(
                "{}{}",
                if v < 0 { "-" } else { "" },
                (i32::from(v).abs() + 5) / 10
            )
        }
        Metric::Clock => format!("{:02}:{:02}", now.hour(), now.minute()),
        Metric::None => return None,
    })
}
pub(super) fn render(
    c: &Config,
    layout: &Composition,
    s: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
) -> RgbaImage {
    render_at(
        c,
        layout,
        s,
        now,
        ((now.timestamp_millis() * 9 / 1000) & 255) as u8,
    )
}
pub(super) fn render_at(
    c: &Config,
    layout: &Composition,
    s: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
    phase: u8,
) -> RgbaImage {
    let bg = color(c.background);
    let accent = color(c.accent);
    let ink = color(c.foreground);
    let mut p = Painter(RgbaImage::from_pixel(128, 128, bg));
    let a = modern::cover_color(accent, bg, 1);
    let effect = layout.backdrop as u32;
    let ph = u32::from(phase);
    for y in 0..128 {
        for x in 0..128 {
            if (4..124).contains(&x) && (4..118).contains(&y) {
                continue;
            }
            let on = match effect {
                1 => {
                    y == 124
                        && x >= 64 - (ph % 32).min(31 - ph % 32) * 3
                        && x <= 64 + (ph % 32).min(31 - ph % 32) * 3
                }
                2 | 4 => {
                    let pos = if y == 2 {
                        x
                    } else if x == 125 {
                        125 + y
                    } else if y == 125 {
                        375 - x
                    } else if x == 2 {
                        500 - y
                    } else {
                        1000
                    };
                    if effect == 2 {
                        pos < 500 && ((pos + 500 - (ph * 3) % 500) % 500) < 8
                    } else {
                        pos < 500 && ((pos + 500 - (ph * 2) % 125) % 125) < 2
                    }
                }
                3 => y == 122 - ((x + ph) % 16).min(15 - (x + ph) % 16) / 2,
                5 => (y == 2 || y == 125) && (x + 128 - (ph * 3) % 128) % 128 < 20,
                _ => false,
            };
            if on {
                p.rect(x, y, 1, 1, a)
            }
        }
    }
    for (i, rect) in layout.rects().iter().enumerate() {
        let [x, y, w, h] = *rect;
        let m = layout.metrics[i];
        let lw =
            Painter::modern_width(label(m), 0) + if (6..=9).contains(&(m as u8)) { 15 } else { 0 };
        let lx = x + (w.saturating_sub(lw)) / 2;
        p.modern(label(m), lx, y, 0, accent);
        if (6..=9).contains(&(m as u8)) {
            let cx = lx + Painter::modern_width(label(m), 0) + 2;
            p.rect(cx, y + 1, 3, 1, accent);
            p.rect(cx, y + 2, 1, 1, accent);
            p.rect(cx + 2, y + 2, 1, 1, accent);
            p.rect(cx, y + 3, 3, 1, accent);
            p.modern("C", cx + 5, y, 0, accent);
        }
        let text = number(m, s, now).unwrap_or_else(|| "--".into());
        let mut font = if w > 60 { 2 } else { 4 };
        if font == 4 && Painter::modern_width(&text, font) > w {
            font = 0;
        } else {
            while font > 0 && Painter::modern_width(&text, font) > w {
                font -= 1;
            }
        }
        let vy = y + if w > 60 { 19 } else { 18 };
        p.modern(
            &text,
            x + (w - Painter::modern_width(&text, font)) / 2,
            vy,
            font,
            ink,
        );
        if layout.bars && (1..=4).contains(&(m as u8)) {
            let val = number(m, s, now)
                .and_then(|v| v.parse::<u16>().ok())
                .map(|v| v.min(100) * 10);
            p.modern_bar(x, y + h - 4, w, 3, val, accent, bg);
        }
    }
    p.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_digit_fps_stays_large_with_a_real_column_gutter() {
        assert!(Painter::modern_width("144", 4) <= 50);
        assert!(Painter::modern_width("360", 4) <= 50);
        let c = Composition::default();
        let r = c.rects();
        assert!(r[1][0] - (r[0][0] + r[0][2]) >= 10);
    }
}

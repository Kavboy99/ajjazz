//! Pixel-exact fonts, geometry and RGB565 composition used by the screen extension.
mod modern;
use crate::dock_telemetry::{Config, Sample};
use anyhow::Result;
use image::{Rgba, RgbaImage};
use std::sync::OnceLock;
mod composition;
mod extra;
mod metrics;

/// Optional fixed animation phase is a preview accessibility choice only.
/// Time and live sensor values continue to update.
pub fn render_composition(
    config: &Config,
    layout: &crate::dock_composition::Composition,
    sample: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
    phase: Option<u8>,
) -> RgbaImage {
    composition::render_at(
        config,
        layout,
        sample,
        now,
        phase.unwrap_or(((now.timestamp_millis() * 9 / 1000) & 255) as u8),
    )
}

#[derive(serde::Deserialize)]
struct Font {
    digits: [[u8; 7]; 10],
    letters: [[u8; 7]; 26],
}
fn bits(ch: u8, y: usize) -> u8 {
    static FONT: OnceLock<Font> = OnceLock::new();
    let font = FONT.get_or_init(|| {
        serde_json::from_str(include_str!("../firmware/aj159-screen/font.json"))
            .expect("Embedded screen font")
    });
    let ch = ch.to_ascii_uppercase();
    match ch {
        b'0'..=b'9' => font.digits[usize::from(ch - b'0')][y],
        b'A'..=b'Z' => font.letters[usize::from(ch - b'A')][y],
        b':' => {
            if y == 2 || y == 4 {
                4
            } else {
                0
            }
        }
        b'.' => {
            if y == 6 {
                4
            } else {
                0
            }
        }
        b'/' => 1 << (y * 4 / 6),
        b'-' => {
            if y == 3 {
                14
            } else {
                0
            }
        }
        b'%' => [25, 25, 2, 4, 8, 19, 19][y],
        _ => 0,
    }
}
fn color(c: [u8; 3]) -> Rgba<u8> {
    Rgba([
        ((u16::from(c[0] >> 3) * 255) / 31) as u8,
        ((u16::from(c[1] >> 2) * 255) / 63) as u8,
        ((u16::from(c[2] >> 3) * 255) / 31) as u8,
        255,
    ])
}
struct Painter(RgbaImage);
impl Painter {
    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, c: Rgba<u8>) {
        for py in y.min(128)..y.saturating_add(h).min(128) {
            for px in x.min(128)..x.saturating_add(w).min(128) {
                self.0.put_pixel(px, py, c)
            }
        }
    }
    fn text(&mut self, text: &str, x: u32, y: u32, scale: u32, c: Rgba<u8>) {
        for (i, ch) in text.bytes().enumerate() {
            let x = x + i as u32 * 6 * scale;
            if x >= 128 {
                break;
            }
            for row in 0..7 {
                for column in 0..5 {
                    if bits(ch, row) & (16 >> column) != 0 {
                        self.rect(x + column * scale, y + row as u32 * scale, scale, scale, c)
                    }
                }
            }
        }
    }
    fn metric(&mut self, c: &Config, at: [u8; 2], label: &str, value: Option<u16>) {
        let (x, y, scale) = (u32::from(at[0]), u32::from(at[1]), u32::from(c.label_scale));
        self.text(label, x, y, scale, color(c.foreground));
        if c.percentages {
            self.text(
                &value
                    .map(|v| format!("{}%", v / 10))
                    .unwrap_or_else(|| "--".into()),
                x + 30 * scale,
                y,
                scale,
                color(c.accent),
            );
        }
        if c.bars {
            let by = y + 7 * scale + 3;
            let (w, h) = (u32::from(c.bar_width), u32::from(c.bar_height));
            self.rect(x, by, w, h, color(c.foreground));
            self.rect(x + 1, by + 1, w - 2, h - 2, color(c.background));
            if let Some(v) = value {
                self.rect(
                    x + 1,
                    by + 1,
                    (w - 2) * u32::from(v) / 1000,
                    h - 2,
                    color(c.accent),
                );
            }
        }
    }
}
pub fn render(c: &Config, sample: Option<&Sample>, hours: u8, minutes: u8) -> Result<RgbaImage> {
    c.validate()?;
    if let Some(s) = sample {
        s.encode()?;
    }
    let mut p = Painter(RgbaImage::from_pixel(128, 128, color(c.background)));
    if c.border {
        p.rect(0, 0, 128, 1, color(c.accent));
        p.rect(0, 127, 128, 1, color(c.accent));
    }
    if c.show_title {
        p.text(
            &c.title,
            u32::from(c.title_position[0]),
            u32::from(c.title_position[1]),
            1,
            color(c.accent),
        );
    }
    p.metric(c, c.cpu_position, "CPU", sample.map(|s| s.cpu_permille));
    p.metric(c, c.ram_position, "RAM", sample.map(|s| s.ram_permille));
    if c.memory_size {
        let text = sample
            .map(|s| {
                let a = s.used_mib * 10 / 1024;
                let b = s.total_mib * 10 / 1024;
                format!("{}.{}/{}.{}GIB", a / 10, a % 10, b / 10, b % 10)
            })
            .unwrap_or_else(|| "NO DATA".into());
        p.text(
            &text,
            u32::from(c.ram_position[0]),
            u32::from(c.ram_position[1])
                + 7 * u32::from(c.label_scale)
                + u32::from(c.bar_height)
                + 6,
            1,
            color(c.foreground),
        );
    }
    if c.clock {
        p.text(
            &format!(
                "{:02}:{:02}",
                if hours <= 23 { hours } else { 0 },
                if minutes <= 59 { minutes } else { 0 }
            ),
            u32::from(c.clock_position[0]),
            u32::from(c.clock_position[1]),
            u32::from(c.clock_scale),
            color(c.foreground),
        );
    }
    Ok(p.0)
}
/// Render the same functional page selected by the MCU scheduler.
pub fn render_page(
    c: &Config,
    pages: &crate::dock_pages::Playlist,
    kind: crate::dock_pages::Kind,
    sample: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
) -> Result<RgbaImage> {
    render_page_with_timer(c, pages, kind, sample, now, None)
}
pub fn render_page_with_timer(
    c: &Config,
    pages: &crate::dock_pages::Playlist,
    kind: crate::dock_pages::Kind,
    sample: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
    timer: Option<&crate::dock_timer::Status>,
) -> Result<RgbaImage> {
    use crate::dock_pages::Kind;
    use chrono::{Datelike, Timelike};
    c.validate()?;
    pages.validate()?;
    if kind == Kind::Custom {
        return Ok(composition::render(
            c,
            &pages.pages[usize::from(pages.selected)].composition,
            sample,
            now,
        ));
    }
    if kind as u8 >= 9 {
        if let Some(s) = sample {
            s.encode_extended()?;
        }
        return Ok(metrics::render(c, kind, sample));
    }
    if let Some(s) = sample {
        s.encode()?;
    }
    if c.style != 0 && matches!(kind, Kind::Cpu | Kind::Memory | Kind::Clock) {
        return Ok(modern::detail(c, pages, kind, sample, now));
    }
    if kind == Kind::Summary && c.style == 3 {
        return render_game(c, sample);
    }
    if kind == Kind::Summary && c.style == 0 {
        return render(c, sample, now.hour() as u8, now.minute() as u8);
    }
    if kind as u8 >= 5 || kind == Kind::Summary {
        return extra::render(c, pages, kind, sample, now, timer);
    }
    let mut p = Painter(RgbaImage::from_pixel(128, 128, color(c.background)));
    let ink = color(c.foreground);
    let accent = color(c.accent);
    if c.border {
        p.rect(0, 0, 128, 1, accent);
        p.rect(0, 127, 128, 1, accent);
    }
    if c.show_title {
        p.text(
            match kind {
                Kind::Cpu => "CPU LOAD",
                Kind::Memory => "MEMORY",
                Kind::Clock => "TIME",
                _ => &c.title,
            },
            u32::from(c.title_position[0]),
            u32::from(c.title_position[1]),
            1,
            accent,
        );
    }
    if kind == Kind::Media {
        p.text("EMPTY SLOT", 6, 56, 1, ink);
        return Ok(p.0);
    }
    if kind == Kind::Clock {
        let x = u32::from(pages.clock_position[0]);
        let y = u32::from(pages.clock_position[1]);
        let scale = u32::from(pages.clock_scale);
        p.text(
            &format!("{:02}:{:02}", now.hour(), now.minute()),
            x,
            y,
            scale,
            ink,
        );
        if pages.show_date {
            p.text(
                &format!("{:04}.{:02}.{:02}", now.year(), now.month(), now.day()),
                x,
                y + 7 * scale + 12,
                1,
                accent,
            );
        }
        if pages.show_seconds {
            p.text(
                &format!("SEC {:02}", now.second()),
                x,
                y + 7 * scale + 26,
                1,
                accent,
            );
        }
    } else {
        let index = kind == Kind::Memory;
        let at = if index {
            pages.memory_position
        } else {
            pages.cpu_position
        };
        let x = u32::from(at[0]);
        let y = u32::from(at[1]);
        let scale = u32::from(pages.detail_scale);
        let value = sample.map(|s| {
            if index {
                s.ram_permille
            } else {
                s.cpu_permille
            }
        });
        p.text(
            &value
                .map(|v| format!("{}%", v / 10))
                .unwrap_or_else(|| "--".into()),
            x,
            y,
            scale,
            accent,
        );
        if c.bars {
            let by = y + 7 * scale + 6;
            let w = u32::from(c.bar_width);
            let h = u32::from(c.bar_height);
            p.rect(x, by, w, h, ink);
            p.rect(x + 1, by + 1, w - 2, h - 2, color(c.background));
            if let Some(v) = value {
                p.rect(x + 1, by + 1, (w - 2) * u32::from(v) / 1000, h - 2, accent);
            }
        }
        if index && c.memory_size {
            let text = sample
                .map(|s| {
                    let a = s.used_mib * 10 / 1024;
                    let b = s.total_mib * 10 / 1024;
                    format!("{}.{}/{}.{}GIB", a / 10, a % 10, b / 10, b % 10)
                })
                .unwrap_or_else(|| "NO DATA".into());
            p.text(
                &text,
                x,
                y + 7 * scale + u32::from(c.bar_height) + 12,
                1,
                ink,
            );
        }
    }
    Ok(p.0)
}
fn render_game(c: &Config, sample: Option<&Sample>) -> Result<RgbaImage> {
    if let Some(s) = sample {
        s.encode_graphics()?;
    }
    let mut p = Painter(RgbaImage::from_pixel(128, 128, color(c.background)));
    let ink = color(c.foreground);
    let accent = color(c.accent);
    if c.border {
        p.rect(0, 0, 128, 1, accent);
        p.rect(0, 127, 128, 1, accent);
    }
    let g = sample.map(|s| &s.graphics);
    p.modern_percent(c, 8, 5, "CPU", sample.map(|s| s.cpu_permille));
    p.modern_percent(c, 70, 5, "RAM", sample.map(|s| s.ram_permille));
    p.modern_percent(c, 8, 57, "GPU", g.and_then(|g| g.gpu_permille));
    let vram = g.and_then(|g| Some((g.vram_used_mib?, g.vram_total_mib?)));
    p.modern_percent(
        c,
        70,
        57,
        "VRAM",
        vram.map(|(a, b)| ((u64::from(a) * 1000) / u64::from(b)) as u16),
    );
    let fps = g
        .and_then(|g| g.fps_tenths)
        .map(|v| ((u32::from(v) + 5) / 10).to_string())
        .unwrap_or_else(|| "--".into());
    p.modern("FPS", 8, 112, 0, accent);
    if c.memory_size {
        p.modern(&fps, 39, 112, 0, ink);
        if let Some((used, _)) = vram {
            let tenth = used * 10 / 1024;
            let label = if tenth < 100 {
                format!("{}.{}G", tenth / 10, tenth % 10)
            } else {
                format!("{}G", tenth / 10)
            };
            if Painter::modern_width(&label, 0) + 39 + Painter::modern_width(&fps, 0) + 8 <= 120 {
                p.modern(
                    &label,
                    120u32.saturating_sub(Painter::modern_width(&label, 0)),
                    112,
                    0,
                    ink,
                );
            }
        }
    } else {
        p.modern(
            &fps,
            120u32.saturating_sub(Painter::modern_width(&fps, 1)),
            104,
            1,
            ink,
        );
    }
    Ok(p.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipping_and_telemetry_validation() {
        let c = Config {
            cpu_position: [127, 127],
            ram_position: [127, 127],
            clock_position: [127, 127],
            title_position: [127, 127],
            label_scale: 2,
            clock_scale: 3,
            ..Default::default()
        };
        assert_eq!(render(&c, None, 12, 34).unwrap().dimensions(), (128, 128));
        assert!(
            render(
                &c,
                Some(&Sample {
                    cpu_permille: 1001,
                    ram_permille: 0,
                    used_mib: 0,
                    total_mib: 1,
                    sequence: 0,
                    graphics: Default::default(),
                }),
                12,
                34
            )
            .is_err()
        );
    }
}

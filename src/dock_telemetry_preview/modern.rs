//! Pixel-exact copy of the MCU's two-bit coverage compositor.
use super::*;
#[derive(serde::Deserialize)]
struct Glyph {
    offset: usize,
    width: u32,
    advance: u32,
}
#[derive(serde::Deserialize)]
struct Font {
    chars: String,
    height: u32,
    glyphs: Vec<Glyph>,
    data: Vec<u8>,
}
#[derive(serde::Deserialize)]
struct Sheet {
    fonts: Vec<Font>,
}
fn fonts() -> &'static [Font] {
    static FONT: std::sync::OnceLock<Sheet> = std::sync::OnceLock::new();
    &FONT
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../firmware/aj159-screen/modern-font.json"))
                .expect("Embedded Inter subset")
        })
        .fonts
}
fn packed(c: Rgba<u8>) -> u16 {
    ((u16::from(c[0] >> 3)) << 11) | ((u16::from(c[1] >> 2)) << 5) | u16::from(c[2] >> 3)
}
fn unpack(c: u16) -> Rgba<u8> {
    Rgba([
        ((c >> 11) * 255 / 31) as u8,
        (((c >> 5) & 63) * 255 / 63) as u8,
        ((c & 31) * 255 / 31) as u8,
        255,
    ])
}
fn cover(ink: u16, old: u16, a: u16) -> u16 {
    if a == 3 {
        return ink;
    }
    let channel = |fg: u16, bg: u16| ((fg * a + bg * (3 - a) + 1) * 171) >> 9;
    (channel(ink >> 11, old >> 11) << 11)
        | (channel((ink >> 5) & 63, (old >> 5) & 63) << 5)
        | channel(ink & 31, old & 31)
}
pub(super) fn cover_color(ink: Rgba<u8>, old: Rgba<u8>, a: u16) -> Rgba<u8> {
    unpack(cover(packed(ink), packed(old), a))
}
impl Painter {
    pub(super) fn modern_width(s: &str, font: usize) -> u32 {
        let f = &fonts()[font];
        s.bytes()
            .filter_map(|ch| {
                f.chars
                    .bytes()
                    .position(|v| v == ch)
                    .map(|i| f.glyphs[i].advance)
            })
            .sum()
    }
    pub(super) fn modern(&mut self, s: &str, mut x: u32, y: u32, font: usize, ink: Rgba<u8>) {
        let f = &fonts()[font];
        let ink = packed(ink);
        for ch in s.bytes() {
            let Some(index) = f.chars.bytes().position(|v| v == ch) else {
                continue;
            };
            let g = &f.glyphs[index];
            for row in 0..f.height {
                for col in 0..g.width {
                    if x + col >= 128 || y + row >= 128 {
                        continue;
                    }
                    let bit = (row * g.width + col) as usize;
                    let a = u16::from((f.data[g.offset + bit / 4] >> (6 - (bit % 4) * 2)) & 3);
                    if a == 0 {
                        continue;
                    }
                    let old = packed(*self.0.get_pixel(x + col, y + row));
                    let c = cover(ink, old, a);
                    self.0.put_pixel(x + col, y + row, unpack(c));
                }
            }
            x += g.advance;
        }
    }
    pub(super) fn modern_center(&mut self, s: &str, y: u32, font: usize, ink: Rgba<u8>) {
        let w = Self::modern_width(s, font);
        self.modern(s, 128u32.saturating_sub(w) / 2, y, font, ink);
    }
    pub(super) fn pill(&mut self, x: u32, y: u32, w: u32, h: u32, c: Rgba<u8>) {
        for dy in 0..h {
            let inset = if dy == 0 || dy + 1 == h {
                2
            } else if dy == 1 || dy + 2 == h {
                1
            } else {
                0
            };
            if w > inset * 2 {
                self.rect(x + inset, y + dy, w - inset * 2, 1, c);
            }
        }
    }
    // Keep the same coordinates, dimensions and colors as the MCU row compositor.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn modern_bar(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        value: Option<u16>,
        ink: Rgba<u8>,
        background: Rgba<u8>,
    ) {
        let track = cover(packed(ink), packed(background), 1);
        self.pill(x, y, w, h, unpack(track));
        if let Some(v) = value {
            let fill = w * u32::from(v) / 1000;
            if fill > 0 {
                self.pill(x, y, fill, h, ink);
            }
        }
    }
    pub(super) fn modern_percent(
        &mut self,
        c: &Config,
        x: u32,
        y: u32,
        label: &str,
        value: Option<u16>,
    ) {
        let ink = color(c.foreground);
        let accent = color(c.accent);
        self.modern(label, x, y, 0, accent);
        if c.percentages {
            self.modern("%", x + 44, y, 0, accent);
            self.modern(
                &value
                    .map(|v| (v / 10).to_string())
                    .unwrap_or_else(|| "--".into()),
                x,
                y + 18,
                1,
                ink,
            );
        }
        if c.bars {
            self.modern_bar(
                x,
                y + if c.percentages { 41 } else { 21 },
                50,
                if c.percentages { 4 } else { 14 },
                value,
                accent,
                color(c.background),
            );
        }
    }
}

pub(super) fn detail(
    c: &Config,
    pages: &crate::dock_pages::Playlist,
    kind: crate::dock_pages::Kind,
    sample: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
) -> RgbaImage {
    use crate::dock_pages::Kind;
    use chrono::{Datelike, Timelike};
    let mut p = Painter(RgbaImage::from_pixel(128, 128, color(c.background)));
    let ink = color(c.foreground);
    let accent = color(c.accent);
    if c.border {
        p.rect(0, 0, 128, 1, accent);
        p.rect(0, 127, 128, 1, accent);
    }
    if kind == Kind::Clock {
        if pages.show_date {
            p.modern_center(
                &format!("{}.{}.{}", now.day(), now.month(), now.year()),
                9,
                0,
                accent,
            );
        }
        p.modern_center(
            &now.format("%H:%M").to_string(),
            43,
            if pages.clock_scale > 1 { 2 } else { 1 },
            ink,
        );
        if pages.show_seconds {
            p.modern_center(&format!(":{:02}", now.second()), 99, 0, accent);
        }
    } else {
        let value = sample.map(|s| {
            if kind == Kind::Cpu {
                s.cpu_permille
            } else {
                s.ram_permille
            }
        });
        p.modern_center(if kind == Kind::Cpu { "CPU" } else { "RAM" }, 9, 0, accent);
        if c.percentages {
            p.modern_center(
                &value
                    .map(|v| (v / 10).to_string())
                    .unwrap_or_else(|| "--".into()),
                36,
                if pages.detail_scale > 1 { 2 } else { 1 },
                ink,
            );
            p.modern("%", 109, 62, 0, accent);
        }
        if c.bars {
            p.modern_bar(8, 89, 112, 7, value, accent, color(c.background));
        }
        if kind == Kind::Memory && c.memory_size {
            let mut label = sample
                .map(|s| {
                    let a = s.used_mib * 10 / 1024;
                    let b = s.total_mib * 10 / 1024;
                    format!("{}.{}/{}.{}GIB", a / 10, a % 10, b / 10, b % 10)
                })
                .unwrap_or_else(|| "NO DATA".into());
            if Painter::modern_width(&label, 0) > 112 {
                label = sample
                    .map(|s| format!("{}G", s.used_mib / 1024))
                    .unwrap_or_else(|| "NO DATA".into());
            }
            p.modern_center(&label, 111, 0, accent);
        } else if c.clock {
            p.modern_center(&now.format("%H:%M").to_string(), 111, 0, accent);
        }
    }
    p.0
}

pub(super) fn summary(
    c: &Config,
    sample: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
) -> RgbaImage {
    let mut p = Painter(RgbaImage::from_pixel(128, 128, color(c.background)));
    let ink = color(c.foreground);
    let accent = color(c.accent);
    if c.border {
        p.rect(0, 0, 128, 1, accent);
        p.rect(0, 127, 128, 1, accent);
    }
    if c.style == 1 {
        if c.clock {
            p.modern_center(&now.format("%H:%M").to_string(), 5, 2, ink);
        }
        p.modern_percent(c, 8, 48, "CPU", sample.map(|s| s.cpu_permille));
        p.modern_percent(c, 70, 48, "RAM", sample.map(|s| s.ram_permille));
        if c.memory_size {
            let label = sample
                .map(|s| {
                    let a = s.used_mib * 10 / 1024;
                    let b = s.total_mib * 10 / 1024;
                    format!("{}.{}/{}.{}GIB", a / 10, a % 10, b / 10, b % 10)
                })
                .unwrap_or_else(|| "NO DATA".into());
            if Painter::modern_width(&label, 0) <= 112 {
                p.modern_center(&label, 111, 0, accent);
            }
        }
    } else {
        for i in 0..2 {
            let y = if i == 0 { 5 } else { 67 };
            let value = sample.map(|s| {
                if i == 0 {
                    s.cpu_permille
                } else {
                    s.ram_permille
                }
            });
            p.modern(if i == 0 { "CPU" } else { "RAM" }, 8, y, 0, accent);
            if c.percentages {
                p.modern("%", 51, y, 0, accent);
                p.modern(
                    &value
                        .map(|v| (v / 10).to_string())
                        .unwrap_or_else(|| "--".into()),
                    8,
                    y + 18,
                    if i == 0 { 2 } else { 1 },
                    ink,
                );
            }
            if c.bars {
                p.modern_bar(
                    8,
                    y + if !c.percentages {
                        22
                    } else if i == 0 {
                        48
                    } else {
                        39
                    },
                    112,
                    if c.percentages { 3 } else { 12 },
                    value,
                    accent,
                    color(c.background),
                );
            }
        }
        if c.clock {
            let label = now.format("%H:%M").to_string();
            p.modern(
                &label,
                120 - Painter::modern_width(&label, 0),
                111,
                0,
                accent,
            );
        }
    }
    p.0
}

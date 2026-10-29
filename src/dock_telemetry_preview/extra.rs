use super::*;
use crate::{
    dock_pages::{Kind, Playlist},
    dock_timer,
};
use chrono::{Datelike, Timelike};
const SINE: [i32; 60] = [
    0, 10, 21, 31, 41, 50, 59, 67, 74, 81, 87, 91, 95, 98, 99, 100, 99, 98, 95, 91, 87, 81, 74, 67,
    59, 50, 41, 31, 21, 10, 0, -10, -21, -31, -41, -50, -59, -67, -74, -81, -87, -91, -95, -98,
    -99, -100, -99, -98, -95, -91, -87, -81, -74, -67, -59, -50, -41, -31, -21, -10,
];
fn line(p: &mut Painter, mut x: i32, mut y: i32, tx: i32, ty: i32, c: Rgba<u8>) {
    let dx = (tx - x).abs();
    let sx = if x < tx { 1 } else { -1 };
    let dy = -(ty - y).abs();
    let sy = if y < ty { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        p.rect(x as u32, y as u32, 1, 1, c);
        if x == tx && y == ty {
            break;
        }
        let e = 2 * err;
        if e >= dy {
            err += dy;
            x += sx;
        }
        if e <= dx {
            err += dx;
            y += sy;
        }
    }
}
fn hand(p: &mut Painter, index: usize, r: i32, c: Rgba<u8>, thick: bool) {
    let x = 64 + SINE[index] * r / 100;
    let y = 72 - SINE[(index + 15) % 60] * r / 100;
    line(p, 64, 72, x, y, c);
    if thick {
        line(p, 65, 72, x + 1, y, c);
    }
}
pub(super) fn render(
    c: &Config,
    pages: &Playlist,
    kind: Kind,
    sample: Option<&Sample>,
    now: chrono::DateTime<chrono::Local>,
    timer: Option<&dock_timer::Status>,
) -> Result<RgbaImage> {
    let ink = color(c.foreground);
    let accent = color(c.accent);
    let background = color(c.background);
    let mut p = Painter(RgbaImage::from_pixel(128, 128, background));
    if c.border {
        p.rect(0, 0, 128, 1, accent);
        p.rect(0, 127, 128, 1, accent);
    }
    match kind {
        Kind::Analog => {
            p.modern_center(&now.format("%H:%M").to_string(), 8, 0, accent);
            let (mut x, mut y, mut err) = (43, 0, -42);
            while x >= y {
                for (px, py) in [
                    (64 + x, 72 + y),
                    (64 + y, 72 + x),
                    (64 - y, 72 + x),
                    (64 - x, 72 + y),
                    (64 - x, 72 - y),
                    (64 - y, 72 - x),
                    (64 + y, 72 - x),
                    (64 + x, 72 - y),
                ] {
                    p.rect(px as u32, py as u32, 1, 1, ink);
                }
                y += 1;
                err += 2 * y - 1;
                if err >= 0 {
                    x -= 1;
                    err -= 2 * x;
                }
            }
            for i in (0..60).step_by(5) {
                let dx = SINE[i];
                let dy = SINE[(i + 15) % 60];
                line(
                    &mut p,
                    64 + dx * 38 / 100,
                    72 - dy * 38 / 100,
                    64 + dx * 41 / 100,
                    72 - dy * 41 / 100,
                    accent,
                );
            }
            p.modern("12", 56, 35, 3, ink);
            p.modern("3", 96, 68, 3, ink);
            p.modern("6", 60, 100, 3, ink);
            p.modern("9", 24, 68, 3, ink);
            hand(
                &mut p,
                (now.hour() % 12 * 5 + now.minute() / 12) as usize,
                23,
                ink,
                true,
            );
            hand(&mut p, now.minute() as usize, 33, ink, true);
            if pages.show_seconds {
                hand(&mut p, now.second() as usize, 37, accent, false);
            }
            p.rect(62, 70, 5, 5, accent);
        }
        Kind::Calendar => {
            let months = [
                "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
            ];
            p.modern(months[now.month0() as usize], 7, 8, 0, accent);
            p.modern(&now.year().to_string(), 76, 8, 0, ink);
            for (i, label) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                p.modern(label, 7 + i as u32 * 17, 29, 0, ink);
            }
            let first = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
            let w = first.weekday().num_days_from_monday();
            for day in 1..=31 {
                if chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), day).is_none() {
                    break;
                }
                let cell = w + day - 1;
                let (x, y) = (7 + (cell % 7) * 17, 46 + (cell / 7) * 14);
                if day == now.day() {
                    p.pill(x - 1, y - 1, 16, 12, accent);
                }
                p.modern(
                    &day.to_string(),
                    x + 14u32.saturating_sub(Painter::modern_width(&day.to_string(), 3)) / 2,
                    y,
                    3,
                    if day == now.day() { background } else { ink },
                );
            }
        }
        Kind::Focus | Kind::Stopwatch => {
            let mode = if kind == Kind::Focus { 1 } else { 2 };
            let timer = timer.filter(|t| t.mode == mode);
            let duration = timer.map_or(u32::from(c.focus_minutes) * 60, |t| t.duration);
            let elapsed = timer.map_or(0, |t| t.elapsed);
            let seconds = if mode == 1 {
                duration.saturating_sub(elapsed)
            } else {
                elapsed
            }
            .min(599999);
            let text = format!("{:02}:{:02}", seconds / 60, seconds % 60);
            let font = if Painter::modern_width(&text, 2) <= 120 {
                2
            } else {
                1
            };
            if c.show_title {
                p.modern_center(if mode == 1 { "FOCUS" } else { "STOPWATCH" }, 9, 0, accent);
            }
            p.modern_center(&text, 43, font, ink);
            let state = match timer {
                None => "READY",
                Some(t) if t.running => "RUNNING",
                Some(_) if mode == 1 && seconds == 0 => "DONE",
                _ => "PAUSED",
            };
            p.modern_center(state, 88, 0, accent);
            if mode == 1 && duration > 0 {
                p.modern_bar(
                    8,
                    112,
                    112,
                    6,
                    Some((1000 * seconds / duration) as u16),
                    accent,
                    background,
                );
            }
        }
        Kind::Summary => {
            return Ok(super::modern::summary(c, sample, now));
        }
        _ => unreachable!("Extra renderer only accepts its screen types"),
    }
    Ok(p.0)
}

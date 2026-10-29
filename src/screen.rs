use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local};
use image::{AnimationDecoder, Frame, ImageReader, Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use std::{io::BufReader, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenDesign {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub accent: [u8; 3],
    pub title: String,
    pub clock_x: f32,
    pub clock_y: f32,
    pub clock_size: f32,
    pub show_date: bool,
    pub show_seconds: bool,
    pub show_clock: bool,
    #[serde(default = "default_accent_line")]
    pub show_accent_line: bool,
    pub image_opacity: f32,
}
fn default_accent_line() -> bool {
    true
}
impl Default for ScreenDesign {
    fn default() -> Self {
        Self {
            background: [13, 20, 31],
            foreground: [237, 244, 248],
            accent: [48, 220, 190],
            title: "AJAZZ / STUDIO".into(),
            clock_x: 10.0,
            clock_y: 42.0,
            clock_size: 36.0,
            show_date: true,
            show_seconds: false,
            show_clock: true,
            show_accent_line: true,
            image_opacity: 0.75,
        }
    }
}

impl ScreenDesign {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.title.len() <= 1024, "Слишком длинный заголовок");
        ensure!(
            [
                self.clock_x,
                self.clock_y,
                self.clock_size,
                self.image_opacity
            ]
            .iter()
            .all(|n| n.is_finite()),
            "Некорректные координаты макета"
        );
        ensure!(
            (0.0..=127.0).contains(&self.clock_x)
                && (0.0..=127.0).contains(&self.clock_y)
                && (10.0..=64.0).contains(&self.clock_size)
                && (0.0..=1.0).contains(&self.image_opacity),
            "Параметры макета вне диапазона"
        );
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let design: Self = crate::storage::read_json(path, 65536)?;
        design.validate()?;
        Ok(design)
    }
}

pub struct Media {
    pub frames: Vec<RgbaImage>,
    pub delays: Vec<u32>,
}
impl Media {
    pub fn load(path: &Path) -> Result<Self> {
        ensure!(
            path.metadata()?.len() <= 32_000_000,
            "Изображение больше 32 МБ"
        );
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
        {
            let mut decoder =
                image::codecs::gif::GifDecoder::new(BufReader::new(std::fs::File::open(path)?))?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(2048);
            limits.max_image_height = Some(2048);
            limits.max_alloc = Some(64 * 1024 * 1024);
            image::ImageDecoder::set_limits(&mut decoder, limits)?;
            let mut frames = Vec::new();
            let mut delays = Vec::new();
            for frame in decoder.into_frames() {
                ensure!(frames.len() < 222, "В GIF больше 222 кадров");
                let frame = frame?;
                let (n, d) = frame.delay().numer_denom_ms();
                delays.push((n / d.max(1)).clamp(20, 10_000));
                frames.push(imageops::resize(
                    frame.buffer(),
                    128,
                    128,
                    imageops::FilterType::Lanczos3,
                ));
            }
            ensure!(!frames.is_empty(), "Пустой GIF");
            Ok(Self { frames, delays })
        } else {
            let mut reader = ImageReader::open(path)?.with_guessed_format()?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(64 * 1024 * 1024);
            reader.limits(limits);
            let img = reader
                .decode()?
                .resize_to_fill(128, 128, imageops::FilterType::Lanczos3)
                .to_rgba8();
            Ok(Self {
                frames: vec![img],
                delays: vec![1000],
            })
        }
    }
    pub fn at(&self, millis: u64) -> &RgbaImage {
        let total: u64 = self.delays.iter().map(|&x| u64::from(x)).sum();
        let mut t = millis % total.max(1);
        for (i, &d) in self.delays.iter().enumerate() {
            if t < u64::from(d) {
                return &self.frames[i];
            }
            t -= u64::from(d);
        }
        &self.frames[0]
    }
    /// The dock stores one common delay for the complete animation.
    pub fn at_fixed_delay(&self, millis: u64, delay_ms: u8) -> &RgbaImage {
        &self.frames[(millis / u64::from(delay_ms.max(1))) as usize % self.frames.len()]
    }
}

pub fn load_system_font() -> Option<ab_glyph::FontArc> {
    let base = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "C:/Windows".into());
    [
        base.join("Fonts/segoeui.ttf"),
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
    ]
    .iter()
    .find_map(|p| std::fs::read(p).ok())
    .and_then(|bytes| ab_glyph::FontArc::try_from_vec(bytes).ok())
}
fn text(
    img: &mut RgbaImage,
    font: &ab_glyph::FontArc,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 3],
) {
    use ab_glyph::{Font, ScaleFont};
    let scaled = font.as_scaled(size);
    let mut cursor = x;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        let glyph = id.with_scale_and_position(size, ab_glyph::point(cursor, y + scaled.ascent()));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if (0..128).contains(&px) && (0..128).contains(&py) {
                    let dest = img.get_pixel_mut(px as u32, py as u32);
                    for i in 0..3 {
                        dest[i] = (f32::from(dest[i]) * (1.0 - coverage)
                            + f32::from(color[i]) * coverage)
                            as u8;
                    }
                }
            });
        }
        cursor += scaled.h_advance(id);
    }
}
pub fn render(
    design: &ScreenDesign,
    now: DateTime<Local>,
    media: Option<&RgbaImage>,
    font: Option<&ab_glyph::FontArc>,
) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(
        128,
        128,
        Rgba([
            design.background[0],
            design.background[1],
            design.background[2],
            255,
        ]),
    );
    if let Some(media) = media {
        for (x, y, pixel) in media
            .enumerate_pixels()
            .filter(|(x, y, _)| *x < 128 && *y < 128)
        {
            let dest = img.get_pixel_mut(x, y);
            let alpha = design.image_opacity.clamp(0.0, 1.0) * f32::from(pixel[3]) / 255.0;
            for i in 0..3 {
                dest[i] = (f32::from(dest[i]) * (1.0 - alpha) + f32::from(pixel[i]) * alpha) as u8;
            }
        }
    }
    if let Some(font) = font {
        text(
            &mut img,
            font,
            &design.title,
            10.0,
            12.0,
            11.0,
            design.accent,
        );
        if design.show_clock {
            let time = now
                .format(if design.show_seconds {
                    "%H:%M:%S"
                } else {
                    "%H:%M"
                })
                .to_string();
            text(
                &mut img,
                font,
                &time,
                design.clock_x,
                design.clock_y,
                design.clock_size,
                design.foreground,
            );
        }
        if design.show_date {
            text(
                &mut img,
                font,
                &now.format("%d / %m / %Y").to_string(),
                11.0,
                91.0,
                13.0,
                design.accent,
            );
        }
    }
    if design.show_accent_line {
        for x in 11..117 {
            img.put_pixel(
                x,
                113,
                Rgba([design.accent[0], design.accent[1], design.accent[2], 255]),
            );
        }
    }
    img
}

/// Standard RGB565 big endian, an interchange format, not an asserted dock upload encoding.
pub fn rgb565(img: &RgbaImage) -> Vec<u8> {
    img.pixels()
        .flat_map(|c| {
            let v =
                (u16::from(c[0]) >> 3) << 11 | (u16::from(c[1]) >> 2) << 5 | u16::from(c[2]) >> 3;
            v.to_be_bytes()
        })
        .collect()
}

pub fn export_png(design: &ScreenDesign, path: &Path, media: Option<&Media>) -> Result<()> {
    design.validate()?;
    let font = load_system_font();
    render(design, Local::now(), media.map(|m| m.at(0)), font.as_ref())
        .save(path)
        .context("Не удалось сохранить PNG")
}

pub fn export_lighting_gif(scene: &crate::model::Scene, path: &Path) -> Result<()> {
    scene.validate()?;
    ensure!(
        scene.duration * scene.fps as f32 <= 900.0,
        "Для GIF выберите не более 900 кадров"
    );
    let mut encoder =
        image::codecs::gif::GifEncoder::new_with_speed(std::fs::File::create(path)?, 10);
    encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
    let rows = crate::model::keyboard_rows();
    for n in 0..(scene.duration * scene.fps as f32).ceil() as u32 {
        let colors = crate::lighting::matrix(scene, n as f32 / scene.fps as f32);
        let mut img = RgbaImage::from_pixel(660, 256, Rgba([14, 18, 26, 255]));
        for (row, keys) in rows.iter().enumerate() {
            let mut x = 4;
            for key in keys {
                let width = (key.width * 38.0) as u32;
                let c = colors[usize::from(key.id)];
                for py in row as u32 * 42 + 4..row as u32 * 42 + 40 {
                    for px in x..(x + width - 3).min(660) {
                        img.put_pixel(px, py, Rgba([c[0], c[1], c[2], 255]));
                    }
                }
                x += width;
            }
        }
        encoder.encode_frame(Frame::from_parts(
            img,
            0,
            0,
            image::Delay::from_numer_denom_ms(1000, scene.fps),
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_finite_screen_coordinates() {
        let s = ScreenDesign {
            clock_x: f32::NAN,
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }
    #[test]
    fn saved_design_round_trips() {
        let a = ScreenDesign::default();
        let encoded = serde_json::to_vec(&a).unwrap();
        let b: ScreenDesign = serde_json::from_slice(&encoded).unwrap();
        b.validate().unwrap();
        assert_eq!(a.title, b.title);
        assert_eq!(a.accent, b.accent);
    }
    #[test]
    fn decoration_can_be_disabled_without_covering_image() {
        let mut design = ScreenDesign::default();
        let mut legacy = serde_json::to_value(&design).unwrap();
        legacy.as_object_mut().unwrap().remove("show_accent_line");
        assert!(
            serde_json::from_value::<ScreenDesign>(legacy)
                .unwrap()
                .show_accent_line
        );
        let media = RgbaImage::from_pixel(128, 128, Rgba([255, 0, 0, 255]));
        design.image_opacity = 1.0;
        design.show_accent_line = false;
        assert_eq!(render(&design, Local::now(), Some(&media), None), media);
        design.show_accent_line = true;
        assert_eq!(
            render(&design, Local::now(), Some(&media), None)
                .get_pixel(11, 113)
                .0[..3],
            design.accent
        );
    }
    #[test]
    fn primary_rgb565_bytes() {
        let mut img = RgbaImage::new(3, 1);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        img.put_pixel(2, 0, Rgba([0, 0, 255, 255]));
        assert_eq!(rgb565(&img), vec![0xf8, 0, 0x07, 0xe0, 0, 0x1f]);
    }
}

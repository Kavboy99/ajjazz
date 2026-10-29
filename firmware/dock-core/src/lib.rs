//! Portable on-device scene runtime. This library is not a flashable dock image.
//! Hardware transport, flash layout, panel orientation and LED wiring belong to
//! a verified board port. No host clock or continuous USB frame stream is used.
#![no_std]

pub mod config;
pub mod presets;

pub const MAX_LEDS: usize = 64;
pub const MAX_LAYERS: usize = 4;
pub const MAX_PAGES: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub const BLACK: Self = Self(0, 0, 0);

    pub fn scale(self, level: u8) -> Self {
        let scale = |v: u8| ((u16::from(v) * u16::from(level) + 127) / 255) as u8;
        Self(scale(self.0), scale(self.1), scale(self.2))
    }

    pub fn mix(self, other: Self, amount: u8) -> Self {
        let mix = |a: u8, b: u8| {
            ((u32::from(a) * u32::from(255 - amount) + u32::from(b) * u32::from(amount) + 127)
                / 255) as u8
        };
        Self(
            mix(self.0, other.0),
            mix(self.1, other.1),
            mix(self.2, other.2),
        )
    }

    /// Logical RGB565 word; byte order and panel orientation are board concerns.
    pub fn rgb565(self) -> u16 {
        ((u16::from(self.0) >> 3) << 11)
            | ((u16::from(self.1) >> 2) << 5)
            | (u16::from(self.2) >> 3)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Palette,
    Period,
    Repeat,
    Layers,
    LedCount,
    Pages,
    Duration,
    Clock,
    Geometry,
    Scanline,
    Animation,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    colors: [Color; 4],
    len: u8,
}

impl Palette {
    pub fn new(colors: [Color; 4], len: u8) -> Result<Self, ConfigError> {
        if !(1..=4).contains(&len) {
            return Err(ConfigError::Palette);
        }
        Ok(Self { colors, len })
    }

    pub const fn solid(color: Color) -> Self {
        Self {
            colors: [color; 4],
            len: 1,
        }
    }

    pub fn at(self, phase: u16) -> Color {
        if self.len == 1 {
            return self.colors[0];
        }
        let position = u32::from(phase) * u32::from(self.len);
        let index = (position >> 16) as usize;
        self.colors[index].mix(
            self.colors[(index + 1) % usize::from(self.len)],
            ((position & 0xffff) >> 8) as u8,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Effect {
    Solid,
    Wave,
    Breathe,
    Comet,
    Sparkle,
}

#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub effect: Effect,
    pub palette: Palette,
    period_ms: u32,
    repeats: u8,
    pub reverse: bool,
    pub opacity: u8,
    pub mask: u64,
}

impl Layer {
    pub fn new(
        effect: Effect,
        palette: Palette,
        period_ms: u32,
        repeats: u8,
    ) -> Result<Self, ConfigError> {
        if !(100..=60_000).contains(&period_ms) {
            return Err(ConfigError::Period);
        }
        if !(1..=16).contains(&repeats) {
            return Err(ConfigError::Repeat);
        }
        Ok(Self {
            effect,
            palette,
            period_ms,
            repeats,
            reverse: false,
            opacity: 255,
            mask: u64::MAX,
        })
    }

    fn phase(self, elapsed_ms: u64) -> u16 {
        let time =
            ((elapsed_ms % u64::from(self.period_ms)) * 65536 / u64::from(self.period_ms)) as u16;
        if self.reverse {
            time.wrapping_neg()
        } else {
            time
        }
    }

    fn color(self, led: usize, count: usize, time: u16) -> Color {
        let space = ((led as u32 * 65536 * u32::from(self.repeats)) / count as u32) as u16;
        let phase = time.wrapping_add(space);
        match self.effect {
            Effect::Solid => self.palette.at(0),
            Effect::Wave => self.palette.at(phase),
            Effect::Breathe => {
                let triangle = if time < 32768 {
                    u32::from(time)
                } else {
                    65535 - u32::from(time)
                };
                let level = (triangle * 255 / 32767) as u8;
                self.palette.at(0).scale(level)
            }
            Effect::Comet => {
                let distance = space.wrapping_sub(time);
                let level = if distance < 16384 {
                    255 - (u32::from(distance) * 255 / 16384) as u8
                } else {
                    0
                };
                self.palette.at(time).scale(level)
            }
            Effect::Sparkle => {
                // Stable per-LED phase offset, continuous envelope, no RNG state.
                let mut hash = (led as u32 + 1).wrapping_mul(0x9e37_79b9);
                hash ^= hash >> 16;
                let phase = time.wrapping_add(hash as u16);
                let level = if phase < 8192 {
                    (u32::from(phase) * 255 / 8192) as u8
                } else if phase < 16384 {
                    ((16384 - u32::from(phase)) * 255 / 8192) as u8
                } else {
                    0
                };
                self.palette.at(hash as u16).scale(level)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Scene {
    layers: [Layer; MAX_LAYERS],
    len: u8,
    pub brightness: u8,
}

impl Scene {
    pub fn new(layers: [Layer; MAX_LAYERS], len: u8, brightness: u8) -> Result<Self, ConfigError> {
        if !(1..=MAX_LAYERS as u8).contains(&len) {
            return Err(ConfigError::Layers);
        }
        Ok(Self {
            layers,
            len,
            brightness,
        })
    }

    pub fn render(&self, elapsed_ms: u64, leds: &mut [Color]) -> Result<(), ConfigError> {
        if leds.is_empty() || leds.len() > MAX_LEDS {
            return Err(ConfigError::LedCount);
        }
        let count = leds.len();
        let mut phases = [0; MAX_LAYERS];
        for (phase, layer) in phases
            .iter_mut()
            .zip(self.layers.iter())
            .take(usize::from(self.len))
        {
            *phase = layer.phase(elapsed_ms);
        }
        for (index, output) in leds.iter_mut().enumerate() {
            let mut color = Color::BLACK;
            for (layer, time) in self.layers[..usize::from(self.len)].iter().zip(phases) {
                if layer.mask & (1_u64 << index) != 0 {
                    color = color.mix(layer.color(index, count, time), layer.opacity);
                }
            }
            *output = color.scale(self.brightness);
        }
        Ok(())
    }
}

/// Extends a board's wrapping millisecond timer. Call at least once per wrap.
#[derive(Debug)]
pub struct Timeline {
    last_ms: u32,
    elapsed_ms: u64,
}

impl Timeline {
    pub const fn new(now_ms: u32) -> Self {
        Self {
            last_ms: now_ms,
            elapsed_ms: 0,
        }
    }

    pub fn advance(&mut self, now_ms: u32) -> u64 {
        self.elapsed_ms += u64::from(now_ms.wrapping_sub(self.last_ms));
        self.last_ms = now_ms;
        self.elapsed_ms
    }
}

/// Drops obsolete frames after a slow transfer; never sends a catch-up burst.
pub struct FramePacer {
    fps: u8,
    next_ms: u64,
}

impl FramePacer {
    pub fn new(fps: u8) -> Result<Self, ConfigError> {
        if !(1..=60).contains(&fps) {
            return Err(ConfigError::Period);
        }
        Ok(Self { fps, next_ms: 0 })
    }

    pub fn due(&mut self, elapsed_ms: u64) -> bool {
        if elapsed_ms < self.next_ms {
            return false;
        }
        // Rational deadlines avoid the 16ms = 62.5Hz rounding mistake.
        let frame = elapsed_ms * u64::from(self.fps) / 1000 + 1;
        self.next_ms = (frame * 1000).div_ceil(u64::from(self.fps));
        true
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TimeOfDay(u32);

impl TimeOfDay {
    pub fn new(hours: u8, minutes: u8, seconds: u8) -> Result<Self, ConfigError> {
        if hours > 23 || minutes > 59 || seconds > 59 {
            return Err(ConfigError::Clock);
        }
        Ok(Self(
            u32::from(hours) * 3600 + u32::from(minutes) * 60 + u32::from(seconds),
        ))
    }

    pub fn after(self, seconds: u64) -> Self {
        Self(((u64::from(self.0) + seconds % 86400) % 86400) as u32)
    }

    pub fn hms(self) -> (u8, u8, u8) {
        (
            (self.0 / 3600) as u8,
            (self.0 / 60 % 60) as u8,
            (self.0 % 60) as u8,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClockStyle {
    pub foreground: Color,
    pub background: Color,
    pub blink_colon: bool,
    scale: u8,
}

impl ClockStyle {
    pub fn new(foreground: Color, background: Color, scale: u8) -> Result<Self, ConfigError> {
        if !(1..=8).contains(&scale) {
            return Err(ConfigError::Geometry);
        }
        Ok(Self {
            foreground,
            background,
            blink_colon: false,
            scale,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Content {
    Clock(ClockStyle),
    Solid(Color),
    /// RGB565 asset read by the board from its validated asset storage.
    Image(u16),
    Animation(Clip),
}

#[derive(Clone, Copy, Debug)]
pub struct Clip {
    id: u16,
    frames: u16,
    frame_ms: u16,
}

impl Clip {
    pub fn new(id: u16, frames: u16, frame_ms: u16) -> Result<Self, ConfigError> {
        if frames == 0 || frames > 256 || !(16..=10_000).contains(&frame_ms) {
            return Err(ConfigError::Animation);
        }
        Ok(Self {
            id,
            frames,
            frame_ms,
        })
    }

    pub fn frame(self, elapsed_ms: u64) -> u16 {
        ((elapsed_ms / u64::from(self.frame_ms)) % u64::from(self.frames)) as u16
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    duration_ms: u32,
    pub content: Content,
}

impl Page {
    pub fn new(duration_ms: u32, content: Content) -> Result<Self, ConfigError> {
        if !(100..=3_600_000).contains(&duration_ms) {
            return Err(ConfigError::Duration);
        }
        Ok(Self {
            duration_ms,
            content,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Pages {
    pages: [Page; MAX_PAGES],
    len: u8,
}

pub struct PageView<'a> {
    pub page: &'a Page,
    pub elapsed_ms: u32,
}

impl Pages {
    pub fn new(pages: [Page; MAX_PAGES], len: u8) -> Result<Self, ConfigError> {
        if !(1..=MAX_PAGES as u8).contains(&len) {
            return Err(ConfigError::Pages);
        }
        Ok(Self { pages, len })
    }

    pub fn current(&self, elapsed_ms: u64) -> &Page {
        self.at(elapsed_ms).page
    }

    pub fn at(&self, elapsed_ms: u64) -> PageView<'_> {
        let active = &self.pages[..usize::from(self.len)];
        let total: u64 = active.iter().map(|p| u64::from(p.duration_ms)).sum();
        let mut phase = elapsed_ms % total;
        for page in active {
            if phase < u64::from(page.duration_ms) {
                return PageView {
                    page,
                    elapsed_ms: phase as u32,
                };
            }
            phase -= u64::from(page.duration_ms);
        }
        PageView {
            page: &active[0],
            elapsed_ms: 0,
        }
    }
}

pub trait Assets {
    type Error;
    /// Implementations must check ID, geometry, row and stored payload bounds.
    fn row(
        &mut self,
        id: u16,
        frame: u16,
        width: u16,
        height: u16,
        y: u16,
        output: &mut [u16],
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenderError<E> {
    Config(ConfigError),
    Asset(E),
}

const DIGITS: [[u8; 7]; 10] = [
    [14, 17, 19, 21, 25, 17, 14],
    [4, 12, 4, 4, 4, 4, 14],
    [14, 17, 1, 2, 4, 8, 31],
    [30, 1, 1, 14, 1, 1, 30],
    [2, 6, 10, 18, 31, 2, 2],
    [31, 16, 16, 30, 1, 1, 30],
    [14, 16, 16, 30, 17, 17, 14],
    [31, 1, 2, 4, 8, 8, 8],
    [14, 17, 17, 14, 17, 17, 14],
    [14, 17, 17, 15, 1, 1, 14],
];

/// Renders only one logical scanline, using caller-owned memory.
pub fn scanline<A: Assets>(
    content: Content,
    time: TimeOfDay,
    width: u16,
    height: u16,
    y: u16,
    output: &mut [u16],
    assets: &mut A,
) -> Result<(), RenderError<A::Error>> {
    render_row(
        content,
        RasterRow {
            time,
            width,
            height,
            y,
            elapsed_ms: 0,
        },
        output,
        assets,
    )
}

/// Pass one snapshot to every row so a screen refresh never mixes times/frames.
#[derive(Clone, Copy)]
pub struct RasterRow {
    pub time: TimeOfDay,
    pub width: u16,
    pub height: u16,
    pub y: u16,
    pub elapsed_ms: u64,
}

pub fn render_row<A: Assets>(
    content: Content,
    row: RasterRow,
    output: &mut [u16],
    assets: &mut A,
) -> Result<(), RenderError<A::Error>> {
    let RasterRow {
        time,
        width,
        height,
        y,
        elapsed_ms,
    } = row;
    if width == 0 || height == 0 || y >= height || output.len() != usize::from(width) {
        return Err(RenderError::Config(ConfigError::Scanline));
    }
    match content {
        Content::Image(id) => assets
            .row(id, 0, width, height, y, output)
            .map_err(RenderError::Asset),
        Content::Animation(clip) => assets
            .row(clip.id, clip.frame(elapsed_ms), width, height, y, output)
            .map_err(RenderError::Asset),
        Content::Solid(color) => {
            output.fill(color.rgb565());
            Ok(())
        }
        Content::Clock(style) => {
            let scale = u16::from(style.scale);
            let (text_width, text_height) = (29 * scale, 7 * scale);
            if width < text_width || height < text_height {
                return Err(RenderError::Config(ConfigError::Geometry));
            }
            output.fill(style.background.rgb565());
            let top = (height - text_height) / 2;
            if y < top || y >= top + text_height {
                return Ok(());
            }
            let row = usize::from((y - top) / scale);
            let left = (width - text_width) / 2;
            let (h, m, s) = time.hms();
            let digits = [h / 10, h % 10, 0, m / 10, m % 10];
            for (x, pixel) in output.iter_mut().enumerate() {
                let x = x as u16;
                if x < left || x >= left + text_width {
                    continue;
                }
                let gx = (x - left) / scale;
                let cell = usize::from(gx / 6);
                let column = gx % 6;
                if column == 5 {
                    continue;
                }
                let bits = if cell == 2 {
                    if (row == 2 || row == 4) && (!style.blink_colon || s % 2 == 0) {
                        4
                    } else {
                        0
                    }
                } else {
                    DIGITS[usize::from(digits[cell])][row]
                };
                if bits & (1 << (4 - column)) != 0 {
                    *pixel = style.foreground.rgb565();
                }
            }
            Ok(())
        }
    }
}

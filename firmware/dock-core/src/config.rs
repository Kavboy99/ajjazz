//! Versioned configuration shared by a future board port and desktop editor.
//! This is an offline format, not a command accepted by the stock dock.
use crate::*;

const HEADER: usize = 12;
const LAYER: usize = 29;
const PAGE: usize = 13;
pub const MAX_CONFIG_BYTES: usize = HEADER + 4 + MAX_LAYERS * LAYER + MAX_PAGES * PAGE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Length,
    Magic,
    Version,
    Checksum,
    Value,
    Config(ConfigError),
}

impl From<ConfigError> for DecodeError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

/// Contains logical configuration only; no GPIO, addresses or flash partitions.
#[derive(Clone, Copy, Debug)]
pub struct Configuration {
    pub scene: Scene,
    pub pages: Pages,
    fps: u8,
}

impl Configuration {
    pub fn new(scene: Scene, pages: Pages, fps: u8) -> Result<Self, ConfigError> {
        FramePacer::new(fps)?;
        Ok(Self { scene, pages, fps })
    }

    pub fn fps(&self) -> u8 {
        self.fps
    }

    pub fn encoded_len(&self) -> usize {
        HEADER + 4 + usize::from(self.scene.len) * LAYER + usize::from(self.pages.len) * PAGE
    }

    /// Writes one complete blob; short output is rejected before any writes.
    pub fn encode(&self, output: &mut [u8]) -> Result<usize, DecodeError> {
        let size = self.encoded_len();
        let out = output.get_mut(..size).ok_or(DecodeError::Length)?;
        out.fill(0);
        out[..4].copy_from_slice(b"AJDC");
        out[4..6].copy_from_slice(&1_u16.to_le_bytes());
        out[6..8].copy_from_slice(&((size - HEADER) as u16).to_le_bytes());
        out[12..16].copy_from_slice(&[
            self.fps,
            self.scene.brightness,
            self.scene.len,
            self.pages.len,
        ]);
        let mut cursor = 16;
        for layer in &self.scene.layers[..usize::from(self.scene.len)] {
            let row = &mut out[cursor..cursor + LAYER];
            row[0] = match layer.effect {
                Effect::Solid => 0,
                Effect::Wave => 1,
                Effect::Breathe => 2,
                Effect::Comet => 3,
                Effect::Sparkle => 4,
            };
            row[1] = layer.palette.len;
            for (i, color) in layer.palette.colors[..usize::from(layer.palette.len)]
                .iter()
                .enumerate()
            {
                row[2 + i * 3..5 + i * 3].copy_from_slice(&[color.0, color.1, color.2]);
            }
            row[14..18].copy_from_slice(&layer.period_ms.to_le_bytes());
            row[18] = layer.repeats;
            row[19] = u8::from(layer.reverse);
            row[20] = layer.opacity;
            row[21..29].copy_from_slice(&layer.mask.to_le_bytes());
            cursor += LAYER;
        }
        for page in &self.pages.pages[..usize::from(self.pages.len)] {
            let row = &mut out[cursor..cursor + PAGE];
            row[..4].copy_from_slice(&page.duration_ms.to_le_bytes());
            match page.content {
                Content::Clock(style) => {
                    row[4] = 0;
                    row[5..13].copy_from_slice(&[
                        style.foreground.0,
                        style.foreground.1,
                        style.foreground.2,
                        style.background.0,
                        style.background.1,
                        style.background.2,
                        style.scale,
                        u8::from(style.blink_colon),
                    ]);
                }
                Content::Solid(color) => {
                    row[4] = 1;
                    row[5..8].copy_from_slice(&[color.0, color.1, color.2]);
                }
                Content::Image(id) => {
                    row[4] = 2;
                    row[5..7].copy_from_slice(&id.to_le_bytes());
                }
                Content::Animation(clip) => {
                    row[4] = 3;
                    row[5..7].copy_from_slice(&clip.id.to_le_bytes());
                    row[7..9].copy_from_slice(&clip.frames.to_le_bytes());
                    row[9..11].copy_from_slice(&clip.frame_ms.to_le_bytes());
                }
            }
            cursor += PAGE;
        }
        let checksum = crc32(out[..8].iter().chain(out[HEADER..].iter()).copied());
        out[8..12].copy_from_slice(&checksum.to_le_bytes());
        Ok(size)
    }

    /// Validates the whole envelope before parsing. Unknown values fail closed.
    /// Callers retain their running configuration unless this returns Ok.
    pub fn decode(input: &[u8]) -> Result<Self, DecodeError> {
        if !(16..=MAX_CONFIG_BYTES).contains(&input.len()) {
            return Err(DecodeError::Length);
        }
        if &input[..4] != b"AJDC" {
            return Err(DecodeError::Magic);
        }
        if u16le(&input[4..6]) != 1 {
            return Err(DecodeError::Version);
        }
        if usize::from(u16le(&input[6..8])) + HEADER != input.len() {
            return Err(DecodeError::Length);
        }
        let checksum = crc32(input[..8].iter().chain(input[HEADER..].iter()).copied());
        if checksum != u32le(&input[8..12]) {
            return Err(DecodeError::Checksum);
        }
        let (layers, pages) = (input[14], input[15]);
        if !(1..=MAX_LAYERS as u8).contains(&layers) || !(1..=MAX_PAGES as u8).contains(&pages) {
            return Err(DecodeError::Value);
        }
        if 16 + usize::from(layers) * LAYER + usize::from(pages) * PAGE != input.len() {
            return Err(DecodeError::Length);
        }
        let empty_layer = Layer::new(Effect::Solid, Palette::solid(Color::BLACK), 1000, 1)?;
        let mut decoded_layers = [empty_layer; MAX_LAYERS];
        let mut cursor = 16;
        for layer in &mut decoded_layers[..usize::from(layers)] {
            let row = &input[cursor..cursor + LAYER];
            let effect = match row[0] {
                0 => Effect::Solid,
                1 => Effect::Wave,
                2 => Effect::Breathe,
                3 => Effect::Comet,
                4 => Effect::Sparkle,
                _ => return Err(DecodeError::Value),
            };
            if !(1..=4).contains(&row[1]) || row[19] > 1 {
                return Err(DecodeError::Value);
            }
            let mut colors = [Color::BLACK; 4];
            for (i, color) in colors.iter_mut().enumerate() {
                *color = Color(row[2 + i * 3], row[3 + i * 3], row[4 + i * 3]);
                if i >= usize::from(row[1]) && *color != Color::BLACK {
                    return Err(DecodeError::Value);
                }
            }
            *layer = Layer::new(
                effect,
                Palette::new(colors, row[1])?,
                u32le(&row[14..18]),
                row[18],
            )?;
            layer.reverse = row[19] != 0;
            layer.opacity = row[20];
            layer.mask =
                u64::from_le_bytes(row[21..29].try_into().map_err(|_| DecodeError::Length)?);
            cursor += LAYER;
        }
        let mut decoded_pages = [Page::new(1000, Content::Solid(Color::BLACK))?; MAX_PAGES];
        for page in &mut decoded_pages[..usize::from(pages)] {
            let row = &input[cursor..cursor + PAGE];
            let content = match row[4] {
                0 => {
                    if row[12] > 1 {
                        return Err(DecodeError::Value);
                    }
                    let mut style = ClockStyle::new(
                        Color(row[5], row[6], row[7]),
                        Color(row[8], row[9], row[10]),
                        row[11],
                    )?;
                    style.blink_colon = row[12] != 0;
                    Content::Clock(style)
                }
                1 if row[8..].iter().all(|b| *b == 0) => {
                    Content::Solid(Color(row[5], row[6], row[7]))
                }
                2 if row[7..].iter().all(|b| *b == 0) => Content::Image(u16le(&row[5..7])),
                3 if row[11..].iter().all(|b| *b == 0) => Content::Animation(Clip::new(
                    u16le(&row[5..7]),
                    u16le(&row[7..9]),
                    u16le(&row[9..11]),
                )?),
                _ => return Err(DecodeError::Value),
            };
            *page = Page::new(u32le(&row[..4]), content)?;
            cursor += PAGE;
        }
        Ok(Self::new(
            Scene::new(decoded_layers, layers, input[13])?,
            Pages::new(decoded_pages, pages)?,
            input[12],
        )?)
    }
}

fn u16le(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}

fn u32le(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// IEEE CRC-32 detects corruption; it is not firmware authenticity validation.
fn crc32(bytes: impl Iterator<Item = u8>) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn ieee_crc_known_check_value() {
        assert_eq!(super::crc32(b"123456789".iter().copied()), 0xcbf4_3926);
        assert_eq!(super::crc32(core::iter::empty()), 0);
    }
}

//! Local effect definitions, evaluated on the MCU after a board port exists.
use crate::{Color, ConfigError, Effect, Layer, Palette, Scene};

#[derive(Clone, Copy, Debug)]
pub enum Preset {
    Aurora,
    Embers,
    Comet,
    NeonPulse,
}

impl Preset {
    pub fn scene(self) -> Result<Scene, ConfigError> {
        let empty = Layer::new(Effect::Solid, Palette::solid(Color::BLACK), 1000, 1)?;
        let mut layers = [empty; 4];
        let count = match self {
            Self::Aurora => {
                layers[0] = Layer::new(
                    Effect::Wave,
                    Palette::new(
                        [
                            Color(0, 210, 135),
                            Color(0, 100, 255),
                            Color(120, 15, 230),
                            Color(0, 210, 135),
                        ],
                        4,
                    )?,
                    9000,
                    1,
                )?;
                1
            }
            Self::Embers => {
                layers[0] = Layer::new(Effect::Solid, Palette::solid(Color(30, 3, 0)), 1000, 1)?;
                layers[1] = Layer::new(
                    Effect::Sparkle,
                    Palette::new(
                        [
                            Color(255, 35, 0),
                            Color(255, 100, 3),
                            Color(255, 185, 30),
                            Color::BLACK,
                        ],
                        3,
                    )?,
                    2600,
                    1,
                )?;
                layers[1].opacity = 220;
                2
            }
            Self::Comet => {
                layers[0] = Layer::new(
                    Effect::Comet,
                    Palette::new(
                        [
                            Color(0, 200, 255),
                            Color(210, 25, 255),
                            Color(255, 90, 20),
                            Color::BLACK,
                        ],
                        3,
                    )?,
                    3200,
                    1,
                )?;
                1
            }
            Self::NeonPulse => {
                layers[0] = Layer::new(
                    Effect::Wave,
                    Palette::new(
                        [
                            Color(0, 10, 80),
                            Color(90, 0, 55),
                            Color::BLACK,
                            Color::BLACK,
                        ],
                        2,
                    )?,
                    8000,
                    2,
                )?;
                layers[1] =
                    Layer::new(Effect::Breathe, Palette::solid(Color(200, 0, 255)), 3400, 1)?;
                layers[1].opacity = 180;
                2
            }
        };
        Scene::new(layers, count, 180)
    }
}

use crate::model::{Blend, Effect, Layer, Rgb, Scene, keyboard_rows};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub struct Led {
    pub id: u8,
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Copy)]
pub struct Pulse {
    pub x: f32,
    pub y: f32,
    pub time: f32,
}

pub fn keyboard_leds() -> Vec<Led> {
    keyboard_rows()
        .iter()
        .enumerate()
        .flat_map(|(row, keys)| {
            let mut x = 0.0;
            keys.iter()
                .map(move |k| {
                    let led = Led {
                        id: k.id,
                        x: (x + k.width / 2.0) / 16.5,
                        y: row as f32 / 5.0,
                    };
                    x += k.width;
                    led
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn keyboard_frame(scene: &Scene, time: f32, pulses: &[Pulse]) -> Vec<(u8, Rgb)> {
    let leds = keyboard_leds();
    leds.iter()
        .zip(render(&scene.layers, &leds, time, scene.brightness, pulses))
        .map(|(led, color)| (led.id, color))
        .collect()
}

/// An opaque Replace layer above this one hides all of its assigned keys.
pub fn covering_layer(layers: &[Layer], selected: usize) -> Option<&Layer> {
    let current = layers.get(selected)?;
    if !current.enabled {
        return None;
    }
    layers[selected + 1..].iter().rev().find(|upper| {
        upper.enabled
            && upper.blend == Blend::Replace
            && upper.opacity == 1.0
            && (upper.keys.is_empty()
                || (!current.keys.is_empty() && current.keys.is_subset(&upper.keys)))
    })
}

pub fn dock_leds(count: u8) -> Vec<Led> {
    (0..count)
        .map(|id| {
            let a = f32::from(id) / f32::from(count) * TAU;
            Led {
                id,
                x: 0.5 + a.cos() * 0.5,
                y: 0.5 + a.sin() * 0.5,
            }
        })
        .collect()
}

fn mix(a: Rgb, b: Rgb, t: f32) -> [f32; 3] {
    std::array::from_fn(|i| f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t)
}
fn value(l: &Layer, led: Led, t: f32, pulses: &[Pulse]) -> [f32; 3] {
    let angle = l.angle.to_radians();
    let pos = (led.x * angle.cos() + led.y * angle.sin()) * l.scale + l.phase;
    let clock = t * l.speed;
    let amount = match l.effect {
        Effect::Solid => 0.0,
        Effect::Gradient => pos.rem_euclid(1.0),
        Effect::Wave => ((pos - clock) * TAU).sin() * 0.5 + 0.5,
        Effect::Breathing => (clock * TAU).sin() * 0.5 + 0.5,
        Effect::Ripple => {
            let dist = ((led.x - 0.5).powi(2) + (led.y - 0.5).powi(2)).sqrt();
            ((dist * l.scale - clock) * TAU).cos() * 0.5 + 0.5
        }
        Effect::Sparkle => {
            let frame = (clock * 12.0).floor() as u32;
            let hash = u32::from(led.id)
                .wrapping_mul(2654435761)
                .wrapping_add(frame.wrapping_mul(2246822519));
            if hash % 13 < 2 { 1.0 } else { 0.03 }
        }
        Effect::Aurora
        | Effect::Plasma
        | Effect::Comet
        | Effect::DigitalRain
        | Effect::Embers
        | Effect::Orbit
        | Effect::Visor
        | Effect::TwinWave
        | Effect::Checkerboard
        | Effect::Fireflies
        | Effect::Raindrops
        | Effect::Lightning
        | Effect::Heartbeat
        | Effect::Nebula
        | Effect::Mosaic => {
            let x = (led.x - 0.5) * angle.cos() + (led.y - 0.5) * angle.sin() + 0.5;
            let y = -(led.x - 0.5) * angle.sin() + (led.y - 0.5) * angle.cos() + 0.5;
            let wave = |v: f32| (v * TAU).sin() * 0.5 + 0.5;
            let (hue, light) = match l.effect {
                Effect::Aurora => {
                    let curtain =
                        wave(x * l.scale * 2.0 + wave(y * 0.7 + clock * 0.4) - clock * 0.3);
                    (wave(y + clock * 0.2), 0.05 + curtain.powf(3.0) * 0.95)
                }
                Effect::Plasma => {
                    let a = wave(x * l.scale + clock);
                    let b = wave(y * l.scale * 1.7 - clock * 0.7);
                    let c = wave(((x - 0.5).hypot(y - 0.5)) * l.scale * 3.0 - clock * 0.5);
                    ((a + b + c) / 3.0, 0.25 + 0.75 * a * b)
                }
                Effect::Comet => {
                    let lane = (y * 7.0).floor();
                    let head = (clock * (0.6 + lane * 0.07) + lane * 0.173).rem_euclid(1.5) - 0.25;
                    let tail = (head - x * l.scale).rem_euclid(1.5);
                    (1.0 - (tail * 2.0).min(1.0), (-tail * 9.0).exp())
                }
                Effect::DigitalRain => {
                    let column = (x * 17.0 * l.scale).floor();
                    let head = (clock * (0.5 + (column * 1.71).sin().abs()) + column * 0.37)
                        .rem_euclid(1.8);
                    let tail = (head - y).rem_euclid(1.8);
                    ((1.0 - tail * 3.0).max(0.0), (-tail * 6.0).exp())
                }
                Effect::Embers => {
                    let heat = wave(x * 3.0 * l.scale + clock * 0.4)
                        * wave(y * 2.0 - clock * 0.7 + wave(x * 4.0 + clock * 0.2));
                    (heat, 0.04 + heat * heat * 0.96)
                }
                Effect::Orbit => {
                    let ox = 0.5 + (clock * TAU).cos() * 0.35;
                    let oy = 0.5 + (clock * TAU).sin() * 0.35;
                    let a = (-((x - ox).hypot(y - oy)) * 9.0 * l.scale).exp();
                    let b = (-((x - (1.0 - ox)).hypot(y - (1.0 - oy))) * 9.0 * l.scale).exp();
                    (b / (a + b).max(0.001), (a + b).min(1.0))
                }
                Effect::Visor => (x, (1.0 - (x - wave(clock)).abs() * 12.0).clamp(0.0, 1.0)),
                Effect::TwinWave => (
                    (wave(x * l.scale + clock) + wave(x * l.scale - clock)) / 2.0,
                    1.0,
                ),
                Effect::Checkerboard => (
                    ((x * l.scale * 4.0).floor()
                        + (y * l.scale * 4.0).floor()
                        + (clock * 4.0).floor())
                    .rem_euclid(2.0),
                    1.0,
                ),
                Effect::Fireflies => {
                    let a = wave(clock + (u32::from(led.id) * 7919 % 255) as f32 / 255.0);
                    (a, a.powi(3))
                }
                Effect::Raindrops => {
                    let age = clock.rem_euclid(1.0);
                    let seed = (clock.floor() * 1.97).sin();
                    let d = (x - (seed * 0.5 + 0.5)).abs() + (y - wave(seed)).abs();
                    let a = (1.0 - (d - age * 2.0).abs() * 8.0).max(0.0) * (1.0 - age);
                    (a, a)
                }
                Effect::Lightning => {
                    let tick = clock.rem_euclid(1.0);
                    let a = if tick < 0.05 || (0.14..0.17).contains(&tick) {
                        (1.0 - (x - wave(clock.floor() + y * 3.0)).abs() * 10.0).max(0.0)
                    } else {
                        0.0
                    };
                    (a, a)
                }
                Effect::Heartbeat => {
                    let tick = clock.rem_euclid(1.0);
                    let a = (1.0 - (tick - 0.125).abs() * 12.8)
                        .max(0.0)
                        .max((1.0 - (tick - 0.3125).abs() * 18.0).max(0.0) * 0.7);
                    (a, a)
                }
                Effect::Nebula => {
                    let a = wave(x * l.scale + clock + wave(y * l.scale - clock / 3.0))
                        * wave((x + y) * l.scale / 1.5 + clock / 2.0);
                    (a, 0.1 + a * 0.9)
                }
                Effect::Mosaic => {
                    let cell = (x * l.scale * 4.0).floor() + 16.0 * (y * l.scale * 4.0).floor();
                    (wave(cell * 1.7 + clock * 0.3), 1.0)
                }
                _ => unreachable!(),
            };
            return mix(l.colors[0], l.colors[1], hue.clamp(0.0, 1.0)).map(|c| c * light);
        }
        Effect::Reactive => {
            return pulses.iter().fold([0.0; 3], |mut acc, p| {
                let age = t - p.time;
                if !(0.0..=2.0).contains(&age) {
                    return acc;
                }
                let dist = ((led.x - p.x).powi(2) + (led.y - p.y).powi(2)).sqrt();
                let strength = (1.0 - ((dist - age * (l.speed + 0.1)) * 8.0).abs()).max(0.0)
                    * (1.0 - age / 2.0);
                for (i, c) in acc.iter_mut().enumerate() {
                    *c = (*c + f32::from(l.colors[0][i]) * strength).min(255.0);
                }
                acc
            });
        }
    };
    let color = mix(l.colors[0], l.colors[1], amount);
    if matches!(l.effect, Effect::Breathing | Effect::Sparkle) {
        color.map(|v| v * amount)
    } else {
        color
    }
}

pub fn render(
    layers: &[Layer],
    leds: &[Led],
    time: f32,
    brightness: f32,
    pulses: &[Pulse],
) -> Vec<Rgb> {
    leds.iter()
        .map(|&led| {
            let mut dst = [0.0_f32; 3];
            for layer in layers
                .iter()
                .filter(|l| l.enabled && (l.keys.is_empty() || l.keys.contains(&led.id)))
            {
                let src = value(layer, led, time, pulses);
                for i in 0..3 {
                    dst[i] = match layer.blend {
                        Blend::Replace => dst[i] * (1.0 - layer.opacity) + src[i] * layer.opacity,
                        Blend::Add => (dst[i] + src[i] * layer.opacity).min(255.0),
                        Blend::Multiply => {
                            dst[i] * (1.0 - layer.opacity + src[i] / 255.0 * layer.opacity)
                        }
                    };
                }
            }
            dst.map(|v| (v * brightness.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8)
        })
        .collect()
}

pub fn matrix(scene: &Scene, time: f32) -> Vec<Rgb> {
    let leds = keyboard_leds();
    let colors = render(&scene.layers, &leds, time, scene.brightness, &[]);
    let mut result = vec![[0; 3]; 128];
    for (led, color) in leds.iter().zip(colors) {
        result[usize::from(led.id)] = color;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    #[test]
    fn hidden_palette_is_reported_only_for_a_covering_opaque_layer() {
        let lower = Layer {
            effect: Effect::Solid,
            colors: [[255, 0, 0]; 2],
            ..Default::default()
        };
        let upper = Layer {
            effect: Effect::Solid,
            colors: [[0, 0, 255]; 2],
            ..Default::default()
        };
        let mut layers = vec![lower, upper];
        assert_eq!(covering_layer(&layers, 0).unwrap().colors[0], [0, 0, 255]);
        let led = [Led {
            id: 1,
            x: 0.0,
            y: 0.0,
        }];
        assert_eq!(render(&layers, &led, 0.0, 1.0, &[]), vec![[0, 0, 255]]);
        layers[1].enabled = false;
        assert!(covering_layer(&layers, 0).is_none());
        assert_eq!(render(&layers, &led, 0.0, 1.0, &[]), vec![[255, 0, 0]]);
        layers[1].enabled = true;
        layers[1].opacity = 0.5;
        assert!(covering_layer(&layers, 0).is_none());
        layers[1].opacity = 1.0;
        layers[1].blend = Blend::Add;
        assert!(covering_layer(&layers, 0).is_none());
    }
    #[test]
    fn a_partial_mask_does_not_hide_an_entire_surface() {
        let mut layers = vec![
            Layer::default(),
            Layer {
                keys: [1, 2].into_iter().collect(),
                ..Default::default()
            },
        ];
        assert!(covering_layer(&layers, 0).is_none());
        layers[0].keys = [1].into_iter().collect();
        assert!(covering_layer(&layers, 0).is_some());
        layers[0].keys = [3].into_iter().collect();
        assert!(covering_layer(&layers, 0).is_none());
    }
    #[test]
    fn masked_layer_does_not_touch_other_keys() {
        let layer = Layer {
            effect: Effect::Solid,
            keys: [1].into_iter().collect(),
            colors: [[80, 120, 240]; 2],
            ..Default::default()
        };
        assert_eq!(
            render(
                &[layer],
                &[
                    Led {
                        id: 0,
                        x: 0.0,
                        y: 0.0
                    },
                    Led {
                        id: 1,
                        x: 0.0,
                        y: 0.0
                    }
                ],
                0.0,
                0.5,
                &[]
            ),
            vec![[0, 0, 0], [40, 60, 120]]
        );
    }
    #[test]
    fn layout_has_unique_real_matrix_ids() {
        let leds = keyboard_leds();
        let ids: std::collections::BTreeSet<_> = leds.iter().map(|l| l.id).collect();
        assert_eq!(leds.len(), 82);
        assert_eq!(ids.len(), 82);
        assert!(ids.iter().all(|&id| id < 128));
    }
    #[test]
    fn project_rejects_unsupported_schema() {
        let s = Scene {
            schema: 99,
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }
}

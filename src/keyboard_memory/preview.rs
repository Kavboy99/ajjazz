//! Integer preview checked byte-for-byte against the compiled ARM renderer.
use super::*;
fn positions() -> [[i32; 3]; 126] {
    let rows = keyboard_rows();
    let mut points = vec![];
    for (y, row) in rows.iter().enumerate() {
        let mut x = 0.0;
        for k in row {
            points.push((k.id, x + k.width / 2.0, y));
            x += k.width;
        }
    }
    let lo = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let hi = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    let mut p = [[0; 3]; 126];
    for (id, x, y) in points {
        p[usize::from(id)] = [
            (((x - lo) / (hi - lo)) * 255.0).round_ties_even() as i32,
            (y as f32 / (rows.len() - 1) as f32 * 255.0).round_ties_even() as i32,
            1,
        ];
    }
    p
}
fn wave(v: u32) -> u32 {
    let x = v & 255;
    if x < 128 { x * 2 } else { 510 - x * 2 }
}
fn noise(mut n: u32) -> u32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846ca68b);
    n ^ (n >> 16)
}
fn weight(
    l: &[u8],
    id: usize,
    frame: u32,
    pos: &[[i32; 3]; 126],
    pressed: &[Option<u16>; 128],
    last: Option<u8>,
    speed: u8,
) -> u32 {
    let [x, y, _] = pos[id];
    let projection = (x * i32::from(word(l, 14) as i16) + y * i32::from(word(l, 16) as i16)) / 256;
    let scale = u32::from(word(l, 12));
    let t = (frame.wrapping_mul(u32::from(word(l, 10))) / 50).wrapping_mul(u32::from(speed)) / 128
        + u32::from(word(l, 18));
    let p = ((projection * scale as i32) / 256) as u32;
    let p = p.wrapping_add(t);
    let h = noise((id as u32).wrapping_mul(1337).wrapping_add(t >> 3));
    match l[1] {
        0 => 0,
        1 => ((projection as u32).wrapping_mul(scale) / 256) & 255,
        2 => wave(p),
        3 => wave(t),
        4 => {
            let Some(last) = last else { return 0 };
            let k = usize::from(last);
            let Some(at) = pressed[k] else { return 0 };
            let age = (frame as u16).wrapping_sub(at);
            if age > 100 {
                return 0;
            }
            let d = (x - pos[k][0]).unsigned_abs() + (y - pos[k][1]).unsigned_abs();
            let gap = (d * scale / 256).abs_diff(u32::from(age) * 6);
            if gap < 36 { (36 - gap) * 7 } else { 0 }
        }
        5 => {
            if h & 255 > 232 {
                255
            } else {
                0
            }
        }
        6 => {
            let Some(at) = pressed[id] else { return 0 };
            let age = (frame as u16).wrapping_sub(at);
            if age < 50 { u32::from(50 - age) * 5 } else { 0 }
        }
        7 => {
            (wave(p.wrapping_add(wave((y as u32 * scale / 256).wrapping_add(t / 2)) / 2))
                + wave((p / 2).wrapping_add(t / 3)))
                / 2
        }
        8 => {
            (wave((x as u32 * scale / 256).wrapping_add(t))
                + wave((y as u32 * scale / 128).wrapping_add(t.wrapping_mul(2)))
                + wave(((x + y) as u32 * scale / 256).wrapping_sub(t)))
                / 3
        }
        9 => {
            let d = p & 255;
            if d < 96 { 255 - d * 255 / 96 } else { 0 }
        }
        10 => {
            let fall = ((y as u32 * scale / 128)
                .wrapping_add(t)
                .wrapping_add(noise(x as u32 / 18).wrapping_mul(3)))
                & 255;
            if fall < 72 { 255 - fall * 255 / 72 } else { 0 }
        }
        11 => (40 + (h & 127) + wave(t.wrapping_add(x as u32).wrapping_add(y as u32)) / 3).min(255),
        12 => wave(
            (((x - 128).unsigned_abs() + (y - 128).unsigned_abs()) * scale / 128).wrapping_sub(t),
        ),
        13 => {
            let d = x.abs_diff(wave(t) as i32);
            if d < 48 { (48 - d) * 255 / 48 } else { 0 }
        }
        14 => (wave(p) + wave((((projection * scale as i32) / 256) as u32).wrapping_sub(t))) / 2,
        15 => {
            if ((x as u32 * scale / 16384 + y as u32 * scale / 16384 + (t >> 5)) & 1) != 0 {
                255
            } else {
                0
            }
        }
        16 => {
            let v = wave(t.wrapping_add(noise((id as u32) * 7919) & 255));
            v * v / 255 * v / 255
        }
        17 => {
            let age = t & 255;
            let seed = noise((t >> 8) * 991);
            let cx = (seed & 255) as i32;
            let cy = ((seed >> 8) & 255) as i32;
            let d = (x - cx).unsigned_abs() + (y - cy).unsigned_abs();
            let gap = d.abs_diff(age * 2);
            if gap < 32 {
                (32 - gap) * (255 - age) / 32
            } else {
                0
            }
        }
        18 => {
            let tick = t & 255;
            if tick > 12 && !(36..=42).contains(&tick) {
                0
            } else {
                let column = noise((t >> 8) * 31) & 255;
                let bolt = column as i32 + (noise((y as u32) / 40 + (t >> 8)) % 65) as i32 - 32;
                if (x - bolt).unsigned_abs() < 26 {
                    255
                } else {
                    8
                }
            }
        }
        19 => {
            let tick = t & 255;
            let a = tick.abs_diff(32);
            let b = tick.abs_diff(80);
            if a < 20 {
                (20 - a) * 255 / 20
            } else if b < 14 {
                (14 - b) * 180 / 14
            } else {
                0
            }
        }
        20 => {
            let a = wave(p.wrapping_add(wave((y as u32 * scale / 256).wrapping_sub(t / 3))));
            let b = wave(((x + y) as u32 * scale / 384).wrapping_add(t / 2));
            a * b / 255
        }
        21 => {
            let cell = x as u32 * scale / 16384 + 16 * (y as u32 * scale / 16384);
            let a = noise(cell.wrapping_add((t >> 7) * 977)) & 255;
            let b = noise(cell.wrapping_add(((t >> 7) + 1) * 977)) & 255;
            let w = t & 127;
            (a * (128 - w) + b * w) / 128
        }
        _ => 0,
    }
}
pub fn render_program(
    page: &[u8],
    frame: u32,
    pressed: &[Option<u16>; 128],
    last: Option<u8>,
    brightness: u8,
    speed: u8,
) -> Result<Vec<[u8; 3]>> {
    validate_page(page, false)?;
    ensure!(
        (8..=248).contains(&speed) && last.is_none_or(|id| id < 126),
        "Неверное состояние предпросмотра AJKS"
    );
    let pos = positions();
    let mut colors = vec![[0u8; 3]; 126];
    for (id, c) in colors.iter_mut().enumerate() {
        if pos[id][2] == 0 {
            continue;
        }
        let mut value = [0u32; 3];
        for l in page[64..64 + usize::from(page[5]) * 48].as_chunks::<48>().0 {
            if l[0] == 0 || l[20 + id / 8] & (1 << (id % 8)) == 0 {
                continue;
            }
            let w = weight(l, id, frame, &pos, pressed, last, speed);
            let alpha = u32::from(l[3]);
            for j in 0..3 {
                let mut v = (u32::from(l[4 + j]) * (255 - w) + u32::from(l[7 + j]) * w) / 255;
                if l[1] >= 4 && ![7, 8, 11, 12, 14, 15, 20, 21].contains(&l[1]) {
                    v = v * w / 255;
                }
                value[j] = match l[2] {
                    0 => (value[j] * (255 - alpha) + v * alpha) / 255,
                    1 => (value[j] + v * alpha / 255).min(255),
                    _ => value[j] * ((255 - alpha) + v * alpha / 255) / 255,
                };
            }
        }
        for j in 0..3 {
            c[j] = (value[j] * u32::from(page[7]) / 255 * u32::from(brightness) / 255) as u8;
        }
    }
    Ok(colors)
}
pub fn scene_preview(
    scene: &Scene,
    time: f32,
    pulses: &[crate::lighting::Pulse],
) -> Result<Vec<[u8; 3]>> {
    device_preview(scene, time, pulses, 255, 128, false)
}
// Inverse sRGB, followed by rounding to the6-bit duty cycle of the stock driver.
fn pwm6(code: u8) -> u8 {
    let x = f64::from(code) / 255.0;
    let linear = if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    };
    (linear * 63.0).round() as u8
}
#[cfg(test)]
fn pwm_input(code: u8) -> u8 {
    let pwm = u32::from(pwm6(code));
    if pwm == 0 {
        0
    } else {
        (pwm * 262144).div_ceil(65025) as u8
    }
}
pub(super) fn display_pwm(code: u8) -> u8 {
    let linear = f64::from(pwm6(code)) / 63.0;
    let x = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (x * 255.0).round() as u8
}
pub fn device_preview(
    scene: &Scene,
    time: f32,
    pulses: &[crate::lighting::Pulse],
    brightness: u8,
    speed: u8,
    color_transfer: bool,
) -> Result<Vec<[u8; 3]>> {
    let page = compile(scene)?;
    let frame = (time.max(0.0) * 50.0) as u32;
    let mut pressed = [None; 128];
    let mut last = None;
    let leds = crate::lighting::keyboard_leds();
    for pulse in pulses {
        if time - pulse.time > 2.0 || pulse.time > time {
            continue;
        }
        if let Some(led) = leds.iter().min_by(|a, b| {
            ((a.x - pulse.x).powi(2) + (a.y - pulse.y).powi(2))
                .total_cmp(&((b.x - pulse.x).powi(2) + (b.y - pulse.y).powi(2)))
        }) {
            pressed[usize::from(led.id)] = Some((pulse.time.max(0.0) * 50.0) as u16);
            last = Some(led.id);
        }
    }
    let colors = render_program(&page, frame, &pressed, last, brightness, speed)?;
    Ok(leds
        .iter()
        .map(|l| {
            colors[usize::from(l.id)].map(|code| {
                if color_transfer {
                    display_pwm(code)
                } else {
                    code
                }
            })
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Deserialize)]
    struct Vector {
        page: Vec<u8>,
        frame: u32,
        pressed_id: u8,
        pressed_at: u16,
        brightness: u8,
        speed: u8,
        rgb: Vec<u8>,
        #[serde(default)]
        color_transfer: u8,
    }
    #[test]
    fn preview_matches_compiled_arm_all_effects() {
        let vectors: Vec<Vector> = serde_json::from_str(include_str!(
            "../../tests/fixtures/keyboard-ajks-preview.json"
        ))
        .unwrap();
        assert_eq!(vectors.len(), Effect::ALL.len() * 4);
        for v in vectors {
            let mut pressed = [None; 128];
            pressed[usize::from(v.pressed_id)] = Some(v.pressed_at);
            let colors = render_program(
                &v.page,
                v.frame,
                &pressed,
                Some(v.pressed_id),
                v.brightness,
                v.speed,
            )
            .unwrap();
            let planes: Vec<_> = (0..3)
                .flat_map(|j| {
                    colors.iter().map(move |c| {
                        if v.color_transfer == 1 {
                            pwm_input(c[j])
                        } else {
                            c[j]
                        }
                    })
                })
                .collect();
            assert_eq!(planes, v.rgb, "effect {} frame {}", v.page[65], v.frame);
        }
    }
    #[test]
    fn transfer_preserves_neutral_gray_and_primary_channels_with_stock_power_limit() {
        assert_eq!(pwm6(0), 0);
        assert_eq!(pwm6(255), 63);
        assert_eq!(pwm6(128), 14);
        assert_eq!(pwm6(31), 1);
        for code in 0..=255 {
            let input = u32::from(pwm_input(code));
            assert_eq!((input * 65025) >> 18, u32::from(pwm6(code)));
        }
        assert_eq!([255, 0, 0].map(display_pwm), [255, 0, 0]);
        assert_eq!([0, 255, 0].map(display_pwm), [0, 255, 0]);
        assert_eq!([0, 0, 255].map(display_pwm), [0, 0, 255]);
    }
}

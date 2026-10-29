//! Packet formats transcribed from source evidence. No bootloader/erase API.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const HEADER: usize = 8;
pub const READ_BLOCKS: &[(u8, usize, &str)] = &[
    (0x10, 48, "device_info"),
    (0x11, 56, "game_mode"),
    (0x12, 512, "keymap"),
    (0x13, 16, "lighting"),
    (0x14, 512, "custom_rgb"),
    (0x17, 1024, "rapid_trigger"),
    (0x18, 1024, "dks"),
];

/// SDK setMusicData USB payload: only physical keys, in layout row order.
/// This is distinct from the 128-slot saved color table. Acknowledgement alone
/// does not establish visible support on a particular firmware revision.
pub fn compact_rgb_stream(table: &[u8]) -> Result<Vec<u8>> {
    ensure!(table.len() == 512, "Invalid saved RGB table size");
    ensure!(
        table
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .all(|(id, entry)| entry[0] as usize == id),
        "Invalid saved RGB IDs"
    );
    let mut bytes = Vec::with_capacity(328);
    for key in crate::model::keyboard_rows().into_iter().flatten() {
        let offset = key.id as usize * 4;
        bytes.extend_from_slice(&table[offset..offset + 4]);
    }
    Ok(bytes)
}

/// Minimal packet-sized spans of changed complete [ID,R,G,B] records.
/// Unchanged records inside a span retain their exact bytes; unused slots are
/// omitted unless changed (including initialization of factory-empty IDs).
pub fn rgb_delta_regions(
    before: &[u8],
    after: &[u8],
    report_size: usize,
) -> Result<Vec<std::ops::Range<usize>>> {
    ensure!(
        before.len() == 512 && after.len() == 512,
        "Invalid RGB table size"
    );
    ensure!([32, 64].contains(&report_size), "Unsupported report size");
    ensure!(
        after
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .all(|(id, record)| record[0] as usize == id),
        "Invalid target RGB IDs"
    );
    let old = before.as_chunks::<4>().0;
    let new = after.as_chunks::<4>().0;
    let per_packet = (report_size - HEADER) / 4;
    let mut regions = Vec::new();
    let mut cursor = 0;
    while let Some(start) = (cursor..128).find(|&id| old[id] != new[id]) {
        let end = (start..(start + per_packet).min(128))
            .rfind(|&id| old[id] != new[id])
            .unwrap()
            + 1;
        regions.push(start * 4..end * 4);
        cursor = end;
    }
    Ok(regions)
}

#[cfg(test)]
mod delta_tests {
    use super::*;
    #[test]
    fn usb_stream_is_compact_physical_order_not_the_saved_128_slot_matrix() {
        let table =
            custom_rgb(&std::array::from_fn::<_, 128, _>(|id| [id as u8, 123, 234])).unwrap();
        let stream = compact_rgb_stream(&table).unwrap();
        assert_eq!(stream.len(), 82 * 4);
        let ids: Vec<_> = crate::model::keyboard_rows()
            .into_iter()
            .flatten()
            .map(|key| key.id)
            .collect();
        for (record, id) in stream.as_chunks::<4>().0.iter().zip(ids) {
            assert_eq!(*record, [id, id, 123, 234]);
        }
        let packets = keyboard_packets(0x32, 0, stream.len(), Some(&stream), 64).unwrap();
        assert_eq!(packets.len(), 6);
        assert!(
            packets[..5]
                .iter()
                .all(|packet| packet[2] == 56 && packet[5..8] == [0, 0, 0])
        );
        assert_eq!(&packets[5][..8], &[0xaa, 0x32, 48, 24, 1, 0, 1, 0]);
        assert!(compact_rgb_stream(&table[..511]).is_err());
        assert!(compact_rgb_stream(&[0; 512]).is_err());
    }
    #[test]
    fn sparse_and_dense_updates_reconstruct_exact_target_without_extra_packets() {
        let base = custom_rgb(&[[12, 34, 56]; 128]).unwrap();
        for report_size in [32, 64] {
            for stride in [1, 2, 7, 14, 31, 127] {
                let mut target = base.clone();
                for id in (0..128).step_by(stride) {
                    target[id * 4 + 2] ^= 255;
                }
                let regions = rgb_delta_regions(&base, &target, report_size).unwrap();
                let mut reconstructed = base.clone();
                let mut end = 0;
                for region in &regions {
                    assert!(region.start >= end);
                    assert!(region.len() <= report_size - HEADER);
                    assert_eq!(region.start % 4, 0);
                    assert_eq!(region.end % 4, 0);
                    reconstructed[region.clone()].copy_from_slice(&target[region.clone()]);
                    end = region.end;
                }
                assert_eq!(reconstructed, target);
                if stride == 1 {
                    assert_eq!(
                        regions.len(),
                        128_usize.div_ceil((report_size - HEADER) / 4)
                    );
                }
            }
            assert!(
                rgb_delta_regions(&base, &base, report_size)
                    .unwrap()
                    .is_empty()
            );
        }
    }
    #[test]
    fn factory_ids_are_initialized_and_corrupt_targets_rejected() {
        let target = custom_rgb(&[[0; 3]; 128]).unwrap();
        let regions = rgb_delta_regions(&[0; 512], &target, 64).unwrap();
        let mut restored = vec![0; 512];
        for region in regions {
            restored[region.clone()].copy_from_slice(&target[region]);
        }
        assert_eq!(restored, target);
        assert!(rgb_delta_regions(&[0; 511], &target, 64).is_err());
        assert!(rgb_delta_regions(&target, &[0; 512], 64).is_err());
        assert!(rgb_delta_regions(&target, &target, 65).is_err());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardLinkEvent {
    Disconnected,
    Sleeping,
    Awake,
}
/// Vendor KX/qX/YX unsolicited link notifications, separate from command replies.
pub fn keyboard_link_event(bytes: &[u8]) -> Option<KeyboardLinkEvent> {
    let bytes = if bytes.first() == Some(&0) {
        &bytes[1..]
    } else {
        bytes
    };
    if bytes.len() < 3 {
        return None;
    }
    if bytes[..3] == [0xa6, 0xff, 1] {
        return Some(KeyboardLinkEvent::Awake);
    }
    if bytes[..2] != [0x55, 0xfc] {
        return None;
    }
    match bytes[2] {
        4 => Some(KeyboardLinkEvent::Disconnected),
        5 => Some(KeyboardLinkEvent::Sleeping),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardPacket {
    pub command: u8,
    pub address: u16,
    pub payload: Vec<u8>,
}

pub fn keyboard_packets(
    command: u8,
    address: u16,
    length: usize,
    data: Option<&[u8]>,
    report_size: usize,
) -> Result<Vec<Vec<u8>>> {
    ensure!([32, 64].contains(&report_size), "Unsupported report size");
    ensure!(
        length > 0 && usize::from(address) + length <= 65536,
        "Address outside protocol range"
    );
    if let Some(data) = data {
        ensure!(data.len() == length, "Payload length mismatch");
    }
    let chunk_size = report_size - HEADER;
    let count = length.div_ceil(chunk_size);
    Ok((0..count)
        .map(|i| {
            let offset = i * chunk_size;
            let len = (length - offset).min(chunk_size);
            let addr = usize::from(address) + offset;
            let mut p = vec![0; report_size];
            p[0] = 0xaa;
            p[1] = command;
            p[2] = len as u8;
            p[3] = addr as u8;
            p[4] = (addr >> 8) as u8;
            p[6] = u8::from(i + 1 == count);
            if let Some(data) = data {
                p[8..8 + len].copy_from_slice(&data[offset..offset + len]);
            }
            p
        })
        .collect())
}

pub fn parse_keyboard_response(
    bytes: &[u8],
    command: u8,
    address: u16,
    expected: usize,
) -> Result<KeyboardPacket> {
    // hidapi removes the unused ID for unnumbered Input reports.
    let bytes = if bytes.first() == Some(&0) {
        &bytes[1..]
    } else {
        bytes
    };
    ensure!(bytes.len() >= HEADER, "Truncated HID response");
    ensure!(
        bytes[0] == 0x55 && bytes[1] == command,
        "Response command mismatch"
    );
    let addr = u16::from_le_bytes([bytes[3], bytes[4]]);
    ensure!(addr == address, "Response address mismatch");
    ensure!(
        usize::from(bytes[2]) == expected,
        "Response length mismatch"
    );
    ensure!(
        bytes.len() >= HEADER + expected,
        "Truncated response payload"
    );
    Ok(KeyboardPacket {
        command,
        address: addr,
        payload: bytes[8..8 + expected].to_vec(),
    })
}

/// The original 1.30 firmware reports custom lighting as mode 20.
/// Keep all unrelated lighting fields from the read-back block.
pub fn static_lighting(original: &[u8]) -> Result<Vec<u8>> {
    ensure!(original.len() == 16, "Invalid lighting block");
    let mut data = original.to_vec();
    data[0] = 20;
    data[4] = 255;
    data[14] = 0xaa;
    data[15] = 0x55;
    Ok(data)
}

/// GET 0x13 reports custom mode as 20 on 80A0 1.30; SET 0x23 uses 128.
/// Keep the persisted/read-back representation separate from the wire command.
pub fn lighting_command(state: &[u8]) -> Result<Vec<u8>> {
    ensure!(state.len() == 16, "Invalid lighting block");
    let mut bytes = state.to_vec();
    if bytes[0] == 20 {
        bytes[0] = 128;
    }
    bytes[4] = 255;
    bytes[14] = 0xaa;
    bytes[15] = 0x55;
    Ok(bytes)
}

pub fn lighting_equivalent(a: &[u8], b: &[u8]) -> bool {
    a.len() == 16
        && b.len() == 16
        && (0..16)
            .filter(|i| ![4, 14, 15].contains(i))
            .all(|i| a[i] == b[i])
}

/// 80A0 1.30 returns the default white/RGB palette even after the vendor site
/// successfully changes the visible native color. Only accept that exact known
/// fallback; mode, brightness, speed, direction and reserved fields still match.
pub fn native_effect_readback_matches(observed: &[u8], requested: &[u8]) -> bool {
    if lighting_equivalent(observed, requested) {
        return true;
    }
    if observed.len() != 16
        || requested.len() != 16
        || requested[0] > 19
        || observed[1..4] != [255, 255, 255]
        || observed[8] != 1
    {
        return false;
    }
    let mut normalized = requested.to_vec();
    normalized[1..4].copy_from_slice(&observed[1..4]);
    normalized[8] = observed[8];
    lighting_equivalent(observed, &normalized)
}

pub const BUILTIN_LIGHTING: &[(u8, &str)] = &[
    (0, "Выключено"),
    (1, "Постоянный цвет"),
    (2, "Свечение нажатой клавиши"),
    (3, "Затухание нажатой клавиши"),
    (4, "Звёзды"),
    (5, "Снег"),
    (6, "Цветение"),
    (7, "Дыхание"),
    (8, "Смена спектра"),
    (9, "Фонтан"),
    (10, "Цветные полосы"),
    (11, "Волна"),
    (12, "Горные волны"),
    (13, "Вспышка при нажатии"),
    (14, "Двойная реакция"),
    (15, "Круги от нажатия"),
    (16, "Поток"),
    (17, "Пики"),
    (18, "Дождь"),
    (19, "Маятник"),
    (20, "Сохранённые цвета клавиш"),
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardLighting {
    pub mode: u8,
    pub color: [u8; 3],
    pub rainbow: bool,
    pub brightness: u8,
    pub speed: u8,
    pub direction: u8,
}
impl KeyboardLighting {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() == 16, "Invalid keyboard lighting block");
        Ok(Self {
            mode: raw[0],
            color: [raw[1], raw[2], raw[3]],
            rainbow: raw[8] != 0,
            brightness: raw[9],
            speed: raw[10],
            direction: raw[11],
        })
    }
    pub fn patch(&self, raw: &[u8]) -> Result<Vec<u8>> {
        ensure!(raw.len() == 16, "Invalid keyboard lighting block");
        ensure!(
            BUILTIN_LIGHTING.iter().any(|(mode, _)| *mode == self.mode),
            "Unknown keyboard effect"
        );
        ensure!(
            (1..=5).contains(&self.brightness)
                && (1..=5).contains(&self.speed)
                && self.direction <= 3,
            "Lighting parameters outside known limits"
        );
        let mut out = raw.to_vec();
        out[0] = self.mode;
        if ![0, 6, 8, 20].contains(&self.mode) {
            out[1..4].copy_from_slice(&self.color);
            out[8] = u8::from(self.rainbow);
        }
        out[4] = 255;
        out[9] = self.brightness;
        out[10] = self.speed;
        out[11] = self.direction;
        out[14] = 0xaa;
        out[15] = 0x55;
        Ok(out)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyboardAnimationLimits {
    pub gif_frames: u16,
    pub led_frames: u16,
    pub tft_frames: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyboardInfo {
    pub vid: u16,
    pub pid: u16,
    pub version: String,
    pub manufacturer: u16,
    pub product: u16,
    pub work_mode: u8,
    pub battery: u8,
    pub charging: u8,
    pub profile: u8,
    pub rt_precision: u8,
    pub frame_version: u8,
    pub lighting_version: u8,
    pub firmware_status: u8,
    /// Absent in older saved snapshots; never treat absence as a device-reported zero.
    #[serde(default)]
    pub animation_limits: Option<KeyboardAnimationLimits>,
}
impl KeyboardInfo {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(b.len() == 48, "Expected 48 bytes of device info");
        ensure!(b[17] <= 100, "Invalid battery value");
        Ok(Self {
            vid: u16::from_le_bytes([b[4], b[5]]),
            pid: u16::from_le_bytes([b[6], b[7]]),
            version: format!("{}.{:02}", b[9], (b[8] >> 4) * 10 + (b[8] & 15)),
            manufacturer: u16::from_le_bytes([b[12], b[13]]),
            product: u16::from_le_bytes([b[14], b[15]]),
            work_mode: b[16],
            battery: b[17],
            charging: b[18],
            profile: b[19],
            rt_precision: b[29],
            frame_version: b[30],
            lighting_version: b[31],
            firmware_status: b[32],
            animation_limits: Some(KeyboardAnimationLimits {
                gif_frames: u16::from_le_bytes([b[24], b[25]]),
                led_frames: u16::from_le_bytes([b[26], b[27]]),
                tft_frames: u16::from_le_bytes([b[22], b[23]]),
            }),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HallKey {
    pub axis: u8,
    pub rapid: bool,
    pub whole_travel: bool,
    pub continuous: bool,
    pub actuation: f32,
    pub press: f32,
    pub release: f32,
}
impl HallKey {
    pub fn decode(b: &[u8], precision: u8) -> Result<Self> {
        ensure!(
            b.len() == 8 && precision <= 2,
            "Invalid Hall record/precision"
        );
        let rt = if precision > 0 { 1000.0 } else { 100.0 };
        let travel = if precision == 2 { 1000.0 } else { 100.0 };
        Ok(Self {
            axis: b[0],
            rapid: b[4..8].iter().any(|v| *v != 0),
            whole_travel: b[1] & 1 != 0,
            continuous: b[1] & 2 != 0,
            actuation: f32::from(u16::from_le_bytes([b[2], b[3]])) / travel,
            press: f32::from(u16::from_le_bytes([b[4], b[5]])) / rt,
            release: f32::from(u16::from_le_bytes([b[6], b[7]])) / rt,
        })
    }
    pub fn patch(&self, original: &[u8], precision: u8) -> Result<[u8; 8]> {
        ensure!(
            original.len() == 8 && precision <= 2,
            "Unknown Hall encoding"
        );
        ensure!(self.axis <= 3, "Unsupported switch type");
        ensure!(
            self.actuation.is_finite() && (0.1..=3.3).contains(&self.actuation),
            "Actuation: 0.10–3.30 mm"
        );
        ensure!(
            [self.press, self.release]
                .iter()
                .all(|v| v.is_finite() && (0.01..=3.3).contains(v)),
            "Travel outside known AK820MAX range"
        );
        let rt = if precision > 0 { 1000.0 } else { 100.0 };
        let travel = if precision == 2 { 1000.0 } else { 100.0 };
        let mut b: [u8; 8] = original.try_into()?;
        b[0] = self.axis;
        b[1] = (b[1] & !3) | u8::from(self.whole_travel) | (u8::from(self.continuous) << 1);
        for (offset, val) in [
            (2, (self.actuation * travel).round() as u16),
            (
                4,
                if self.rapid {
                    (self.press * rt).round() as u16
                } else {
                    0
                },
            ),
            (
                6,
                if self.rapid {
                    (self.release * rt).round() as u16
                } else {
                    0
                },
            ),
        ] {
            b[offset..offset + 2].copy_from_slice(&val.to_le_bytes());
        }
        Ok(b)
    }
}

pub fn custom_rgb(colors: &[[u8; 3]]) -> Result<Vec<u8>> {
    ensure!(colors.len() == 128, "RGB matrix must contain all 128 slots");
    Ok(colors
        .iter()
        .enumerate()
        .flat_map(|(id, c)| [id as u8, c[0], c[1], c[2]])
        .collect())
}

/// Only established configuration areas can reach the transport. No raw opcodes.
#[derive(Clone, Copy, Debug)]
pub enum KeyboardArea {
    Keymap,
    Lighting,
    CustomRgb,
    RapidTrigger,
    GameMode,
}
impl KeyboardArea {
    pub fn spec(self) -> (u8, u8, usize) {
        match self {
            Self::Keymap => (0x12, 0x22, 512),
            Self::Lighting => (0x13, 0x23, 16),
            Self::CustomRgb => (0x14, 0x24, 512),
            Self::RapidTrigger => (0x17, 0x27, 1024),
            Self::GameMode => (0x11, 0x21, 56),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MouseOptions {
    pub profile: u8,
    pub polling_code: u8,
    pub debounce_ms: u8,
    pub lift_off: u8,
    pub angle_snap: u8,
    pub x_sensitivity: u8,
    pub y_sensitivity: u8,
}
impl MouseOptions {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(
            b.len() == 64 && b[0] == 0xd3,
            "Invalid mouse options response"
        );
        Ok(Self {
            profile: b[8],
            polling_code: b[9],
            debounce_ms: b[10],
            lift_off: b[52],
            angle_snap: b[53],
            x_sensitivity: b[50],
            y_sensitivity: b[51],
        })
    }
    pub fn patch(&self, original: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            original.len() == 64 && original[0] == 0xd3,
            "A device snapshot is required"
        );
        ensure!(
            self.profile == original[8],
            "Profile switching needs separate validation"
        );
        self.polling_hz()?;
        ensure!((2..=10).contains(&self.debounce_ms), "Debounce: 2–10 мс");
        ensure!(
            self.lift_off <= 2 && self.angle_snap <= 1,
            "Invalid LOD/angle snapping"
        );
        ensure!(
            self.x_sensitivity == original[50] && self.y_sensitivity == original[51],
            "Sensitivity editing is not supported for this revision"
        );
        let mut b = original.to_vec();
        b[9] = self.polling_code;
        b[10] = self.debounce_ms;
        b[50] = self.x_sensitivity;
        b[51] = self.y_sensitivity;
        b[52] = self.lift_off;
        b[53] = self.angle_snap;
        Ok(b)
    }
    pub fn polling_hz(&self) -> Result<u32> {
        Ok(match self.polling_code {
            129 => 8000,
            130 => 4000,
            132 => 2000,
            1 => 1000,
            2 => 500,
            4 => 250,
            8 => 125,
            _ => anyhow::bail!("Unknown polling rate code"),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MouseSleep {
    pub radio_seconds: u16,
}
impl MouseSleep {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        ensure!(
            raw.len() == 64 && raw[0] == 0xd3,
            "Invalid mouse sleep block"
        );
        Ok(Self {
            radio_seconds: u16::from_le_bytes([raw[44], raw[45]]),
        })
    }
    pub fn patch(&self, raw: &[u8]) -> Result<Vec<u8>> {
        let before = Self::parse(raw)?;
        ensure!(
            self == &before || self.radio_seconds <= 3600,
            "Сон мыши: 0–3600 секунд, 0 отключает сон"
        );
        let mut out = raw.to_vec();
        out[44..46].copy_from_slice(&self.radio_seconds.to_le_bytes());
        Ok(out)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MouseDpi {
    pub current_index: u8,
    pub count: u8,
    pub x: [u16; 8],
    pub y: [u16; 8],
    pub colors: [[u8; 3]; 8],
}
impl MouseDpi {
    pub fn patch(&self, original: &[u8]) -> Result<Vec<u8>> {
        let before = Self::parse(original)?;
        ensure!(self.count == before.count, "DPI table size cannot change");
        ensure!(self.current_index < self.count, "Invalid active DPI stage");
        ensure!(self.x == self.y, "AJ159 APEX uses linked X/Y DPI");
        ensure!(
            self.x
                .iter()
                .all(|v| *v == 0 || (50..=42000).contains(v) && v % 50 == 0),
            "DPI: 50–42000, шаг 50; 0 отключает ступень"
        );
        ensure!(
            self.x[usize::from(self.current_index)] != 0,
            "Активная ступень DPI отключена"
        );
        let mut b = original.to_vec();
        b[2] = self.current_index;
        for i in 0..8 {
            b[8 + i * 2..10 + i * 2].copy_from_slice(&self.x[i].to_le_bytes());
            b[24 + i * 2..26 + i * 2].copy_from_slice(&self.y[i].to_le_bytes());
            b[40 + i * 3..43 + i * 3].copy_from_slice(&self.colors[i]);
        }
        Ok(b)
    }
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(b.len() == 64 && b[0] == 0xd4, "Invalid mouse DPI response");
        ensure!(
            b[2] < 8 && (1..=8).contains(&b[3]),
            "Invalid DPI slot count/index"
        );
        Ok(Self {
            current_index: b[2],
            count: b[3],
            x: std::array::from_fn(|i| u16::from_le_bytes([b[8 + i * 2], b[9 + i * 2]])),
            y: std::array::from_fn(|i| u16::from_le_bytes([b[24 + i * 2], b[25 + i * 2]])),
            colors: std::array::from_fn(|i| [b[40 + i * 3], b[41 + i * 3], b[42 + i * 3]]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_lighting_uses_vendor_set_mode_without_changing_saved_state() {
        let state = [20, 12, 34, 56, 0, 7, 8, 9, 1, 5, 2, 0, 0, 0, 0, 0];
        let wire = lighting_command(&state).unwrap();
        assert_eq!(
            wire,
            [128, 12, 34, 56, 255, 7, 8, 9, 1, 5, 2, 0, 0, 0, 170, 85]
        );
        assert_eq!(state[0], 20);
        for mode in 0..=19 {
            let mut builtin = state;
            builtin[0] = mode;
            assert_eq!(lighting_command(&builtin).unwrap()[0], mode);
        }
        assert!(lighting_command(&state[..15]).is_err());
    }
    #[test]
    fn animation_limits_follow_vendor_offsets_and_old_snapshots_are_unknown() {
        let mut raw = vec![0; 48];
        raw[22..28].copy_from_slice(&[0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a]);
        let info = KeyboardInfo::parse(&raw).unwrap();
        let limits = info.animation_limits.as_ref().unwrap();
        assert_eq!(
            (limits.tft_frames, limits.gif_frames, limits.led_frames),
            (0x1234, 0x5678, 0x9abc)
        );
        let mut old = serde_json::to_value(info).unwrap();
        old.as_object_mut().unwrap().remove("animation_limits");
        assert!(
            serde_json::from_value::<KeyboardInfo>(old)
                .unwrap()
                .animation_limits
                .is_none()
        );
        let captured: Vec<u8> = vec![
            0, 0, 0, 146, 69, 12, 160, 128, 48, 1, 0, 0, 102, 1, 4, 16, 2, 18, 0, 0, 4, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let limits = KeyboardInfo::parse(&captured)
            .unwrap()
            .animation_limits
            .unwrap();
        assert_eq!(
            (limits.tft_frames, limits.gif_frames, limits.led_frames),
            (0, 0, 0)
        );
    }
    #[test]
    fn radio_notifications_are_not_confused_with_settings_or_keyboard_input() {
        assert_eq!(
            keyboard_link_event(&[0x55, 0xfc, 4]),
            Some(KeyboardLinkEvent::Disconnected)
        );
        assert_eq!(
            keyboard_link_event(&[0, 0x55, 0xfc, 5]),
            Some(KeyboardLinkEvent::Sleeping)
        );
        assert_eq!(
            keyboard_link_event(&[0xa6, 0xff, 1]),
            Some(KeyboardLinkEvent::Awake)
        );
        for bytes in [
            &[0x55, 0x10, 4][..],
            &[0x55, 0xfc, 3],
            &[0x55, 0xfc, 6],
            &[0xa6, 0xff, 2],
            &[0x55, 0xfc],
            &[],
            &[0, 0, 0],
        ] {
            assert_eq!(keyboard_link_event(bytes), None);
        }
    }
    #[test]
    fn captured_mouse_reads_keep_eight_slots_and_unusual_values() {
        #[derive(Deserialize)]
        struct Fixture {
            option0: Vec<u8>,
            option1: Vec<u8>,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/aj159-apex-read.json")).unwrap();
        let options = MouseOptions::parse(&fixture.option0).unwrap();
        assert_eq!(options.polling_hz().unwrap(), 8000);
        assert_eq!(options.debounce_ms, 2);
        let dpi = MouseDpi::parse(&fixture.option1).unwrap();
        assert_eq!(dpi.x, [400, 800, 1200, 1600, 2400, 3200, 42000, 0]);
        assert_eq!(dpi.x, dpi.y);
        assert_eq!(dpi.current_index, 2);
        assert_eq!(dpi.count, 8);
        assert_eq!(dpi.colors[0], [255, 0, 0]);
        assert!(MouseDpi::parse(&fixture.option0).is_err());
    }
    #[test]
    fn packets_match_vendor_header_and_final_flag() {
        let p = keyboard_packets(0x10, 0, 48, None, 32).unwrap();
        assert_eq!(&p[0][..8], &[0xaa, 0x10, 24, 0, 0, 0, 0, 0]);
        assert_eq!(&p[1][..8], &[0xaa, 0x10, 24, 24, 0, 0, 1, 0]);
    }
    #[test]
    fn split_write_preserves_every_byte() {
        let data: Vec<u8> = (0..512).map(|i| i as u8).collect();
        let p = keyboard_packets(0x24, 0, 512, Some(&data), 32).unwrap();
        let out: Vec<u8> = p
            .iter()
            .flat_map(|p| p[8..8 + usize::from(p[2])].iter().copied())
            .collect();
        assert_eq!(out, data);
        assert_eq!(p.len(), 22);
        assert_eq!(p.last().unwrap()[6], 1);
    }
    #[test]
    fn mismatched_responses_are_rejected() {
        let mut b = vec![0; 32];
        b[0] = 0x55;
        b[1] = 0x10;
        b[2] = 24;
        b[3] = 24;
        assert!(parse_keyboard_response(&b, 0x10, 0, 24).is_err());
        assert!(parse_keyboard_response(&b[..20], 0x10, 24, 24).is_err());
        assert!(parse_keyboard_response(&b, 0x10, 24, 24).is_ok());
    }
    #[test]
    fn hall_patch_preserves_unknown_flag_bits() {
        let raw = [1, 0xf8, 120, 0, 10, 0, 10, 0];
        let mut k = HallKey::decode(&raw, 0).unwrap();
        k.whole_travel = true;
        let out = k.patch(&raw, 0).unwrap();
        assert_eq!(out[1], 0xf9);
        assert_eq!(&out[2..], &raw[2..]);
    }
    #[test]
    fn hall_rt_enable_is_separate_from_the_whole_travel_flag() {
        let raw = [0, 0xf9, 120, 0, 0, 0, 0, 0];
        let mut k = HallKey::decode(&raw, 0).unwrap();
        assert!(!k.rapid && k.whole_travel);
        k.press = 0.12;
        k.release = 0.25;
        k.rapid = true;
        let enabled = k.patch(&raw, 0).unwrap();
        assert_eq!(enabled, [0, 0xf9, 120, 0, 12, 0, 25, 0]);
        k.rapid = false;
        assert_eq!(k.patch(&enabled, 0).unwrap(), raw);
        k.actuation = 0.01;
        assert!(k.patch(&raw, 0).is_err());
    }
}

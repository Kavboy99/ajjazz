//! AJKS procedural programs. Upload a bounded program once; no host frame loop.
//! This protocol requires the independent AJKS capability, never an OEM echo.
use crate::model::{Blend, Effect, Scene, keyboard_rows};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
mod battery;
mod preview;
pub use battery::{BatteryIndicator, battery_preview};
pub use preview::{device_preview, render_program, scene_preview};

pub const PAGE_BYTES: usize = 512;
pub const SLOT_COUNT: usize = 8;
pub fn crc(data: &[u8]) -> u16 {
    let mut n = 0xffffu16;
    for &v in data {
        n ^= u16::from(v) << 8;
        for _ in 0..8 {
            n = (n << 1) ^ if n & 0x8000 != 0 { 0x1021 } else { 0 };
        }
    }
    n
}
fn finish(p: &mut [u8]) {
    let n = crc(&p[..510]);
    p[510..512].copy_from_slice(&n.to_le_bytes());
}
fn word(p: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([p[at], p[at + 1]])
}
pub fn validate_page(p: &[u8], config: bool) -> Result<()> {
    ensure!(
        p.len() == 512 && &p[..4] == if config { b"AKCF" } else { b"AKLP" } && p[4] == 1,
        "Неверный формат AJKS"
    );
    ensure!(
        word(p, 510) == crc(&p[..510]),
        "Контрольная сумма AJKS не совпадает"
    );
    if config {
        ensure!(
            (p[5] < 8 || p[5] == 255) && (8..=248).contains(&p[7]) && p[10] <= 1,
            "Некорректные настройки AJKS"
        );
        for i in 0..3 {
            Action::decode(&p[16 + i * 4..20 + i * 4])?;
        }
        BatteryIndicator::parse(p)?;
    } else {
        ensure!(
            p[5] <= 8,
            "В памяти клавиатуры поддерживается до восьми слоёв"
        );
        for l in p[64..64 + usize::from(p[5]) * 48].as_chunks::<48>().0 {
            ensure!(
                l[0] <= 1
                    && usize::from(l[1]) < Effect::ALL.len()
                    && l[2] <= 2
                    && word(l, 10) <= 2048
                    && (16..=2048).contains(&word(l, 12)),
                "Некорректный слой AJKS"
            );
            ensure!(
                (-256..=256).contains(&(word(l, 14) as i16))
                    && (-256..=256).contains(&(word(l, 16) as i16)),
                "Некорректное направление AJKS"
            );
        }
    }
    Ok(())
}
pub fn compile(scene: &Scene) -> Result<Vec<u8>> {
    scene.validate()?;
    ensure!(
        scene.layers.len() <= 8,
        "Для памяти клавиатуры оставьте не более восьми слоёв"
    );
    let mut p = vec![0; 512];
    p[..4].copy_from_slice(b"AKLP");
    p[4] = 1;
    p[5] = scene.layers.len() as u8;
    p[7] = (scene.brightness * 255.0).round() as u8;
    let mut end = scene.name.len().min(32);
    while !scene.name.is_char_boundary(end) {
        end -= 1;
    }
    p[12..12 + end].copy_from_slice(&scene.name.as_bytes()[..end]);
    let physical: std::collections::BTreeSet<_> = keyboard_rows()
        .into_iter()
        .flatten()
        .map(|k| k.id)
        .collect();
    for (i, layer) in scene.layers.iter().enumerate() {
        ensure!(
            (0.0625..=8.0).contains(&layer.scale) && (0.0..=8.0).contains(&layer.speed),
            "Для AJKS скорость 0–8, масштаб 0.0625–8"
        );
        ensure!(
            layer.keys.iter().all(|id| physical.contains(id)),
            "Слой содержит ID вне физической раскладки AK820MAX"
        );
        let l = &mut p[64 + i * 48..112 + i * 48];
        l[0] = layer.enabled.into();
        l[1] = Effect::ALL.iter().position(|v| *v == layer.effect).unwrap() as u8;
        l[2] = match layer.blend {
            Blend::Replace => 0,
            Blend::Add => 1,
            Blend::Multiply => 2,
        };
        l[3] = (layer.opacity * 255.0).round() as u8;
        l[4..7].copy_from_slice(&layer.colors[0]);
        l[7..10].copy_from_slice(&layer.colors[1]);
        for (at, n) in [
            (10, (layer.speed * 256.0).round() as u16),
            (12, (layer.scale * 256.0).round() as u16),
            (
                14,
                (layer.angle.to_radians().cos() * 256.0).round() as i16 as u16,
            ),
            (
                16,
                (layer.angle.to_radians().sin() * 256.0).round() as i16 as u16,
            ),
            (18, (layer.phase.rem_euclid(1.0) * 256.0).round() as u16),
        ] {
            l[at..at + 2].copy_from_slice(&n.to_le_bytes());
        }
        for &id in if layer.keys.is_empty() {
            &physical
        } else {
            &layer.keys
        } {
            l[20 + usize::from(id / 8)] |= 1 << (id % 8);
        }
    }
    finish(&mut p);
    validate_page(&p, false)?;
    Ok(p)
}
// Recover an angle inside the original rounding cell. atan2 of rounded Q8
// components alone can normalize (241,88) to (240,88), changing an untouched
// program on read/edit/save. The circle intersects one of the cell's edges.
fn decoded_direction(x: i16, y: i16) -> f32 {
    let angle = (y as f32).atan2(x as f32).to_degrees();
    let matches = |a: f32| {
        (a.to_radians().cos() * 256.0).round() as i16 == x
            && (a.to_radians().sin() * 256.0).round() as i16 == y
    };
    if matches(angle) {
        return angle;
    }
    for offset in [-0.4999_f64, 0.4999] {
        let cx = f64::from(x) + offset;
        if cx.abs() <= 256.0 {
            let sy = (65536.0 - cx * cx).sqrt().copysign(f64::from(y));
            let a = sy.atan2(cx).to_degrees() as f32;
            if matches(a) {
                return a;
            }
        }
        let sy = f64::from(y) + offset;
        if sy.abs() <= 256.0 {
            let cx = (65536.0 - sy * sy).sqrt().copysign(f64::from(x));
            let a = sy.atan2(cx).to_degrees() as f32;
            if matches(a) {
                return a;
            }
        }
    }
    angle
}
pub fn edit_scene(p: &[u8]) -> Result<Scene> {
    validate_page(p, false)?;
    let layers = p[64..64 + usize::from(p[5]) * 48]
        .as_chunks::<48>()
        .0
        .iter()
        .enumerate()
        .map(|(i, l)| crate::model::Layer {
            name: format!("Слой {}", i + 1),
            enabled: l[0] != 0,
            effect: Effect::ALL[usize::from(l[1])],
            blend: Blend::ALL[usize::from(l[2])],
            keys: keyboard_rows()
                .into_iter()
                .flatten()
                .filter(|k| l[20 + usize::from(k.id / 8)] & (1 << (k.id % 8)) != 0)
                .map(|k| k.id)
                .collect(),
            colors: [[l[4], l[5], l[6]], [l[7], l[8], l[9]]],
            opacity: f32::from(l[3]) / 255.0,
            speed: f32::from(word(l, 10)) / 256.0,
            scale: f32::from(word(l, 12)) / 256.0,
            angle: decoded_direction(word(l, 14) as i16, word(l, 16) as i16),
            phase: f32::from(word(l, 18)) / 256.0,
        })
        .collect();
    let s = Scene {
        name: String::from_utf8_lossy(&p[12..44])
            .trim_end_matches('\0')
            .into(),
        brightness: f32::from(p[7]) / 255.0,
        layers,
        dock_layers: vec![],
        ..Scene::default()
    };
    s.validate()?;
    Ok(s)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Native,
    Media(u16),
    Brightness(i16),
    Mode(i16),
    Toggle,
    Speed(i16),
    Pause,
}
impl Action {
    pub const CHOICES: [Self; 15] = [
        Self::Native,
        Self::Media(0xe9),
        Self::Media(0xea),
        Self::Media(0xe2),
        Self::Media(0xcd),
        Self::Media(0xb5),
        Self::Media(0xb6),
        Self::Brightness(1),
        Self::Brightness(-1),
        Self::Mode(1),
        Self::Mode(-1),
        Self::Toggle,
        Self::Speed(1),
        Self::Speed(-1),
        Self::Pause,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Штатное действие",
            Self::Media(0xe9) => "Громкость +",
            Self::Media(0xea) => "Громкость −",
            Self::Media(0xe2) => "Выключить звук",
            Self::Media(0xcd) => "Воспроизведение / пауза",
            Self::Media(0xb5) => "Следующий трек",
            Self::Media(0xb6) => "Предыдущий трек",
            Self::Media(_) => "Медиа-команда",
            Self::Brightness(1) => "Яркость +",
            Self::Brightness(_) => "Яркость −",
            Self::Mode(1) => "Следующий свой режим",
            Self::Mode(_) => "Предыдущий свой режим",
            Self::Toggle => "Подсветка вкл. / выкл.",
            Self::Speed(1) => "Скорость +",
            Self::Speed(_) => "Скорость −",
            Self::Pause => "Заморозить / продолжить эффект",
        }
    }
    fn encode(self) -> Result<[u8; 4]> {
        let (kind, param) = match self {
            Self::Native => (0, 0),
            Self::Media(n) => (1, n),
            Self::Brightness(n) => (2, n as u16),
            Self::Mode(n) => (3, n as u16),
            Self::Toggle => (4, 0),
            Self::Speed(n) => (5, n as u16),
            Self::Pause => (6, 0),
        };
        let p = [kind, 0, param as u8, (param >> 8) as u8];
        Self::decode(&p)?;
        Ok(p)
    }
    fn decode(p: &[u8]) -> Result<Self> {
        ensure!(
            p.len() == 4 && p[1] == 0,
            "Некорректное назначение энкодера"
        );
        let n = word(p, 2);
        if [2, 3, 5].contains(&p[0]) {
            ensure!(
                [1, 65535].contains(&n),
                "Шаг энкодера должен быть +1 или −1"
            );
        }
        Ok(match p[0] {
            0 => Self::Native,
            1 => {
                ensure!((1..=0x3ff).contains(&n), "Неверный consumer usage");
                Self::Media(n)
            }
            2 => Self::Brightness(n as i16),
            3 => Self::Mode(n as i16),
            4 => Self::Toggle,
            5 => Self::Speed(n as i16),
            6 => Self::Pause,
            _ => anyhow::bail!("Неизвестное назначение энкодера"),
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub selected: Option<u8>,
    pub brightness: u8,
    pub speed: u8,
    pub paused: bool,
    pub encoder: [Action; 3],
    #[serde(default)]
    pub battery: BatteryIndicator,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            selected: None,
            brightness: 255,
            speed: 128,
            paused: false,
            encoder: [Action::Native; 3],
            battery: BatteryIndicator::default(),
        }
    }
}
impl Config {
    pub fn parse(p: &[u8]) -> Result<Self> {
        validate_page(p, true)?;
        Ok(Self {
            selected: (p[5] != 255).then_some(p[5]),
            brightness: p[6],
            speed: p[7],
            paused: p[10] != 0,
            battery: BatteryIndicator::parse(p)?,
            encoder: [
                Action::decode(&p[16..20])?,
                Action::decode(&p[20..24])?,
                Action::decode(&p[24..28])?,
            ],
        })
    }
    pub fn page(&self) -> Result<Vec<u8>> {
        let mut p = vec![0; 512];
        p[..4].copy_from_slice(b"AKCF");
        p[4] = 1;
        p[5] = self.selected.unwrap_or(255);
        p[6] = self.brightness;
        p[7] = self.speed;
        p[10] = self.paused.into();
        for (i, a) in self.encoder.iter().enumerate() {
            p[16 + i * 4..20 + i * 4].copy_from_slice(&a.encode()?);
        }
        self.battery.write(&mut p)?;
        finish(&mut p);
        validate_page(&p, true)?;
        Ok(p)
    }
}
pub fn request(op: u8, seq: u8, payload: &[u8]) -> Result<Vec<u8>> {
    ensure!(op <= 11 && payload.len() <= 48, "Неверная команда AJKS");
    let mut p = vec![0; 64];
    p[..8].copy_from_slice(&[0xaa, 0x70, b'A', b'K', 1, op, seq, payload.len() as u8]);
    p[8..8 + payload.len()].copy_from_slice(payload);
    let n = crc(&p[..62]);
    p[62..].copy_from_slice(&n.to_le_bytes());
    Ok(p)
}
pub fn response(p: &[u8], op: u8, seq: u8) -> Result<Vec<u8>> {
    let p = if p.len() == 65 && p[0] == 0 {
        &p[1..]
    } else {
        p
    };
    ensure!(
        p.len() == 64 && p[..7] == [0x55, 0x70, b'K', b'S', 1, op, seq],
        "Прошивка AJKS не установлена либо получен чужой ответ"
    );
    ensure!(word(p, 62) == crc(&p[..62]), "Повреждённый ответ AJKS");
    ensure!(p[7] == 0, "AJKS: команда отклонена, код {}", p[7]);
    Ok(p[8..56].to_vec())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryState {
    pub config: Config,
    pub scenes: Vec<Option<Vec<u8>>>,
    pub firmware: String,
    pub build_id: String,
    #[serde(default)]
    pub runtime: Option<Runtime>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Runtime {
    pub frame: u32,
    pub millis: u32,
    pub selected: Option<u8>,
    pub brightness: u8,
    pub speed: u8,
    pub paused: bool,
    pub color_transfer: u8,
    #[serde(default)]
    pub effect_count: u8,
    #[serde(default)]
    pub battery_supported: bool,
    #[serde(default)]
    pub battery_percent: Option<u8>,
    #[serde(default)]
    pub battery_visible: bool,
}
impl Runtime {
    pub fn parse(p: &[u8]) -> Result<Option<Self>> {
        ensure!(
            p.len() >= 48 && &p[..6] == b"AJKS\x01\x08",
            "Неверное состояние подсветки"
        );
        if &p[32..36] != b"CLK2" {
            return Ok(None);
        }
        ensure!(
            p[37] == 20
                && p[36] == 1
                && p[8] <= 1
                && (8..=248).contains(&p[10])
                && (p[6] == 255 || p[6] < 8 && p[9] & (1 << p[6]) != 0),
            "Неизвестная версия синхронизации подсветки"
        );
        Ok(Some(Self {
            frame: u32::from_le_bytes(p[24..28].try_into()?),
            millis: u32::from_le_bytes(p[28..32].try_into()?),
            selected: (p[6] != 255).then_some(p[6]),
            brightness: p[7],
            speed: p[10],
            paused: p[8] != 0,
            color_transfer: p[36],
            effect_count: if p[38] == 0 { 13 } else { p[38] },
            battery_supported: p[39] == 1,
            battery_percent: (p[39] == 1 && p[40] <= 100).then_some(p[40]),
            battery_visible: p[39] == 1 && p[41] == 1,
        }))
    }
    pub fn frame_after(&self, elapsed_ms: u64) -> u32 {
        self.frame
            .wrapping_add(if self.paused || self.selected.is_none() {
                0
            } else {
                (elapsed_ms / 20) as u32
            })
    }
}
impl MemoryState {
    pub fn name(&self, slot: usize) -> Option<String> {
        self.scenes.get(slot)?.as_ref().map(|p| {
            String::from_utf8_lossy(&p[12..44])
                .trim_end_matches('\0')
                .to_owned()
        })
    }
}
#[derive(Clone)]
pub enum Operation {
    Read,
    Upload(u8, Scene),
    Select(Option<u8>),
    Configure(Config),
    Delete(u8),
    Save,
    /// Save a program, activate it and persist the choice in one user action.
    Publish(u8, Scene),
    /// Activate an existing program (or native lighting), then persist the choice.
    Activate(Option<u8>),
    /// Merge only encoder assignments into a freshly read runtime configuration.
    Encoder([Action; 3], BatteryIndicator),
}

#[derive(Debug)]
enum WorkflowStep {
    Program(u8, Vec<u8>),
    Select(Option<u8>),
    Config(Config),
    Save,
}
pub fn same_program(a: &[u8], b: &[u8]) -> bool {
    a.len() == 512 && b.len() == 512 && a[..8] == b[..8] && a[10..510] == b[10..510]
}
fn workflow(operation: &Operation, before: &MemoryState) -> Result<Vec<WorkflowStep>> {
    ensure!(before.scenes.len() == 8, "Неверное число режимов");
    let mut steps = Vec::new();
    match operation {
        Operation::Publish(slot, scene) => {
            ensure!(*slot < 8, "Неверный слот");
            let page = compile(scene)?;
            if !before.scenes[usize::from(*slot)]
                .as_ref()
                .is_some_and(|p| same_program(p, &page))
            {
                steps.push(WorkflowStep::Program(*slot, page));
            }
            steps.push(WorkflowStep::Select(Some(*slot)));
            steps.push(WorkflowStep::Save);
        }
        Operation::Activate(slot) => {
            ensure!(
                slot.is_none_or(|s| s < 8 && before.scenes[usize::from(s)].is_some()),
                "Режим отсутствует в памяти"
            );
            steps.push(WorkflowStep::Select(*slot));
            steps.push(WorkflowStep::Save);
        }
        Operation::Encoder(actions, battery) => {
            let mut config = before.config.clone();
            if config.encoder != *actions || config.battery != *battery {
                config.encoder = *actions;
                config.battery = battery.clone();
                config.page()?;
                steps.push(WorkflowStep::Config(config));
            }
        }
        _ => {}
    }
    Ok(steps)
}

#[cfg(windows)]
struct Client {
    keyboard: crate::hardware::Keyboard,
    seq: std::cell::Cell<u8>,
    firmware: String,
}
#[cfg(windows)]
impl Client {
    fn open() -> Result<Self> {
        let keyboard = crate::hardware::Keyboard::open_usb()?;
        let info = keyboard.read_info()?;
        ensure!(
            info.version == "1.36"
                && info.manufacturer == 0x0166
                && info.vid == 0x0c45
                && info.pid == 0x80a0
                && info.firmware_status == 0,
            "Для автономных режимов нужна прошивка клавиатуры AJKS на базе 1.36. Сейчас {}",
            info.version
        );
        let c = Self {
            keyboard,
            seq: std::cell::Cell::new(0),
            firmware: info.version,
        };
        let p = c.call(0, &[])?;
        ensure!(&p[..6] == b"AJKS\x01\x08", "Неизвестные возможности AJKS");
        Ok(c)
    }
    fn call(&self, op: u8, p: &[u8]) -> Result<Vec<u8>> {
        let seq = self.seq.get().wrapping_add(1);
        self.seq.set(seq);
        self.keyboard.memory_exchange(&request(op, seq, p)?)
    }
    fn read(&self) -> Result<MemoryState> {
        let info = self.call(0, &[])?;
        let mut config = vec![];
        for i in 0..16 {
            config.extend_from_slice(&self.call(6, &[i])?[..32]);
        }
        let config = Config::parse(&config)?;
        let mut scenes = vec![];
        for slot in 0..8 {
            if info[9] & (1 << slot) != 0 {
                let mut p = vec![];
                for i in 0..16 {
                    p.extend_from_slice(&self.call(1, &[slot, i])?[..32]);
                }
                validate_page(&p, false)?;
                scenes.push(Some(p));
            } else {
                scenes.push(None);
            }
        }
        let latest = self.call(0, &[])?;
        let runtime = Runtime::parse(&latest)?;
        let mut config = config;
        if let Some(rt) = &runtime {
            config.selected = rt.selected;
            config.brightness = rt.brightness;
            config.speed = rt.speed;
            config.paused = rt.paused;
        }
        Ok(MemoryState {
            config,
            scenes,
            firmware: self.firmware.clone(),
            build_id: info[16..24].iter().map(|v| format!("{v:02x}")).collect(),
            runtime,
        })
    }
    fn upload(&self, slot: Option<u8>, p: &[u8]) -> Result<()> {
        validate_page(p, slot.is_none())?;
        if let Some(s) = slot {
            ensure!(s < 8, "Неверный слот");
            self.call(2, &[s])?;
        } else {
            self.call(7, &[])?;
        }
        for i in 0..16u8 {
            let mut data = vec![];
            if let Some(s) = slot {
                data.push(s);
            }
            data.push(i);
            data.extend_from_slice(&p[usize::from(i) * 32..usize::from(i + 1) * 32]);
            self.call(if slot.is_some() { 3 } else { 8 }, &data)?;
        }
        self.call(
            if slot.is_some() { 4 } else { 9 },
            &slot.into_iter().collect::<Vec<_>>(),
        )?;
        Ok(())
    }
}
/// Read-only clock/status query; no pixel upload and no persistent write.
pub fn runtime() -> Result<Option<Runtime>> {
    #[cfg(windows)]
    {
        let client = Client::open()?;
        Runtime::parse(&client.call(0, &[])?)
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
/// Opportunistic GET only. A user operation owns the mailbox first; skipping
/// this optional observation is not a connection error or a UI operation.
pub fn runtime_background() -> Result<Option<Runtime>> {
    #[cfg(windows)]
    {
        let Some(keyboard) = crate::hardware::Keyboard::open_usb_background()? else {
            return Ok(None);
        };
        Runtime::parse(&keyboard.memory_exchange(&request(0, 1, &[])?)?)
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}
/// Every mutation has a readback and a local backup of all custom programs.
pub fn run(operation: Operation, backup: &std::path::Path) -> Result<MemoryState> {
    #[cfg(windows)]
    {
        let c = Client::open()?;
        let before = c.read()?;
        if matches!(operation, Operation::Read) {
            return Ok(before);
        }
        match &operation {
            Operation::Upload(_, scene) | Operation::Publish(_, scene) => ensure!(
                scene.layers.iter().all(|l| Effect::ALL
                    .iter()
                    .position(|e| *e == l.effect)
                    .unwrap()
                    < usize::from(before.runtime.as_ref().map_or(13, |r| r.effect_count))),
                "Для этого эффекта обновите прошивку клавиатуры"
            ),
            Operation::Encoder(_, battery) if *battery != before.config.battery => ensure!(
                before.runtime.as_ref().is_some_and(|r| r.battery_supported),
                "Для индикатора заряда обновите прошивку клавиатуры"
            ),
            _ => {}
        }
        let steps = workflow(&operation, &before)?;
        if matches!(operation, Operation::Encoder(..)) && steps.is_empty() {
            return Ok(before);
        }
        crate::storage::backup(backup, "ak820max-ajks", &before)?;
        match &operation {
            Operation::Upload(slot, scene) => c.upload(Some(*slot), &compile(scene)?)?,
            Operation::Select(slot) => {
                ensure!(slot.is_none_or(|s| s < 8), "Неверный слот");
                c.call(5, &[slot.unwrap_or(255)])?;
            }
            Operation::Configure(config) => c.upload(None, &config.page()?)?,
            Operation::Delete(slot) => {
                ensure!(*slot < 8, "Неверный слот");
                c.call(10, &[*slot])?;
            }
            Operation::Save => {
                c.call(11, &[])?;
            }
            Operation::Read => {}
            Operation::Publish(..) | Operation::Activate(_) | Operation::Encoder(..) => {
                for step in steps {
                    match step {
                        WorkflowStep::Program(slot, p) => c.upload(Some(slot), &p)?,
                        WorkflowStep::Select(slot) => {
                            c.call(5, &[slot.unwrap_or(255)])?;
                        }
                        WorkflowStep::Config(config) => c.upload(None, &config.page()?)?,
                        WorkflowStep::Save => {
                            c.call(11, &[])?;
                        }
                    }
                }
            }
        }
        let after = c.read()?;
        let changed_slot = match &operation {
            Operation::Publish(slot, _) | Operation::Upload(slot, _) | Operation::Delete(slot) => {
                Some(usize::from(*slot))
            }
            _ => None,
        };
        match operation {
            Operation::Upload(slot, scene) => {
                let expected = compile(&scene)?;
                let got = after.scenes[usize::from(slot)]
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Режим не сохранился"))?;
                ensure!(
                    expected[..8] == got[..8] && expected[10..510] == got[10..510],
                    "Проверка записанного режима не прошла"
                );
            }
            Operation::Select(slot) => {
                ensure!(after.config.selected == slot, "Режим не переключился")
            }
            Operation::Configure(config) => ensure!(
                after.config.page()? == config.page()?,
                "Настройки энкодера не прошли проверку"
            ),
            Operation::Delete(slot) => {
                ensure!(after.scenes[usize::from(slot)].is_none(), "Режим не удалён")
            }
            Operation::Publish(slot, scene) => {
                ensure!(
                    after.config.selected == Some(slot),
                    "Режим записан, но выбор не подтверждён"
                );
                ensure!(
                    after.scenes[usize::from(slot)]
                        .as_ref()
                        .is_some_and(|p| same_program(p, &compile(&scene).unwrap_or_default())),
                    "Режим не прошёл проверку"
                );
            }
            Operation::Activate(slot) => {
                ensure!(after.config.selected == slot, "Выбор режима не подтверждён")
            }
            Operation::Encoder(actions, battery) => ensure!(
                after.config.encoder == actions && after.config.battery == battery,
                "Назначения энкодера не подтверждены"
            ),
            _ => {}
        }
        for slot in 0..8 {
            if Some(slot) != changed_slot {
                ensure!(
                    before.scenes[slot] == after.scenes[slot],
                    "Другой режим изменился во время записи; копия сохранена"
                );
            }
        }
        Ok(after)
    }
    #[cfg(not(windows))]
    {
        let _ = (operation, backup);
        anyhow::bail!("Windows only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_quantized_directions_does_not_reencode_an_untouched_program() {
        for step in 0..36000 {
            let angle = step as f32 / 100.0;
            let x = (angle.to_radians().cos() * 256.0).round() as i16;
            let y = (angle.to_radians().sin() * 256.0).round() as i16;
            let decoded = decoded_direction(x, y);
            assert_eq!(
                (
                    (decoded.to_radians().cos() * 256.0).round() as i16,
                    (decoded.to_radians().sin() * 256.0).round() as i16
                ),
                (x, y),
                "Direction {angle}"
            );
        }
    }
    #[test]
    fn compiled_runtime_reports_clock_and_rejects_unknown_period_or_legacy_echo() {
        let p: Vec<u8> =
            serde_json::from_str(include_str!("../tests/fixtures/keyboard-runtime.json")).unwrap();
        let rt = Runtime::parse(&p).unwrap().unwrap();
        assert_eq!(
            (
                rt.frame,
                rt.millis,
                rt.speed,
                rt.brightness,
                rt.color_transfer
            ),
            (50, 1000, 128, 255, 1)
        );
        assert_eq!(rt.frame_after(240), 62);
        let mut paused = rt.clone();
        paused.paused = true;
        assert_eq!(paused.frame_after(10000), 50);
        paused.paused = false;
        paused.frame = u32::MAX - 2;
        assert_eq!(paused.frame_after(100), 2);
        let mut legacy = p.clone();
        legacy[32..36].fill(0);
        assert!(Runtime::parse(&legacy).unwrap().is_none());
        let mut corrupt = p;
        corrupt[37] = 1;
        assert!(Runtime::parse(&corrupt).is_err());
        assert!(Runtime::parse(&[0; 48]).is_err());
    }
    fn workflow_state() -> MemoryState {
        MemoryState {
            config: Config {
                selected: Some(2),
                brightness: 137,
                speed: 64,
                paused: true,
                ..Default::default()
            },
            scenes: vec![None; 8],
            firmware: "1.36".into(),
            build_id: String::new(),
            runtime: None,
        }
    }
    #[test]
    fn encoder_workflow_preserves_live_knob_settings_and_avoids_duplicate_flash_writes() {
        let before = workflow_state();
        let actions = [
            Action::Media(0xea),
            Action::Media(0xe9),
            Action::Media(0xcd),
        ];
        let steps = workflow(
            &Operation::Encoder(actions, before.config.battery.clone()),
            &before,
        )
        .unwrap();
        let [WorkflowStep::Config(config)] = steps.as_slice() else {
            panic!("Only encoder config should be written")
        };
        assert_eq!(config.selected, Some(2));
        assert_eq!(
            (config.brightness, config.speed, config.paused),
            (137, 64, true)
        );
        assert_eq!(config.encoder, actions);
        assert!(
            workflow(
                &Operation::Encoder(before.config.encoder, before.config.battery.clone()),
                &before
            )
            .unwrap()
            .is_empty()
        );
    }
    #[test]
    fn publish_orders_program_before_activation_and_persistence_and_skips_identical_program() {
        let mut before = workflow_state();
        let scene = Scene::default();
        let steps = workflow(&Operation::Publish(3, scene.clone()), &before).unwrap();
        assert!(matches!(
            steps.as_slice(),
            [
                WorkflowStep::Program(3, _),
                WorkflowStep::Select(Some(3)),
                WorkflowStep::Save
            ]
        ));
        let mut stored = compile(&scene).unwrap();
        stored[8] = 43;
        finish(&mut stored);
        before.scenes[3] = Some(stored);
        assert!(matches!(
            workflow(&Operation::Publish(3, scene), &before)
                .unwrap()
                .as_slice(),
            [WorkflowStep::Select(Some(3)), WorkflowStep::Save]
        ));
        assert!(workflow(&Operation::Activate(Some(7)), &before).is_err());
        assert!(workflow(&Operation::Publish(8, Scene::default()), &before).is_err());
    }
    #[test]
    fn stored_program_opens_in_editor_and_bad_signed_direction_is_rejected() {
        let scene = Scene::default();
        let mut p = compile(&scene).unwrap();
        let restored = edit_scene(&p).unwrap();
        assert_eq!(restored.name, scene.name);
        assert_eq!(restored.layers.len(), 2);
        assert_eq!(restored.layers[1].keys, scene.layers[1].keys);
        assert_eq!(restored.layers[0].colors, scene.layers[0].colors);
        p[78..80].copy_from_slice(&i16::MIN.to_le_bytes());
        finish(&mut p);
        assert!(validate_page(&p, false).is_err());
    }
    #[test]
    fn program_preserves_masks_colors_and_utf8() {
        let scene = Scene::default();
        let p = compile(&scene).unwrap();
        validate_page(&p, false).unwrap();
        assert_eq!(p[5], 2);
        assert_eq!(&p[68..74], &[38, 225, 190, 111, 76, 240]);
        assert!(std::str::from_utf8(&p[12..44]).is_ok());
        let l = &p[112..160];
        assert!(l[20 + 34 / 8] & (1 << (34 % 8)) != 0);
        assert_eq!(l[20] & 1, 0);
    }
    #[test]
    fn oem_echo_is_never_accepted_as_ajks() {
        let mut p = request(0, 2, &[]).unwrap();
        p[0] = 0x55;
        assert!(response(&p, 0, 2).is_err());
    }
    #[test]
    fn encoder_roundtrip_and_invalid_steps() {
        let mut c = Config {
            encoder: [Action::Mode(1), Action::Brightness(-1), Action::Media(0xcd)],
            ..Default::default()
        };
        assert_eq!(
            Config::parse(&c.page().unwrap()).unwrap().encoder,
            c.encoder
        );
        c.encoder[0] = Action::Mode(0);
        assert!(c.page().is_err());
    }
    #[test]
    fn no_silent_layer_or_parameter_truncation() {
        let mut s = Scene::default();
        s.layers = vec![s.layers[0].clone(); 9];
        assert!(compile(&s).is_err());
        s.layers.truncate(1);
        s.layers[0].scale = 9.0;
        assert!(compile(&s).is_err());
    }
    #[test]
    fn crc_and_sequence_protect_replies() {
        assert_eq!(crc(b"123456789"), 0x29b1);
        let mut p = request(0, 3, &[]).unwrap();
        p[..7].copy_from_slice(&[0x55, 0x70, b'K', b'S', 1, 0, 3]);
        p[7] = 0;
        let n = crc(&p[..62]);
        p[62..].copy_from_slice(&n.to_le_bytes());
        assert!(response(&p, 0, 3).is_ok());
        assert!(response(&p, 0, 4).is_err());
        p[10] ^= 1;
        assert!(response(&p, 0, 3).is_err());
    }
}

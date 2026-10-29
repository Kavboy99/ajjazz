//! Host animation using the same 0x24 color tables as Lighting Lab.
//! No inference about RAM-only writes or flash endurance is made here.
use crate::{
    hardware::{self, LightingIo, LightingState, Snapshot},
    model::Scene,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    #[default]
    Usb,
    Radio,
}
impl Channel {
    /// Prefer the actual wired control interface over an attached receiver.
    pub fn detect(endpoints: &[hardware::Endpoint]) -> Option<Self> {
        if endpoints
            .iter()
            .any(|e| e.vid == 0x0c45 && e.pid == 0x80a0 && e.usage_page == 0xff68 && e.usage == 97)
        {
            Some(Self::Usb)
        } else if endpoints
            .iter()
            .any(|e| e.vid == 0x0c45 && e.pid == 0xfefc && e.usage_page == 0xff80 && e.usage == 1)
        {
            Some(Self::Radio)
        } else {
            None
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Radio => "2.4 GHz",
        }
    }
    fn work_mode(self) -> u8 {
        match self {
            Self::Usb => 15,
            Self::Radio => 2,
        }
    }
    pub fn minimum_interval(self) -> u64 {
        16
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub frame: u64,
    pub scene_time: f32,
    pub transfer_ms: u64,
    pub write_ms: u64,
    pub frame_ms: u64,
    pub verified: bool,
    pub actual_fps: f32,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub backup_path: PathBuf,
    pub configuration_path: PathBuf,
    pub channel: Channel,
    pub frames: u64,
    pub restored: bool,
}

trait AnimationIo: LightingIo {
    fn snapshot(&self) -> Result<Snapshot>;
    fn write_frame(&self, bytes: &[u8]) -> Result<()>;
    fn read_frame(&self) -> Result<Vec<u8>>;
    fn write_delta(&self, _previous: &[u8], bytes: &[u8]) -> Result<()> {
        self.write_frame(bytes)
    }
}
#[cfg(windows)]
impl AnimationIo for hardware::Keyboard {
    fn snapshot(&self) -> Result<Snapshot> {
        self.snapshot()
    }
    fn write_frame(&self, bytes: &[u8]) -> Result<()> {
        self.write_animation_table(bytes)
    }
    fn read_frame(&self) -> Result<Vec<u8>> {
        self.read_animation_table()
    }
    fn write_delta(&self, previous: &[u8], bytes: &[u8]) -> Result<()> {
        self.write_animation_delta(previous, bytes)
    }
}

pub fn play(
    scene: &Scene,
    channel: Channel,
    interval_ms: u64,
    repeat: bool,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<Receipt> {
    play_with_previous(scene, channel, interval_ms, repeat, cancel, progress, None)
}

#[allow(clippy::too_many_arguments)]
pub fn play_with_previous(
    scene: &Scene,
    channel: Channel,
    interval_ms: u64,
    repeat: bool,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
    previous_command: Option<&[u8]>,
) -> Result<Receipt> {
    scene.validate()?;
    ensure!(
        (channel.minimum_interval()..=5000).contains(&interval_ms),
        "Интервал кадров вне диапазона"
    );
    #[cfg(windows)]
    {
        let device = match channel {
            Channel::Usb => hardware::Keyboard::open_usb()?,
            Channel::Radio => hardware::Keyboard::open()?,
        };
        play_on(
            &device,
            scene,
            channel,
            Duration::from_millis(interval_ms),
            repeat,
            cancel,
            &hardware::backup_directory()?,
            progress,
            previous_command,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (repeat, cancel, progress);
        anyhow::bail!("Windows only")
    }
}

#[allow(clippy::too_many_arguments)]
fn play_on(
    device: &impl AnimationIo,
    scene: &Scene,
    channel: Channel,
    interval: Duration,
    repeat: bool,
    cancel: &AtomicBool,
    directory: &Path,
    mut progress: impl FnMut(Progress),
    previous_command: Option<&[u8]>,
) -> Result<Receipt> {
    scene.validate()?;
    let before = device.snapshot()?;
    hardware::supported_keyboard_identity(&before.info)?;
    ensure!(
        before.info.work_mode == channel.work_mode(),
        "Клавиатура работает в другом режиме подключения"
    );
    let state = LightingState {
        lighting: before.blocks["lighting"].clone(),
        custom_rgb: before.blocks["custom_rgb"].clone(),
    };
    let working = animation_base(&state)?;
    let restore_effect = if let Some(command) = previous_command {
        ensure!(
            crate::protocol::native_effect_readback_matches(&state.lighting, command),
            "Режим изменился перед запуском; прочитайте настройки заново"
        );
        command.to_vec()
    } else {
        state.lighting.clone()
    };
    let backup = AnimationBackup {
        schema: 3,
        before: before.clone(),
        restore_effect: restore_effect.clone(),
    };
    let backup_path = crate::storage::backup(directory, "ak820max-animation-lighting", &backup)?;
    let configuration_path =
        crate::storage::backup(directory, "ak820max-animation-configuration", &before)?;
    ensure!(
        device.info()?.profile == before.info.profile
            && device.read_frame()? == state.custom_rgb
            && crate::protocol::lighting_equivalent(&device.effect()?, &state.lighting),
        "Настройки изменились перед воспроизведением"
    );
    let mut frames = 0;
    let mut mutated = false;
    let played = (|| -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Mode 128 of the old editor is normalized by firmware 1.30 to mode 20.
        let mut effect = crate::protocol::static_lighting(&state.lighting)?;
        effect[9] = 5; // Brightness is applied to rendered RGB, once.
        if !crate::protocol::lighting_equivalent(&state.lighting, &effect) {
            mutated = true;
            device
                .write_effect(&effect)
                .context("Выбор режима сохранённых цветов")?;
            // Give the mode transition the same settling period as ordinary
            // configuration writes before starting a multi-report RGB table.
            std::thread::sleep(Duration::from_millis(200));
            ensure!(
                crate::protocol::lighting_equivalent(&device.effect()?, &effect),
                "Не включился пользовательский режим"
            );
        }
        let start = Instant::now();
        let mut previous = state.custom_rgb.clone();
        let mut last_verified = None::<Instant>;
        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let elapsed = start.elapsed().as_secs_f32();
            if !repeat && elapsed >= scene.duration {
                break;
            }
            let tick_start = Instant::now();
            ensure!(
                device.info()?.profile == before.info.profile,
                "Профиль изменился во время анимации"
            );
            let scene_time = elapsed.rem_euclid(scene.duration);
            let mut colors = crate::lighting::keyboard_frame(scene, scene_time, &[]);
            let active: Vec<_> = scene.layers.iter().filter(|layer| layer.enabled).collect();
            if !active.is_empty() && active.iter().all(|layer| !layer.keys.is_empty()) {
                colors.retain(|(id, _)| active.iter().any(|layer| layer.keys.contains(id)));
            }
            let target = hardware::frame_target(&working, &colors)?;
            let frame_start = Instant::now();
            let mut write_ms = 0;
            let mut verified = false;
            if target.custom_rgb != previous {
                mutated = true;
                device
                    .write_delta(&previous, &target.custom_rgb)
                    .context("Передача RGB-кадра")?;
                write_ms = frame_start.elapsed().as_millis() as u64;
                // Every SET still waits for its matching ACK. Full table reads
                // validate the first frame and at least once per second instead
                // of blocking every frame. Stop verifies the complete config.
                if last_verified.is_none_or(|when| when.elapsed() >= Duration::from_secs(1)) {
                    ensure!(
                        device.read_frame().context("Проверка RGB-кадра")? == target.custom_rgb,
                        "Цветовая таблица кадра не прошла проверку"
                    );
                    last_verified = Some(Instant::now());
                    verified = true;
                }
                previous = target.custom_rgb;
                frames += 1;
            }
            progress(Progress {
                frame: frames,
                scene_time,
                transfer_ms: frame_start.elapsed().as_millis() as u64,
                write_ms,
                frame_ms: tick_start.elapsed().max(interval).as_millis() as u64,
                verified,
                actual_fps: frames as f32 / start.elapsed().as_secs_f32().max(0.001),
            });
            // No frame queue or catch-up bursts; sample scene time after each transfer.
            let next = tick_start + interval;
            while Instant::now() < next && !cancel.load(Ordering::Relaxed) {
                std::thread::sleep(
                    next.saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(20)),
                );
            }
        }
        Ok(())
    })();
    let restored = (|| -> Result<()> {
        if !mutated {
            return Ok(());
        }
        restore_on(device, &backup)?;
        Ok(())
    })();
    let record = serde_json::json!({"channel":channel,"frames":frames,"restored":restored.is_ok(),"play_error":played.as_ref().err().map(|e|format!("{e:#}")),"restore_error":restored.as_ref().err().map(|e|format!("{e:#}"))});
    crate::storage::write_json_atomic(&configuration_path.with_extension("result.json"), &record)?;
    if let Err(error) = restored {
        anyhow::bail!(
            "{error:#}. Восстановление не подтверждено. Копия: {}. Ошибка воспроизведения: {:?}",
            configuration_path.display(),
            played.err()
        );
    }
    if let Err(error) = played {
        anyhow::bail!(
            "{error:#}. Исходная подсветка восстановлена и проверена. Копия: {}",
            backup_path.display()
        );
    }
    Ok(Receipt {
        backup_path,
        configuration_path,
        channel,
        frames,
        restored: true,
    })
}

/// Only the known factory-empty representation may omit matrix IDs.
/// The raw backup is preserved; IDs are generated in the working copy only.
fn animation_base(state: &LightingState) -> Result<LightingState> {
    ensure!(state.lighting.len() == 16, "Неверный блок подсветки");
    let mut base = state.clone();
    if base.custom_rgb.len() == 512 && base.custom_rgb.iter().all(|&b| b == 0) {
        base.custom_rgb = crate::protocol::custom_rgb(&[[0; 3]; 128])?;
    }
    base.validate()?;
    Ok(base)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct AnimationBackup {
    schema: u32,
    before: Snapshot,
    restore_effect: Vec<u8>,
}
impl AnimationBackup {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 3 && self.before.schema == 1,
            "Неверная версия копии анимации"
        );
        hardware::supported_lighting_revision(&self.before.info)?;
        let state = LightingState {
            lighting: self
                .before
                .blocks
                .get("lighting")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Нет режима в копии"))?,
            custom_rgb: self
                .before
                .blocks
                .get("custom_rgb")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Нет цветов в копии"))?,
        };
        animation_base(&state)?;
        ensure!(
            crate::protocol::native_effect_readback_matches(&state.lighting, &self.restore_effect),
            "Режим восстановления не соответствует копии"
        );
        Ok(())
    }
}
fn restore_on(device: &impl AnimationIo, backup: &AnimationBackup) -> Result<()> {
    backup.validate()?;
    let info = device.info()?;
    hardware::supported_keyboard_identity(&info)?;
    ensure!(
        info.profile == backup.before.info.profile
            && info.work_mode == backup.before.info.work_mode,
        "Выбран другой профиль или подключение; возврат остановлен"
    );
    let rgb_error = device.write_rgb(&backup.before.blocks["custom_rgb"]).err();
    let mode_error = device.write_effect(&backup.restore_effect).err();
    let after = device.snapshot()?;
    ensure!(
        after.info.profile == backup.before.info.profile
            && crate::protocol::native_effect_readback_matches(
                &after.blocks["lighting"],
                &backup.restore_effect
            )
            && backup
                .before
                .blocks
                .iter()
                .filter(|(key, _)| key.as_str() != "lighting")
                .all(|(key, bytes)| after.blocks.get(key) == Some(bytes)),
        "Исходные настройки не совпали: RGB={rgb_error:?}, режим={mode_error:?}"
    );
    Ok(())
}

/// Manual recovery follows the same profile guards as Stop and graceful exit.
pub(crate) fn restore_backup(path: &Path) -> Result<hardware::LightingReceipt> {
    let backup: AnimationBackup = crate::storage::read_json(path, 64 * 1024)?;
    backup.validate()?;
    #[cfg(windows)]
    {
        let device = if backup.before.info.work_mode == 15 {
            hardware::Keyboard::open_usb()?
        } else {
            hardware::Keyboard::open()?
        };
        let current = device.snapshot()?;
        let current_backup = AnimationBackup {
            schema: 3,
            restore_effect: current.blocks["lighting"].clone(),
            before: current,
        };
        current_backup.validate()?;
        ensure!(
            current_backup.before.info.profile == backup.before.info.profile,
            "Выбран другой профиль клавиатуры"
        );
        let undo = crate::storage::backup(
            &hardware::backup_directory()?,
            "ak820max-animation-recovery",
            &current_backup,
        )?;
        let mut target = backup.clone();
        // Preserve unrelated settings changed since the historical backup.
        target.before.blocks = current_backup.before.blocks.clone();
        target
            .before
            .blocks
            .insert("lighting".into(), backup.before.blocks["lighting"].clone());
        target.before.blocks.insert(
            "custom_rgb".into(),
            backup.before.blocks["custom_rgb"].clone(),
        );
        if let Err(error) = restore_on(&device, &target) {
            let rollback = restore_on(&device, &current_backup);
            anyhow::bail!(
                "{error:#}; возврат перед восстановлением: {rollback:?}. Копия: {}",
                undo.display()
            );
        }
        Ok(hardware::LightingReceipt {
            backup_path: undo,
            info: device.info()?,
            state: LightingState {
                lighting: device.effect()?,
                custom_rgb: device.read_frame()?,
            },
            rgb_verified: true,
            effect_color_verified: false,
            requested_effect: Some(backup.restore_effect),
        })
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hardware::Endpoint, protocol::KeyboardInfo};
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };

    #[derive(Clone, Copy, PartialEq)]
    enum Fault {
        None,
        PartialWrite,
        WrongReadback,
        SwitchProfile,
        RestoreMismatch,
    }
    struct Mock {
        data: RefCell<Snapshot>,
        writes: Cell<u32>,
        fault: Fault,
        frames: RefCell<Vec<Vec<u8>>>,
    }
    impl LightingIo for Mock {
        fn info(&self) -> Result<KeyboardInfo> {
            Ok(self.data.borrow().info.clone())
        }
        fn state(&self) -> Result<LightingState> {
            let data = self.data.borrow();
            Ok(LightingState {
                lighting: data.blocks["lighting"].clone(),
                custom_rgb: data.blocks["custom_rgb"].clone(),
            })
        }
        fn write_rgb(&self, bytes: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            let mut data = self.data.borrow_mut();
            data.blocks.insert("custom_rgb".into(), bytes.to_vec());
            if self.fault == Fault::RestoreMismatch {
                data.blocks.get_mut("custom_rgb").unwrap()[1] ^= 255;
            }
            Ok(())
        }
        fn write_effect(&self, bytes: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            self.data
                .borrow_mut()
                .blocks
                .insert("lighting".into(), bytes.to_vec());
            Ok(())
        }
    }
    impl AnimationIo for Mock {
        fn snapshot(&self) -> Result<Snapshot> {
            Ok(self.data.borrow().clone())
        }
        fn write_frame(&self, bytes: &[u8]) -> Result<()> {
            self.frames.borrow_mut().push(bytes.to_vec());
            self.writes.set(self.writes.get() + 1);
            let mut data = self.data.borrow_mut();
            data.blocks.insert("custom_rgb".into(), bytes.to_vec());
            if self.fault == Fault::SwitchProfile {
                data.info.profile = 1;
            }
            ensure!(
                !matches!(self.fault, Fault::PartialWrite | Fault::SwitchProfile),
                "Simulated interrupted transfer"
            );
            Ok(())
        }
        fn read_frame(&self) -> Result<Vec<u8>> {
            let mut bytes = self.data.borrow().blocks["custom_rgb"].clone();
            if self.fault == Fault::WrongReadback && !self.frames.borrow().is_empty() {
                bytes[1] ^= 255;
            }
            Ok(bytes)
        }
    }
    fn fixture(fault: Fault) -> Mock {
        Mock {
            data: RefCell::new(Snapshot {
                schema: 1,
                endpoint: Endpoint {
                    path: "mock".into(),
                    vid: 0x0c45,
                    pid: 0x80a0,
                    product: "AK820MAX".into(),
                    usage_page: 0xff68,
                    usage: 97,
                    interface: 2,
                    role: "mock".into(),
                },
                info: KeyboardInfo {
                    vid: 0x0c45,
                    pid: 0x80a0,
                    version: "1.30".into(),
                    manufacturer: 358,
                    product: 4100,
                    work_mode: 15,
                    battery: 50,
                    charging: 0,
                    profile: 0,
                    rt_precision: 0,
                    frame_version: 0,
                    lighting_version: 0,
                    firmware_status: 0,
                    animation_limits: None,
                },
                blocks: BTreeMap::from([
                    (
                        "lighting".into(),
                        crate::protocol::static_lighting(&[0; 16]).unwrap(),
                    ),
                    (
                        "custom_rgb".into(),
                        crate::protocol::custom_rgb(&[[12, 34, 56]; 128]).unwrap(),
                    ),
                    ("game_mode".into(), vec![42; 56]),
                    ("rt".into(), vec![53; 1024]),
                ]),
            }),
            writes: Cell::new(0),
            fault,
            frames: RefCell::new(Vec::new()),
        }
    }
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "ajazz-playback-test-{}",
            crate::storage::unique_suffix()
        ))
    }
    fn run(device: &Mock, path: &Path, cancel: &AtomicBool) -> Result<Receipt> {
        play_on(
            device,
            &Scene::default(),
            Channel::Usb,
            Duration::from_millis(100),
            true,
            cancel,
            path,
            |_| cancel.store(true, Ordering::Relaxed),
            None,
        )
    }
    #[test]
    fn automatic_connection_prefers_cable_and_falls_back_after_unplug() {
        let endpoint = |pid, usage_page, usage| hardware::Endpoint {
            path: String::new(),
            vid: 0x0c45,
            pid,
            product: String::new(),
            usage_page,
            usage,
            interface: 2,
            role: String::new(),
        };
        let cable = endpoint(0x80a0, 0xff68, 97);
        let receiver = endpoint(0xfefc, 0xff80, 1);
        assert_eq!(
            Channel::detect(&[receiver.clone(), cable.clone()]),
            Some(Channel::Usb)
        );
        assert_eq!(
            Channel::detect(&[cable, receiver.clone()]),
            Some(Channel::Usb)
        );
        assert_eq!(Channel::detect(&[receiver]), Some(Channel::Radio));
        assert_eq!(Channel::detect(&[]), None);
        assert_eq!(Channel::detect(&[endpoint(0x80a0, 1, 6)]), None);
        assert_eq!(Channel::detect(&[endpoint(0xfefc, 0xff80, 2)]), None);
    }
    #[test]
    fn fast_frames_keep_packet_acks_but_do_not_read_the_whole_table_every_tick() {
        let device = fixture(Fault::None);
        let cancel = AtomicBool::new(false);
        let mut verified = Vec::new();
        let scene = crate::keyboard_animation::Settings::default().scene();
        let receipt = play_on(
            &device,
            &scene,
            Channel::Usb,
            Duration::from_millis(16),
            true,
            &cancel,
            &directory(),
            |p| {
                verified.push(p.verified);
                if p.frame == 3 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            None,
        )
        .unwrap();
        assert_eq!(receipt.frames, 3);
        assert_eq!(verified, vec![true, false, false]);
        assert!(receipt.restored);
    }
    #[test]
    fn factory_empty_table_is_initialized_only_in_working_frames_and_restored_raw() {
        let device = fixture(Fault::None);
        device
            .data
            .borrow_mut()
            .blocks
            .insert("custom_rgb".into(), vec![0; 512]);
        let original = device.data.borrow().blocks.clone();
        let receipt = run(&device, &directory(), &AtomicBool::new(false)).unwrap();
        assert_eq!(receipt.frames, 1);
        assert_eq!(device.data.borrow().blocks, original);
        let frame = device.frames.borrow()[0].clone();
        for (id, entry) in frame.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(entry[0] as usize, id);
        }
        let backup: AnimationBackup =
            crate::storage::read_json(&receipt.backup_path, 64 * 1024).unwrap();
        backup.validate().unwrap();
        assert_eq!(backup.before.blocks["custom_rgb"], vec![0; 512]);
        let mut malformed = original;
        malformed.get_mut("custom_rgb").unwrap()[5] = 99;
        device.data.borrow_mut().blocks = malformed;
        let writes = device.writes.get();
        assert!(run(&device, &directory(), &AtomicBool::new(false)).is_err());
        assert_eq!(writes, device.writes.get());
    }
    #[test]
    fn selected_animation_preserves_all_other_slots_and_original_brightness() {
        let device = fixture(Fault::None);
        device.data.borrow_mut().blocks.get_mut("lighting").unwrap()[9] = 2;
        let original = device.data.borrow().blocks.clone();
        let id = crate::model::keyboard_rows()[0][0].id;
        let settings = crate::keyboard_animation::Settings {
            keys: [id].into_iter().collect(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        play_on(
            &device,
            &settings.scene(),
            Channel::Usb,
            Duration::ZERO,
            true,
            &cancel,
            &directory(),
            |_| cancel.store(true, Ordering::Relaxed),
            None,
        )
        .unwrap();
        let frames = device.frames.borrow();
        for slot in 0..128 {
            if slot != id as usize {
                assert_eq!(
                    &frames[0][slot * 4..slot * 4 + 4],
                    &original["custom_rgb"][slot * 4..slot * 4 + 4]
                );
            }
        }
        assert_eq!(device.data.borrow().blocks, original);
    }
    #[test]
    fn radio_uses_its_own_connection_guard_and_restores() {
        let device = fixture(Fault::None);
        device.data.borrow_mut().info.work_mode = 2;
        let cancel = AtomicBool::new(false);
        let receipt = play_on(
            &device,
            &Scene::default(),
            Channel::Radio,
            Duration::ZERO,
            true,
            &cancel,
            &directory(),
            |_| cancel.store(true, Ordering::Relaxed),
            None,
        )
        .unwrap();
        assert!(receipt.restored);
        assert_eq!(receipt.frames, 1);
    }
    #[test]
    fn cancellation_restores_all_original_blocks_and_persists_backups() {
        let device = fixture(Fault::None);
        let original = device.data.borrow().blocks.clone();
        let receipt = run(&device, &directory(), &AtomicBool::new(false)).unwrap();
        assert_eq!(receipt.frames, 1);
        assert!(
            receipt.restored && receipt.backup_path.exists() && receipt.configuration_path.exists()
        );
        assert_eq!(device.data.borrow().blocks, original);
        // Unused matrix slots are kept from the original block, not blackened.
        let saved: AnimationBackup =
            crate::storage::read_json(&receipt.backup_path, 64 * 1024).unwrap();
        assert_eq!(saved.before.blocks["custom_rgb"], original["custom_rgb"]);
    }
    #[test]
    fn partial_transfer_and_wrong_readback_both_restore_original_blocks() {
        for fault in [Fault::PartialWrite, Fault::WrongReadback] {
            let device = fixture(fault);
            let original = device.data.borrow().blocks.clone();
            let error = run(&device, &directory(), &AtomicBool::new(false)).unwrap_err();
            assert!(error.to_string().contains("восстановлена и проверена"));
            assert_eq!(device.data.borrow().blocks, original);
        }
    }
    #[test]
    fn changed_profile_is_never_overwritten_by_restoration() {
        let device = fixture(Fault::SwitchProfile);
        let error = run(&device, &directory(), &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("Восстановление не подтверждено"));
        assert_eq!(device.writes.get(), 2); // mode (brightness changed) + interrupted frame, no restore writes
    }
    #[test]
    fn restore_mismatch_is_reported_instead_of_success() {
        let device = fixture(Fault::RestoreMismatch);
        let error = run(&device, &directory(), &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("Восстановление не подтверждено"));
    }
    #[test]
    fn backup_failure_and_cancel_before_start_prevent_all_writes() {
        let device = fixture(Fault::None);
        let path = directory();
        std::fs::write(&path, b"not a directory").unwrap();
        assert!(run(&device, &path, &AtomicBool::new(false)).is_err());
        assert_eq!(device.writes.get(), 0);
        let receipt = run(&device, &directory(), &AtomicBool::new(true)).unwrap();
        assert_eq!(receipt.frames, 0);
        assert_eq!(device.writes.get(), 0);
    }
}

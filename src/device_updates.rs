//! Verified, local OEM update workflow. Preparing never launches an updater.
//! Studio owns backups and model checks; the supplied OEM engine owns flashing.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Target {
    Dock,
    Keyboard,
    Mouse,
}
impl Target {
    pub const ALL: [Self; 3] = [Self::Dock, Self::Keyboard, Self::Mouse];
    pub fn title(self) -> &'static str {
        match self {
            Self::Dock => "Экран дока",
            Self::Keyboard => "Клавиатура",
            Self::Mouse => "Мышь",
        }
    }
    fn directory(self) -> &'static str {
        match self {
            Self::Dock => "Dock",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
        }
    }
    pub fn detail(self) -> &'static str {
        match self {
            Self::Dock => "APEX · OLED UI4, свой экран и оформление",
            Self::Keyboard => "AK820MAX ULTRA · AJKS, эффекты и энкодер",
            Self::Mouse => "AJ159 APEX ID1755 · штатная 3.02 от поддержки",
        }
    }
    pub fn instruction(self) -> &'static str {
        match self {
            Self::Dock => "В окне AJAZZ нажмите OLED.",
            Self::Keyboard => "В окне AJAZZ нажмите Start.",
            Self::Mouse => "В окне AJAZZ выберите обновление мыши.",
        }
    }
}
#[derive(Deserialize)]
struct Entry {
    target: Target,
    version: String,
    executable: String,
    files: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Manifest {
    entries: Vec<Entry>,
    sensor_installer_sha256: String,
    keyboard_image_offset: usize,
    keyboard_image_bytes: usize,
    keyboard_checksum_offset: usize,
}
fn manifest() -> Manifest {
    serde_json::from_str(include_str!("../assets/update-manifest.json"))
        .expect("Pinned update manifest")
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':'])
        && Path::new(name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}
fn directory() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .context("Нет каталога Studio")?
        .join("tools/updates"))
}
fn entry(target: Target) -> Result<Entry> {
    manifest()
        .entries
        .into_iter()
        .find(|e| e.target == target)
        .context("Пакет обновления не найден")
}
fn verify_files(root: &Path, files: &BTreeMap<String, String>) -> Result<()> {
    ensure!(!files.is_empty(), "Пакет обновления пуст");
    for (n, h) in files {
        ensure!(safe_relative(n), "Недопустимый путь пакета");
        ensure!(
            digest(
                &std::fs::read(root.join(n))
                    .with_context(|| format!("В пакете отсутствует {n}"))?
            ) == *h,
            "Файл обновления изменён: {n}"
        );
    }
    Ok(())
}
pub fn version(target: Target) -> String {
    entry(target).map_or_else(|_| "Нет пакета".into(), |e| e.version)
}
pub fn audit_bundle() -> Result<serde_json::Value> {
    let root = directory()?;
    let m = manifest();
    let mut targets = Vec::new();
    for e in m.entries {
        verify_files(&root.join(e.target.directory()), &e.files)?;
        targets.push(serde_json::json!({"target":e.target,"version":e.version,"verified_files":e.files.len()}));
    }
    ensure!(
        digest(&std::fs::read(root.join("PawnIO_setup.exe"))?) == m.sensor_installer_sha256,
        "Установщик датчиков изменён"
    );
    Ok(
        serde_json::json!({"targets":targets,"sensor_installer_verified":true,"hardware_access":false,"updaters_executed":false}),
    )
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prepared {
    pub target: Target,
    pub version: String,
    pub directory: PathBuf,
    pub executable: String,
    pub files: BTreeMap<String, String>,
    pub backup: PathBuf,
    pub notice: Option<String>,
    pub image_sha256: Option<String>,
    pub memory_fingerprint: Option<String>,
}
fn memory_fingerprint(s: &crate::keyboard_memory::MemoryState) -> Result<String> {
    Ok(digest(&serde_json::to_vec(&(&s.config, &s.scenes))?))
}
fn seed_keyboard(bytes: &mut [u8], state: &crate::keyboard_memory::MemoryState) -> Result<String> {
    let m = manifest();
    let start = m.keyboard_image_offset;
    let end = start
        .checked_add(m.keyboard_image_bytes)
        .context("Размер образа")?;
    ensure!(
        end <= bytes.len()
            && m.keyboard_image_bytes == 516096
            && m.keyboard_checksum_offset + 8 <= bytes.len(),
        "Неверный контейнер клавиатуры"
    );
    ensure!(
        state.scenes.len() == 8 && state.build_id == "fd469e3144ab533c",
        "Для сохранения режимов требуется установленная AJKS fd469e3144ab533c"
    );
    let image = &mut bytes[start..end];
    image[0x24000..0x26400].fill(0);
    for (i, program) in state.scenes.iter().enumerate() {
        if let Some(p) = program {
            crate::keyboard_memory::validate_page(p, false)?;
            image[0x24000 + i * 1024..0x24200 + i * 1024].copy_from_slice(p);
        }
    }
    image[0x26000..0x26200].copy_from_slice(&state.config.page()?);
    let hash = digest(image);
    let checksum = image.as_chunks::<2>().0.iter().fold(0u16, |a, b| {
        a.wrapping_add(u16::from_le_bytes([b[0], b[1]]))
    });
    let word: Vec<u8> = format!("{checksum:04X}")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes[m.keyboard_checksum_offset..m.keyboard_checksum_offset + 8].copy_from_slice(&word);
    Ok(hash)
}
#[cfg(windows)]
fn wired_mouse() -> Result<serde_json::Value> {
    let api = hidapi::HidApi::new()?;
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == 0x3151
                && [0x400b, 0x4026, 0x402a].contains(&d.product_id())
                && d.usage_page() == 0xffff
                && d.usage() == 2
        })
        .collect();
    ensure!(
        devices.len() == 1,
        "Подключите AJ159 APEX кабелем и включите проводной режим"
    );
    let d = devices[0].open_device(&api)?;
    let mut p = [0; 65];
    p[1] = 0xf1;
    p[8] = 14;
    d.send_feature_report(&p)?;
    std::thread::sleep(std::time::Duration::from_millis(60));
    let mut r = [0; 65];
    ensure!(
        d.get_feature_report(&mut r)? == 65
            && r[1] == 0xf1
            && u16::from_le_bytes([r[2], r[3]]) == 1755,
        "Проводная мышь не подтвердила ID1755"
    );
    Ok(
        serde_json::json!({"id":1755,"vid":0x3151,"pid":devices[0].product_id(),"identity":r.as_slice()}),
    )
}
/// Read-only preparation, including durable backups, before any confirmation.
#[cfg(windows)]
pub fn prepare(target: Target) -> Result<Prepared> {
    let e = entry(target)?;
    let source = directory()?.join(target.directory());
    verify_files(&source, &e.files)?;
    let backups = crate::storage::backup_directory()?;
    let (backup, memory, notice) = match target {
        Target::Dock => {
            let identity = crate::dock::read_info()?;
            ensure!(identity.oled_version == 0x104, "Требуется OLED104 / ID1759");
            let (config, pages) = crate::dock_telemetry::read_editor()?;
            (crate::storage::backup(&backups,"before-dock-firmware",&serde_json::json!({"identity":identity,"config":config,"pages":pages}))?,None,Some("Копия содержит настройки и список экранов. Фото/GIF остаются в доке; исходные файлы в копию не считываются.".into()))
        }
        Target::Keyboard => {
            let native = crate::hardware::usb_snapshot()?;
            ensure!(
                native.is_wired()?,
                "Для прошивки подключите клавиатуру кабелем"
            );
            let state =
                crate::keyboard_memory::run(crate::keyboard_memory::Operation::Read, &backups)?;
            let path = crate::storage::backup(
                &backups,
                "before-keyboard-firmware",
                &serde_json::json!({"native":native,"memory":state}),
            )?;
            (path,Some(state),Some("Текущие режимы, пустые слоты и энкодер перенесены в образ. После обновления пройдите калибровку клавиш в Studio.".into()))
        }
        Target::Mouse => {
            let identity = wired_mouse()?;
            let settings = crate::mouse_hid::read_snapshot();
            let notice=settings.as_ref().err().map(|_|"Прочитан ID мыши. Копия настроек по радиоканалу недоступна; штатный обновлятор может сбросить настройки.".to_owned());
            let path = crate::storage::backup(
                &backups,
                "before-mouse-firmware",
                &serde_json::json!({"identity":identity,"settings":settings.as_ref().ok(),"settings_error":settings.as_ref().err().map(|e|format!("{e:#}"))}),
            )?;
            (path, None, notice)
        }
    };
    let work = backups
        .parent()
        .context("Нет каталога Studio")?
        .join("updates")
        .join(format!(
            "{}-{}",
            target.directory(),
            crate::storage::unique_suffix()
        ));
    std::fs::create_dir_all(&work)?;
    for n in e.files.keys() {
        let to = work.join(n);
        std::fs::create_dir_all(to.parent().unwrap())?;
        std::fs::copy(source.join(n), to)?;
    }
    let mut files = e.files;
    let mut image_sha256 = None;
    let mut fingerprint = None;
    if let Some(s) = memory {
        let exe = work.join(&e.executable);
        let mut data = std::fs::read(&exe)?;
        image_sha256 = Some(seed_keyboard(&mut data, &s)?);
        fingerprint = Some(memory_fingerprint(&s)?);
        std::fs::write(exe, &data)?;
        files.insert(e.executable.clone(), digest(&data));
    }
    verify_files(&work, &files)?;
    let prepared = Prepared {
        target,
        version: e.version,
        directory: work,
        executable: e.executable,
        files,
        backup,
        notice,
        image_sha256,
        memory_fingerprint: fingerprint,
    };
    crate::storage::write_json_atomic(
        &prepared.directory.join("studio-preflight.json"),
        &prepared,
    )?;
    Ok(prepared)
}
#[cfg(not(windows))]
pub fn prepare(_: Target) -> Result<Prepared> {
    anyhow::bail!("Windows only")
}

#[cfg(windows)]
fn native_installer(path: &Path, parameters: &str, elevate: bool, hidden: bool) -> Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
    };
    let wide = |v: &std::ffi::OsStr| v.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let file = wide(path.as_os_str());
    let cwd = wide(path.parent().context("Каталог обновления")?.as_os_str());
    let args: Vec<u16> = parameters.encode_utf16().chain(Some(0)).collect();
    let verb: Vec<u16> = if elevate { "runas\0" } else { "open\0" }
        .encode_utf16()
        .collect();
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = args.as_ptr();
    info.lpDirectory = cwd.as_ptr();
    info.nShow = if hidden { 0 } else { 1 };
    ensure!(
        unsafe { ShellExecuteExW(&mut info) } != 0,
        "Не удалось открыть обновлятор: {}",
        std::io::Error::last_os_error()
    );
    ensure!(
        !info.hProcess.is_null(),
        "Windows не вернул процесс обновлятора"
    );
    let waited = unsafe { WaitForSingleObject(info.hProcess, u32::MAX) };
    let mut code = 0;
    let read = unsafe { GetExitCodeProcess(info.hProcess, &mut code) };
    unsafe { CloseHandle(info.hProcess) };
    ensure!(
        waited == WAIT_OBJECT_0 && read != 0,
        "Не удалось получить результат обновлятора"
    );
    Ok(code)
}
/// Only the explicitly confirmed prepared package may reach this function.
#[cfg(windows)]
pub fn launch(p: &Prepared) -> Result<String> {
    verify_files(&p.directory, &p.files)?;
    ensure!(p.backup.is_file(), "Резервная копия не найдена");
    match p.target {
        Target::Dock => {
            crate::dock_telemetry::probe()?;
        }
        Target::Mouse => {
            wired_mouse()?;
        }
        Target::Keyboard => {
            ensure!(
                crate::hardware::usb_snapshot()?.is_wired()?,
                "Подключите клавиатуру кабелем"
            );
            let now = crate::keyboard_memory::run(
                crate::keyboard_memory::Operation::Read,
                &crate::storage::backup_directory()?,
            )?;
            ensure!(
                Some(memory_fingerprint(&now)?) == p.memory_fingerprint,
                "Настройки клавиатуры изменились. Подготовьте обновление заново"
            );
        }
    }
    let code = native_installer(&p.directory.join(&p.executable), "", false, false)?;
    crate::storage::write_json_atomic(
        &p.directory.join("studio-result.json"),
        &serde_json::json!({"exit_code":code,"flash_success_not_inferred_from_exit_code":true}),
    )?;
    ensure!(
        code == 0,
        "Обновлятор завершился с кодом {code}; не отключайте устройство, проверьте его сообщение"
    );
    Ok(match p.target{
        Target::Dock=>if crate::dock_telemetry::probe()?.design_version>=4{"Док ответил OLED UI4. Свои экраны доступны."}else{"Обновлятор закрыт. OLED UI4 не подтверждён: проверьте результат в его окне."},
        Target::Keyboard=>{let s=crate::keyboard_memory::run(crate::keyboard_memory::Operation::Read,&crate::storage::backup_directory()?)?;ensure!(Some(memory_fingerprint(&s)?)==p.memory_fingerprint,"Клавиатура ответила, но режимы отличаются от копии. Настройки сохранены в резервной копии");"Клавиатура ответила, режимы и энкодер сверены. Пройдите калибровку клавиш."}
        Target::Mouse=>{wired_mouse()?;"Мышь ответила ID1755. Окончательный результат записи смотрите в окне AJAZZ."}
    }.into())
}
#[cfg(not(windows))]
pub fn launch(_: &Prepared) -> Result<String> {
    anyhow::bail!("Windows only")
}
pub fn sensor_driver_installed() -> bool {
    std::env::var_os("ProgramFiles")
        .is_some_and(|p| PathBuf::from(p).join("PawnIO/PawnIOLib.dll").is_file())
}
pub fn enable_sensors() -> Result<()> {
    #[cfg(windows)]
    {
        if !sensor_driver_installed() {
            let p = directory()?.join("PawnIO_setup.exe");
            ensure!(
                digest(&std::fs::read(&p)?) == manifest().sensor_installer_sha256,
                "Установщик датчиков изменён"
            );
            let code = native_installer(&p, "-install -silent", true, true)?;
            ensure!(code == 0, "Установка датчиков завершилась с кодом {code}");
        }
        if crate::metrics_agent::snapshot(&crate::graphics::Options::default()).is_none()
            && !crate::metrics_agent::pending()
        {
            crate::metrics_agent::request()?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_has_exact_targets_and_safe_local_files() {
        let m = manifest();
        assert_eq!(m.entries.len(), 3);
        for t in Target::ALL {
            let e = entry(t).unwrap();
            assert!(e.files.contains_key(&e.executable));
            for n in e.files.keys() {
                assert!(safe_relative(n));
            }
        }
        for n in ["../x", "C:/x", "/x", "a\\b", ""] {
            assert!(!safe_relative(n));
        }
    }
    #[test]
    fn seeded_keyboard_preserves_code_and_every_empty_slot() {
        let m = manifest();
        let mut bytes = vec![0x5a; m.keyboard_checksum_offset + 8];
        let before = bytes.clone();
        let mut state = crate::keyboard_memory::MemoryState {
            config: Default::default(),
            scenes: vec![None; 8],
            firmware: "1.36".into(),
            build_id: "fd469e3144ab533c".into(),
            runtime: None,
        };
        state.scenes[2] =
            Some(crate::keyboard_memory::compile(&crate::model::Scene::default()).unwrap());
        state.config.encoder[0] = crate::keyboard_memory::Action::Speed(-1);
        seed_keyboard(&mut bytes, &state).unwrap();
        let at = m.keyboard_image_offset;
        assert_eq!(bytes[..at + 0x24000], before[..at + 0x24000]);
        for (i, p) in state.scenes.iter().enumerate() {
            let slot = &bytes[at + 0x24000 + i * 1024..at + 0x24400 + i * 1024];
            if let Some(p) = p {
                assert_eq!(slot[..512], p[..]);
                assert!(slot[512..].iter().all(|v| *v == 0));
            } else {
                assert!(slot.iter().all(|v| *v == 0));
            }
        }
        assert_eq!(
            bytes[at + 0x26000..at + 0x26200],
            state.config.page().unwrap()
        );
        assert_eq!(
            bytes[at + 0x26400..m.keyboard_checksum_offset],
            before[at + 0x26400..m.keyboard_checksum_offset]
        );
        let image = &bytes[at..at + 516096];
        let sum = image
            .as_chunks::<2>()
            .0
            .iter()
            .fold(0u16, |a, b| a.wrapping_add(u16::from_le_bytes(*b)));
        let text: Vec<u8> = format!("{sum:04X}")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(bytes[m.keyboard_checksum_offset..], text);
    }
    #[test]
    fn changed_bundle_is_rejected_before_preparation() {
        let root = std::env::temp_dir().join(crate::storage::unique_suffix());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("engine.exe"), b"original").unwrap();
        let files = BTreeMap::from([("engine.exe".into(), digest(b"original"))]);
        assert!(verify_files(&root, &files).is_ok());
        std::fs::write(root.join("engine.exe"), b"changed").unwrap();
        assert!(verify_files(&root, &files).is_err());
        std::fs::remove_file(root.join("engine.exe")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

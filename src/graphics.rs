//! Read-only graphics telemetry. Native ETW supplies DXGI/D3D9 presents;
//! PresentMon covers other runtimes. Missing/stale observations stay unavailable.
use serde::{Deserialize, Serialize};
#[cfg(windows)]
mod native;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metrics {
    pub gpu_permille: Option<u16>,
    pub vram_used_mib: Option<u32>,
    pub vram_total_mib: Option<u32>,
    pub fps_tenths: Option<u16>,
    #[serde(default)]
    pub temperatures: crate::temperatures::Readings,
    #[serde(default)]
    pub power_deciwatts: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_history: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Options {
    pub fps_enabled: bool,
    pub process_name: String,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            fps_enabled: true,
            process_name: String::new(),
        }
    }
}

fn csv(line: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line
        .trim_start_matches('\u{feff}')
        .trim_end()
        .chars()
        .peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return None;
    }
    fields.push(field);
    Some(fields)
}

type FrameQueue = std::collections::VecDeque<(std::time::Instant, f64)>;
#[derive(Default)]
struct Frames {
    native_events: u64,
    native_decoded: u64,
    native_lag_ms: Option<u32>,
    native_error: Option<String>,
    lines: u64,
    header: Vec<String>,
    chains: std::collections::HashMap<(u32, String), (String, FrameQueue)>,
    error: Option<String>,
    last_game: Option<u32>,
}
impl Frames {
    fn ingest(&mut self, line: &str, now: std::time::Instant) {
        self.lines += 1;
        let Some(row) = csv(line) else {
            return;
        };
        if row
            .iter()
            .any(|s| s.eq_ignore_ascii_case("msBetweenPresents"))
            && row.iter().any(|s| s.eq_ignore_ascii_case("ProcessID"))
        {
            self.header = row;
            return;
        }
        let get = |name: &str| {
            self.header
                .iter()
                .position(|s| s.eq_ignore_ascii_case(name))
                .and_then(|i| row.get(i))
        };
        let Some(pid) = get("ProcessID").and_then(|s| s.parse::<u32>().ok()) else {
            return;
        };
        let Some(ms) = get("MsBetweenPresents")
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.1 && *n <= 1000.0)
        else {
            return;
        };
        let Some(chain) = get("SwapChainAddress") else {
            return;
        };
        let name = get("Application").cloned().unwrap_or_default();
        self.observe(pid, chain.clone(), name, ms, now);
    }
    fn observe(&mut self, pid: u32, chain: String, name: String, ms: f64, now: std::time::Instant) {
        if !ms.is_finite() || !(0.1..=1000.0).contains(&ms) {
            return;
        }
        if self.chains.len() > 128 {
            self.chains.retain(|_, (_, q)| {
                q.back()
                    .is_some_and(|(at, _)| now.duration_since(*at).as_secs() < 3)
            });
        }
        let (_, queue) = self
            .chains
            .entry((pid, chain))
            .or_insert_with(|| (name, Default::default()));
        queue.push_back((now, ms));
        while queue.len() > 1000
            || queue
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at).as_secs_f64() > 1.5)
        {
            queue.pop_front();
        }
    }
    fn fps_observation(
        &self,
        pid: u32,
        name: &str,
        now: std::time::Instant,
    ) -> Option<(u16, bool)> {
        let matching = self
            .chains
            .iter()
            .filter(|((p, _), (app, q))| {
                (if name.is_empty() {
                    *p == pid && pid != std::process::id()
                } else {
                    app.eq_ignore_ascii_case(name)
                }) && q.len() >= 3
                    && q.back()
                        .is_some_and(|(at, _)| now.duration_since(*at).as_secs_f64() < 2.0)
            })
            // Prefer the continuous in-process trace. The compatibility worker
            // may restart or stop producing output without affecting this path.
            .max_by_key(|((_, chain), (_, q))| (chain.starts_with("native:"), q.len()));
        let ((_, chain), (_, q)) = matching?;
        let native = chain.starts_with("native:");
        let mut count = 0;
        let mut total = 0.0;
        for (at, ms) in q.iter().rev() {
            // Respond to a frame-rate change without averaging the previous
            // 1.5 seconds. Keep at least three samples for low frame rates.
            if native
                && count >= 3
                && now.duration_since(*at) > std::time::Duration::from_millis(350)
            {
                break;
            }
            count += 1;
            total += ms;
        }
        let fps = (count as f64 * 10000.0 / total).round();
        (fps > 0.0 && fps <= 65535.0).then_some((fps as u16, native))
    }
    fn fps(&self, pid: u32, name: &str, now: std::time::Instant) -> Option<u16> {
        self.fps_observation(pid, name, now).map(|v| v.0)
    }
}

fn game_candidate(app: &str) -> bool {
    let name = app
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(app)
        .to_ascii_lowercase();
    !name.is_empty()
        && !matches!(
            name.as_str(),
            "<unknown>"
                | "dwm.exe"
                | "explorer.exe"
                | "ajazz-studio.exe"
                | "ajazz-lab.exe"
                | "codex.exe"
                | "chatgpt.exe"
                | "discord.exe"
                | "chrome.exe"
                | "msedge.exe"
                | "firefox.exe"
                | "steam.exe"
                | "steamwebhelper.exe"
                | "nvidia overlay.exe"
                | "searchhost.exe"
                | "startmenuexperiencehost.exe"
                | "applicationframehost.exe"
                | "msedgewebview2.exe"
                | "gameoverlayui64.exe"
                | "gameoverlayui.exe"
                | "gamebar.exe"
                | "gamebarftserver.exe"
                | "presentmon.exe"
                | "presentmon_x64.exe"
                | "nvidia share.exe"
                | "nvcontainer.exe"
                | "textinputhost.exe"
                | "systemsettings.exe"
                | "wallpaper32.exe"
                | "wallpaper64.exe"
        )
}
impl Frames {
    fn automatic_fps(&mut self, pid: u32, name: &str, now: std::time::Instant) -> Option<u16> {
        if !name.is_empty() {
            return self.fps(pid, name, now);
        }
        let game = self
            .chains
            .iter()
            .any(|((p, _), (app, _))| *p == pid && game_candidate(app));
        if game && let Some(fps) = self.fps(pid, "", now) {
            self.last_game = Some(pid);
            return Some(fps);
        }
        if self.last_game.is_none() {
            let candidates: std::collections::BTreeSet<_> = self
                .chains
                .iter()
                .filter(|((p, _), (app, _))| game_candidate(app) && self.fps(*p, "", now).is_some())
                .map(|((p, _), _)| *p)
                .collect();
            if candidates.len() == 1 {
                self.last_game = candidates.first().copied();
            }
        }
        let last = self.last_game?;
        let fps = self.fps(last, "", now);
        if fps.is_none() {
            self.last_game = None;
        }
        fps
    }
}
#[cfg(windows)]
mod win {
    use super::*;
    use anyhow::{Context, Result, ensure};
    use std::{
        ffi::c_void,
        io::{BufRead, BufReader},
        os::windows::process::CommandExt,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Instant,
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(path: *mut u16, len: u32) -> u32;
        fn LoadLibraryW(path: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
    }
    #[repr(C)]
    #[derive(Default)]
    struct Memory {
        total: u64,
        free: u64,
        used: u64,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Utilization {
        gpu: u32,
        memory: u32,
    }
    type Init = unsafe extern "C" fn() -> i32;
    type Handle = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
    type GetMemory = unsafe extern "C" fn(*mut c_void, *mut Memory) -> i32;
    type GetUtilization = unsafe extern "C" fn(*mut c_void, *mut Utilization) -> i32;
    type GetTemperature = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32;
    type GetPower = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
    struct Nvml {
        library: *mut c_void,
        device: *mut c_void,
        memory: GetMemory,
        util: GetUtilization,
        shutdown: Init,
        temperature: Option<GetTemperature>,
        power: Option<GetPower>,
    }
    impl Nvml {
        fn new() -> Result<Self> {
            let mut path = [0u16; 512];
            // SAFETY: fixed output buffer; load only the installed system driver.
            let n = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
            ensure!(
                n > 0 && n + 10 < path.len(),
                "Путь драйвера NVIDIA недоступен"
            );
            let mut full = path[..n].to_vec();
            full.extend("\\nvml.dll\0".encode_utf16());
            let library = unsafe { LoadLibraryW(full.as_ptr()) };
            ensure!(!library.is_null(), "Драйвер NVIDIA NVML недоступен");
            macro_rules! symbol {
                ($name:literal,$ty:ty) => {{
                    let p = unsafe { GetProcAddress(library, concat!($name, "\0").as_ptr()) };
                    if p.is_null() {
                        unsafe { FreeLibrary(library) };
                        anyhow::bail!("NVML: отсутствует {}", $name);
                    }
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(p) }
                }};
            }
            let init = symbol!("nvmlInit_v2", Init);
            let handle = symbol!("nvmlDeviceGetHandleByIndex_v2", Handle);
            let memory = symbol!("nvmlDeviceGetMemoryInfo", GetMemory);
            let util = symbol!("nvmlDeviceGetUtilizationRates", GetUtilization);
            let shutdown = symbol!("nvmlShutdown", Init);
            let t = unsafe { GetProcAddress(library, c"nvmlDeviceGetTemperature".as_ptr().cast()) };
            let p = unsafe { GetProcAddress(library, c"nvmlDeviceGetPowerUsage".as_ptr().cast()) };
            let temperature = if t.is_null() {
                None
            } else {
                Some(unsafe { std::mem::transmute::<*mut c_void, GetTemperature>(t) })
            };
            let power = if p.is_null() {
                None
            } else {
                Some(unsafe { std::mem::transmute::<*mut c_void, GetPower>(p) })
            };
            if unsafe { init() } != 0 {
                unsafe { FreeLibrary(library) };
                anyhow::bail!("Не удалось открыть NVML");
            }
            let mut device = std::ptr::null_mut();
            if unsafe { handle(0, &mut device) } != 0 {
                unsafe {
                    shutdown();
                    FreeLibrary(library)
                };
                anyhow::bail!("Видеокарта NVIDIA недоступна");
            }
            Ok(Self {
                library,
                device,
                memory,
                util,
                shutdown,
                temperature,
                power,
            })
        }
        fn sample(&self) -> Metrics {
            let mut value = Metrics {
                temperatures: crate::temperatures::sample(),
                ..Default::default()
            };
            let mut util = Utilization::default();
            let mut mem = Memory::default();
            let mut temperature = 0;
            let mut power = 0;
            if self
                .temperature
                .is_some_and(|get| unsafe { get(self.device, 0, &mut temperature) } == 0)
                && temperature <= 150
            {
                value.temperatures.gpu = Some((temperature * 10) as i16);
            }
            if self
                .power
                .is_some_and(|get| unsafe { get(self.device, &mut power) } == 0)
                && power <= 3_000_000
            {
                value.power_deciwatts = Some((power / 100) as u16);
            }
            // SAFETY: function signatures match NVML v1 ABI and outputs are valid.
            if unsafe { (self.util)(self.device, &mut util) } == 0 && util.gpu <= 100 {
                value.gpu_permille = Some((util.gpu * 10) as u16);
            }
            if unsafe { (self.memory)(self.device, &mut mem) } == 0
                && mem.total > 0
                && mem.used <= mem.total
                && mem.total / (1024 * 1024) <= 1_048_576
            {
                value.vram_used_mib = Some((mem.used / (1024 * 1024)) as u32);
                value.vram_total_mib = Some((mem.total / (1024 * 1024)) as u32);
            }
            value
        }
    }
    impl Drop for Nvml {
        fn drop(&mut self) {
            unsafe {
                (self.shutdown)();
                FreeLibrary(self.library)
            };
        }
    }

    pub struct Collector {
        nvml: Option<Nvml>,
        frames: Arc<Mutex<Frames>>,
        stop: Arc<AtomicBool>,
        options: Options,
        pub gpu_notice: Option<String>,
        native: Option<std::thread::JoinHandle<()>>,
        slow: std::cell::RefCell<Option<(Instant, Metrics)>>,
    }
    impl Collector {
        pub fn new(options: Options) -> Self {
            let (nvml, gpu_notice) = match Nvml::new() {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(e.to_string())),
            };
            let frames = Arc::new(Mutex::new(Frames::default()));
            let stop = Arc::new(AtomicBool::new(false));
            let mut native = None;
            if options.fps_enabled {
                let native_frames = frames.clone();
                let native_stop = stop.clone();
                native = Some(std::thread::spawn(move || {
                    if let Err(e) = super::native::run(native_frames.clone(), native_stop)
                        && let Ok(mut s) = native_frames.lock()
                    {
                        s.native_error = Some(format!("{e:#}"));
                    }
                }));
                let f = frames.clone();
                let cancel = stop.clone();
                std::thread::spawn(move || {
                    if let Err(e) = fps_worker(&f, &cancel)
                        && let Ok(mut state) = f.lock()
                    {
                        state.error = Some(format!("FPS: {e:#}"));
                    }
                });
            }
            Self {
                nvml,
                frames,
                stop,
                options,
                gpu_notice,
                native,
                slow: Default::default(),
            }
        }
        pub fn sample(&self) -> Metrics {
            if let Some(remote) = crate::metrics_agent::snapshot(&self.options) {
                return remote.metrics;
            }
            let mut slow = self.slow.borrow_mut();
            if slow
                .as_ref()
                .is_none_or(|(at, _)| at.elapsed() >= std::time::Duration::from_secs(1))
            {
                let mut result = self
                    .nvml
                    .as_ref()
                    .map_or_else(Metrics::default, Nvml::sample);
                if self.nvml.is_none() {
                    result.temperatures = crate::temperatures::sample();
                }
                *slow = Some((Instant::now(), result));
            }
            let mut result = slow.as_ref().unwrap().1.clone();
            drop(slow);
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
            if let Ok(mut state) = self.frames.lock() {
                result.fps_tenths =
                    state.automatic_fps(pid, self.options.process_name.trim(), Instant::now());
            }
            result
        }
        pub fn fps_notice(&self) -> Option<String> {
            if let Some(remote) = crate::metrics_agent::snapshot(&self.options) {
                return remote.notice;
            }
            let state = self.frames.lock().ok()?;
            if state.chains.values().any(|(_, q)| {
                q.back()
                    .is_some_and(|(at, _)| at.elapsed() < std::time::Duration::from_secs(2))
            }) {
                return None;
            }
            state.error.clone().or_else(|| state.native_error.clone())
        }
        pub fn debug(&self) -> serde_json::Value {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
            let Ok(s) = self.frames.lock() else {
                return serde_json::Value::Null;
            };
            let now = Instant::now();
            let mut apps: Vec<_> = s.chains.iter().map(|((p,_),(name,q))| {
                serde_json::json!({"pid":p,"app":name.chars().take(40).collect::<String>(),"samples":q.len(),"fps":s.fps(*p,"",now),"source":s.fps_observation(*p,"",now).map(|v|if v.1 {"Windows ETW"}else{"PresentMon"})})
            }).collect();
            apps.sort_by_key(|v| v["pid"].as_u64());
            apps.dedup_by_key(|v| v["pid"].as_u64());
            apps.truncate(6);
            serde_json::json!({"foreground":pid,"options":self.options,"lines":s.lines,"native_events":s.native_events,"native_decoded":s.native_decoded,"native_lag_ms":s.native_lag_ms,"native_error":s.native_error,"header":s.header,"apps":apps})
        }
    }
    impl Drop for Collector {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(native) = self.native.take() {
                let _ = native.join();
            }
        }
    }
    fn fps_worker(frames: &Mutex<Frames>, stop: &AtomicBool) -> Result<()> {
        use sha2::{Digest, Sha256};
        let helper = std::env::current_exe()?
            .parent()
            .context("Нет каталога приложения")?
            .join("tools/presentmon/PresentMon.exe");
        let bytes = std::fs::read(&helper)
            .context("Компонент PresentMon отсутствует в пакете приложения")?;
        ensure!(
            format!("{:x}", Sha256::digest(bytes))
                == "b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af",
            "Компонент FPS изменён; переустановите пакет Studio"
        );
        static SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let session = format!(
            "AJAZZ-Studio-{}-{}",
            std::process::id(),
            SESSION.fetch_add(1, Ordering::Relaxed)
        );
        while !stop.load(Ordering::Relaxed) {
            // A bounded trace ends itself cleanly. No forced process kill,
            // persistent service, injected DLL, output CSV file or global hotkey.
            let mut child = std::process::Command::new(&helper)
                .args([
                    "--output_stdout",
                    "--no_console_stats",
                    "--v1_metrics",
                    "--no_track_gpu",
                    "--no_track_input",
                    "--timed",
                    "10",
                    "--terminate_after_timed",
                    "--session_name",
                    &session,
                ])
                .creation_flags(0x08000000)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            let stderr = child.stderr.take().unwrap();
            let errors = std::thread::spawn(move || {
                let mut out = String::new();
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if out.len() < 2000 {
                        out.push_str(&line);
                        out.push(' ');
                    }
                }
                out
            });
            for line in BufReader::new(child.stdout.take().unwrap()).lines() {
                let line = line?;
                if let Ok(mut state) = frames.lock() {
                    state.ingest(&line, Instant::now());
                }
            }
            let status = child.wait()?;
            let error = errors.join().unwrap_or_default();
            ensure!(
                status.success(),
                "Нет доступа к захвату кадров. {error} Разрешите фоновый сборщик показателей в Studio."
            );
        }
        Ok(())
    }
}
#[cfg(windows)]
pub use win::Collector;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn native_fps_reacts_to_new_frames_without_long_old_rate_tail() {
        let mut f = Frames::default();
        let t = Instant::now();
        for i in (400u32..1200).step_by(4).rev() {
            f.observe(
                42,
                "native:1".into(),
                "game.exe".into(),
                4.0,
                t - Duration::from_millis(u64::from(i)),
            );
        }
        for i in (0u32..200).step_by(10).rev() {
            f.observe(
                42,
                "native:1".into(),
                "game.exe".into(),
                10.0,
                t - Duration::from_millis(u64::from(i)),
            );
        }
        assert_eq!(f.fps(42, "", t), Some(1000));
        let mut slow = Frames::default();
        for i in (0u32..600).step_by(200).rev() {
            slow.observe(
                42,
                "native:2".into(),
                "slow.exe".into(),
                200.0,
                t - Duration::from_millis(u64::from(i)),
            );
        }
        assert_eq!(slow.fps(42, "", t), Some(50));
    }
    #[test]
    fn native_trace_is_primary_and_compatibility_fallback_stays_fresh() {
        let mut f = Frames::default();
        let t = Instant::now();
        for _ in 0..20 {
            f.observe(42, "0x1".into(), "game.exe".into(), 20.0, t);
        }
        for _ in 0..3 {
            f.observe(42, "native:1".into(), "game.exe".into(), 10.0, t);
        }
        assert_eq!(f.fps_observation(42, "", t), Some((1000, true)));
        let later = t + Duration::from_secs(3);
        for _ in 0..3 {
            f.observe(42, "0x1".into(), "game.exe".into(), 25.0, later);
        }
        assert_eq!(f.fps_observation(42, "", later), Some((400, false)));
        assert_eq!(
            f.fps_observation(42, "", later + Duration::from_secs(3)),
            None
        );
    }
    #[test]
    fn pinned_presentmon_v26_header_and_bom_are_recognized() {
        let mut f = Frames::default();
        let now = Instant::now();
        f.ingest("\u{feff}Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,Dropped,TimeInSeconds,msInPresentAPI,msBetweenPresents,AllowsTearing,PresentMode,msUntilRenderComplete,msUntilDisplayed,msBetweenDisplayChange,msFlipDelay",now);
        for _ in 0..12 {
            f.ingest("Overwatch.exe,20108,0x000001,DXGI,0,0,0,1.0,0.1,4.0,1,Hardware: Independent Flip,3,4,4,0",now);
        }
        assert_eq!(f.automatic_fps(20108, "", now), Some(2500));
        assert_eq!(f.automatic_fps(0, "", now + Duration::from_secs(3)), None);
    }
    #[test]
    fn overlays_do_not_prevent_automatic_game_selection() {
        let mut f = Frames::default();
        let t = Instant::now();
        f.ingest(
            "Application,ProcessID,SwapChainAddress,msBetweenPresents",
            t,
        );
        for _ in 0..6 {
            f.ingest("game.exe,42,0x1,10", t);
            f.ingest("gameoverlayui64.exe,43,0x2,5", t);
            f.ingest("GameBar.exe,44,0x3,1", t);
        }
        assert_eq!(f.automatic_fps(0, "", t), Some(1000));
        assert_eq!(f.last_game, Some(42));
    }
    #[test]
    fn automatic_target_follows_games_survives_alt_tab_and_expires() {
        let mut f = Frames::default();
        let t = Instant::now();
        f.ingest(
            "Application,ProcessID,SwapChainAddress,MsBetweenPresents",
            t,
        );
        for _ in 0..6 {
            f.ingest("Overwatch.exe,42,0x1,10", t);
            f.ingest("other-game.exe,43,0x2,20", t);
            f.ingest("msedge.exe,44,0x3,1", t);
        }
        assert_eq!(f.automatic_fps(44, "", t), None);
        assert_eq!(f.automatic_fps(42, "", t), Some(1000));
        assert_eq!(f.automatic_fps(std::process::id(), "", t), Some(1000));
        assert_eq!(f.automatic_fps(43, "", t), Some(500));
        assert_eq!(f.automatic_fps(44, "", t), Some(500));
        assert_eq!(f.automatic_fps(44, "Overwatch.exe", t), Some(1000));
        assert_eq!(f.automatic_fps(44, "", t + Duration::from_secs(3)), None);
        assert_eq!(f.last_game, None);
    }
    #[test]
    fn csv_frames_do_not_sum_swapchains_or_keep_stale_fps() {
        let mut f = Frames::default();
        let t = Instant::now();
        f.ingest(
            "Application,ProcessID,SwapChainAddress,MsBetweenPresents",
            t,
        );
        for _ in 0..60 {
            f.ingest("game.exe,42,0x1,16.6667", t);
        }
        for _ in 0..4 {
            f.ingest("game.exe,42,0x2,100", t);
        }
        for bad in [
            "game.exe,42,0x1,NaN",
            "game.exe,42,0x1,-10",
            "game.exe,42,0x1,0",
        ] {
            f.ingest(bad, t);
        }
        assert_eq!(f.fps(42, "", t), Some(600));
        assert_eq!(f.fps(99, "game.exe", t), Some(600));
        assert_eq!(f.fps(99, "", t), None);
        assert_eq!(f.fps(42, "", t + Duration::from_secs(3)), None);
        assert_eq!(csv("\"game,a.exe\",42").unwrap(), ["game,a.exe", "42"]);
        assert!(csv("\"bad").is_none());
    }
}

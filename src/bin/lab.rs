use ajazz_studio::{
    firmware, hardware, keyboard_settings, lighting, local_bridge, model::Scene, mouse, screen,
    storage,
};
use anyhow::{Context, Result, bail};
use std::path::Path;
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("updates-check") => {
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::device_updates::audit_bundle()?)?
            );
        }
        Some("update-prepare") => {
            let target = match args.get(2).map(String::as_str) {
                Some("dock") => ajazz_studio::device_updates::Target::Dock,
                Some("keyboard") => ajazz_studio::device_updates::Target::Keyboard,
                Some("mouse") => ajazz_studio::device_updates::Target::Mouse,
                _ => bail!("Specify dock, keyboard or mouse"),
            };
            let p = ajazz_studio::device_updates::prepare(target)?;
            storage::write_json_atomic(Path::new(args.get(3).context("Missing output JSON")?), &p)?;
            println!("Prepared and backed up; updater not launched.");
        }
        #[cfg(windows)]
        Some("metrics-check") => {
            let output = Path::new(args.get(2).context("Missing diagnostic output")?);
            if args.iter().any(|s| s == "--request-access") {
                ajazz_studio::metrics_agent::request()?;
            }
            let mut options = ajazz_studio::graphics::Options::default();
            let mut collector = ajazz_studio::graphics::Collector::new(options.clone());
            let mut samples = Vec::new();
            for _ in 0..240 {
                if let Some(i) = args.iter().position(|s| s == "--controls")
                    && let Some(path) = args.get(i + 1)
                    && let Ok(bytes) = std::fs::read(path)
                    && let Ok(next) =
                        serde_json::from_slice::<ajazz_studio::graphics::Options>(&bytes)
                    && next != options
                {
                    options = next;
                    collector = ajazz_studio::graphics::Collector::new(options.clone());
                }
                let value = collector.sample();
                let ready = value.fps_tenths.is_some();
                let capture = ajazz_studio::metrics_agent::snapshot(&options)
                    .map(|s| s.debug)
                    .unwrap_or_else(|| collector.debug());
                samples.push(serde_json::json!({"metrics":value,"capture":capture,"fps_notice":collector.fps_notice(),"agent_notice":ajazz_studio::metrics_agent::notice()}));
                storage::write_json_atomic(output, &samples)?;
                if !args.iter().any(|s| s == "--keep-running")
                    && ready
                    && samples
                        .iter()
                        .rev()
                        .take(5)
                        .filter(|s| !s["metrics"]["fps_tenths"].is_null())
                        .count()
                        >= 5
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            println!("{}", serde_json::to_string_pretty(samples.last().unwrap())?);
        }
        Some("keyboard-runtime-watch") => {
            let seconds: u64 = args.get(2).context("Missing3..30seconds")?.parse()?;
            anyhow::ensure!((3..=30).contains(&seconds), "Duration3..30seconds");
            let output = Path::new(args.get(3).context("Missing output JSON")?);
            let start = std::time::Instant::now();
            let mut rows = Vec::new();
            while start.elapsed() < std::time::Duration::from_secs(seconds) {
                let value = ajazz_studio::keyboard_memory::runtime_background();
                rows.push(serde_json::json!({"elapsed_ms":start.elapsed().as_millis(),"runtime":value.as_ref().ok().and_then(|v|v.clone()),"error":value.err().map(|e|format!("{e:#}"))}));
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            storage::write_json_atomic(output, &rows)?;
            println!("Captured{} read-only observations", rows.len());
        }
        #[cfg(windows)]
        Some("graphics-info") => {
            let options = ajazz_studio::graphics::Options {
                fps_enabled: args.iter().any(|s| s == "--fps"),
                process_name: args
                    .get(2)
                    .filter(|s| !s.starts_with("--"))
                    .cloned()
                    .unwrap_or_default(),
            };
            let collector = ajazz_studio::graphics::Collector::new(options);
            std::thread::sleep(std::time::Duration::from_secs(
                if args.iter().any(|s| s == "--fps") {
                    5
                } else {
                    1
                },
            ));
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"metrics":collector.sample(),"gpu_notice":collector.gpu_notice,"fps_notice":collector.fps_notice()})
                )?
            );
        }
        Some("dock-composition-demo") => {
            use ajazz_studio::{
                dock_composition::{Backdrop, Composition, Metric},
                dock_pages::{Kind, Page, Playlist},
                dock_telemetry::{Config, Sample},
                graphics::Metrics,
                temperatures::Readings,
            };
            let out = Path::new(args.get(2).context("Missing output directory")?);
            std::fs::create_dir_all(out)?;
            let now = chrono::DateTime::parse_from_rfc3339("2026-10-26T12:34:00+03:00")?
                .with_timezone(&chrono::Local);
            for effect in Backdrop::ALL {
                for case in 1..=if effect == Backdrop::Still { 15 } else { 4 } {
                    let count = if case > 4 { 1 } else { case };
                    for variant in 0..3 {
                        let missing = variant == 1;
                        let mut layout = Composition {
                            metrics: [Metric::Fps, Metric::CpuTemp, Metric::GpuTemp, Metric::Vram],
                            backdrop: effect,
                            bars: variant != 0,
                        };
                        for i in count..4 {
                            layout.metrics[i] = Metric::None;
                        }
                        if case > 4 {
                            layout.metrics = [
                                Metric::ALL[case - 4],
                                Metric::None,
                                Metric::None,
                                Metric::None,
                            ];
                        }
                        let c = Config {
                            style: 3,
                            border: false,
                            ..Default::default()
                        };
                        let p = Playlist {
                            pages: vec![Page {
                                kind: Kind::Custom,
                                composition: layout,
                                ..Default::default()
                            }],
                            selected: 0,
                            autoplay: false,
                            skip_offline: false,
                            ..Default::default()
                        };
                        let mut s = Sample {
                            cpu_permille: 234,
                            ram_permille: 625,
                            used_mib: 10000,
                            total_mib: 16000,
                            sequence: 1,
                            graphics: Metrics {
                                gpu_permille: Some(765),
                                vram_used_mib: Some(6144),
                                vram_total_mib: Some(8192),
                                fps_tenths: Some(2388),
                                power_deciwatts: Some(2270),
                                temperatures: Readings {
                                    cpu: Some(623),
                                    gpu: Some(-100),
                                    storage: Some(530),
                                    board: Some(500),
                                },
                                ..Default::default()
                            },
                        };
                        if variant == 2 {
                            s.graphics.fps_tenths = Some(65535);
                            s.graphics.power_deciwatts = Some(65535);
                            s.graphics.temperatures.cpu = Some(1500);
                            s.cpu_permille = 1000;
                            s.ram_permille = 1000;
                        }
                        let path =
                            out.join(format!("composition-{}-{case}-{variant}", effect as u8));
                        ajazz_studio::dock_telemetry_preview::render_page(
                            &c,
                            &p,
                            Kind::Custom,
                            if missing { None } else { Some(&s) },
                            now,
                        )?
                        .save(path.with_extension("png"))?;
                        storage::write_json_atomic(
                            &path.with_extension("json"),
                            &serde_json::json!({"config_bytes":c.encode()?.as_slice(),"pages_bytes":p.encode()?.as_slice(),"sample":if missing{None}else{Some(s.encode_extended()?.to_vec())},"phase":((now.timestamp_millis()*9/1000)&255)as u8}),
                        )?;
                    }
                }
            }
        }
        Some("dock-metrics-demo") => {
            use ajazz_studio::{
                dock_pages::{Kind, Page, Playlist},
                dock_telemetry::{Config, Sample},
                graphics::Metrics,
                temperatures::Readings,
            };
            let out = Path::new(args.get(2).context("Missing output directory")?);
            std::fs::create_dir_all(out)?;
            for kind in [Kind::Gaming, Kind::Thermals, Kind::Gpu, Kind::Pulse] {
                for variant in ["live", "missing", "edge"] {
                    let c = Config {
                        style: 3,
                        border: false,
                        ..Default::default()
                    };
                    let p = Playlist {
                        pages: vec![Page {
                            kind,
                            ..Default::default()
                        }],
                        selected: 0,
                        skip_offline: false,
                        autoplay: false,
                        ..Default::default()
                    };
                    let mut history = Vec::new();
                    let mut wires = Vec::new();
                    let mut last = None;
                    if variant != "missing" {
                        for i in 0..32 {
                            let mut s = Sample {
                                cpu_permille: 234,
                                ram_permille: 625,
                                used_mib: 10000,
                                total_mib: 16000,
                                sequence: i + 1,
                                graphics: Metrics {
                                    gpu_permille: Some(765),
                                    vram_used_mib: Some(6144),
                                    vram_total_mib: Some(8192),
                                    fps_tenths: Some(2388),
                                    power_deciwatts: Some(2270),
                                    temperatures: Readings {
                                        cpu: Some(623),
                                        gpu: Some(680),
                                        storage: Some(530),
                                        board: Some(500),
                                    },
                                    ..Default::default()
                                },
                            };
                            if variant == "edge" {
                                s.cpu_permille = 1000;
                                s.graphics.gpu_permille = Some(1000);
                                s.graphics.fps_tenths = Some(65535);
                                s.graphics.power_deciwatts = Some(65535);
                                s.graphics.temperatures = Readings {
                                    cpu: Some(1500),
                                    gpu: Some(-100),
                                    storage: None,
                                    board: Some(999),
                                };
                            }
                            let cpu = if i == 31 {
                                s.cpu_permille
                            } else {
                                (i * 137) % 1001
                            };
                            let gpu = if i == 31 {
                                s.graphics.gpu_permille.unwrap()
                            } else {
                                (i * 83) % 1001
                            };
                            s.cpu_permille = cpu;
                            s.graphics.gpu_permille = Some(gpu);
                            history.insert(
                                0,
                                ((u32::from(cpu) * 14 / 1000) | ((u32::from(gpu) * 14 / 1000) << 4))
                                    as u8,
                            );
                            s.graphics.load_history = history.clone();
                            wires.push(s.encode_extended()?.to_vec());
                            last = Some(s);
                        }
                    }
                    let path = out.join(format!("metrics-{}-{variant}", kind as u8));
                    let now = chrono::DateTime::parse_from_rfc3339("2026-10-23T12:34:23+03:00")?
                        .with_timezone(&chrono::Local);
                    ajazz_studio::dock_telemetry_preview::render_page(
                        &c,
                        &p,
                        kind,
                        last.as_ref(),
                        now,
                    )?
                    .save(path.with_extension("png"))?;
                    storage::write_json_atomic(
                        &path.with_extension("json"),
                        &serde_json::json!({"config_bytes":c.encode()?.as_slice(),"pages_bytes":p.encode()?.as_slice(),"samples":wires,"kind":kind as u8}),
                    )?;
                }
            }
        }
        Some("dock-game-demo") => {
            use ajazz_studio::{dock_telemetry as d, graphics};
            let path = Path::new(args.get(2).context("Missing PNG output")?);
            let mut c = d::Config {
                style: 3,
                ..Default::default()
            };
            match args.get(3).map(String::as_str) {
                Some("percent") => c.bars = false,
                Some("bars") => c.percentages = false,
                _ => {}
            }
            let mut sample = d::Sample {
                cpu_permille: 234,
                ram_permille: 625,
                used_mib: 10000,
                total_mib: 16000,
                sequence: 43,
                graphics: graphics::Metrics {
                    gpu_permille: Some(850),
                    vram_used_mib: Some(6144),
                    vram_total_mib: Some(8192),
                    fps_tenths: Some(1440),
                    ..Default::default()
                },
            };
            let pages = ajazz_studio::dock_pages::Playlist {
                autoplay: false,
                skip_offline: false,
                ..Default::default()
            };
            match args.get(4).map(String::as_str) {
                Some("max") => {
                    sample.cpu_permille = 1000;
                    sample.ram_permille = 1000;
                    sample.graphics = graphics::Metrics {
                        gpu_permille: Some(1000),
                        vram_used_mib: Some(1_048_576),
                        vram_total_mib: Some(1_048_576),
                        fps_tenths: Some(65535),
                        ..Default::default()
                    };
                }
                Some("zero") => {
                    sample.cpu_permille = 0;
                    sample.ram_permille = 0;
                    sample.graphics.gpu_permille = Some(0);
                    sample.graphics.vram_used_mib = Some(0);
                    sample.graphics.fps_tenths = Some(10);
                }
                Some("missing") => sample.graphics = Default::default(),
                Some("fps") => c.memory_size = false,
                _ => {}
            }
            if args.get(5).is_some_and(|v| v == "paper") {
                c.background = [242, 240, 232];
                c.foreground = [25, 30, 34];
                c.accent = [40, 95, 215];
            }
            let sample = if args.get(4).is_some_and(|v| v == "offline") {
                None
            } else {
                Some(sample)
            };
            let now = chrono::Local::now();
            ajazz_studio::dock_telemetry_preview::render_page(
                &c,
                &pages,
                ajazz_studio::dock_pages::Kind::Summary,
                sample.as_ref(),
                now,
            )?
            .save(path)?;
            storage::write_json_atomic(
                &path.with_extension("json"),
                &serde_json::json!({"config_bytes":c.encode()?.as_slice(),"pages_bytes":pages.encode()?.as_slice(),"sample_bytes":sample.as_ref().map(d::Sample::encode_graphics).transpose()?.map(|v|v.to_vec())}),
            )?;
        }
        Some("keyboard-clock-check") => {
            let seconds: u64 = args
                .get(2)
                .context("Missing duration3..15 seconds")?
                .parse()?;
            anyhow::ensure!((3..=15).contains(&seconds), "Duration3..15 seconds");
            let output = Path::new(args.get(3).context("Missing output file")?);
            let begin = std::time::Instant::now();
            let first = ajazz_studio::keyboard_memory::runtime()?
                .context("Clock-capable firmware required")?;
            let first_at = begin.elapsed().as_millis() as u64;
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            let last =
                ajazz_studio::keyboard_memory::runtime()?.context("Clock response missing")?;
            let elapsed_ms = begin.elapsed().as_millis() as u64 - first_at;
            anyhow::ensure!(
                !first.paused
                    && !last.paused
                    && first.selected == last.selected
                    && first.selected.is_some(),
                "Animation paused or mode changed during observation"
            );
            let device_ms = last.millis.wrapping_sub(first.millis);
            let frames = last.frame.wrapping_sub(first.frame);
            let clock_error = (i64::from(device_ms) - elapsed_ms as i64).abs();
            let frame_error = (i64::from(frames) * 20 - i64::from(device_ms)).abs();
            let report = serde_json::json!({"first":first,"last":last,"wall_ms":elapsed_ms,"device_ms":device_ms,"frames":frames,"clock_error_ms":clock_error,"frame_error_ms":frame_error,"passed":clock_error<150&&frame_error<=20,"read_only":true});
            storage::write_json_atomic(output, &report)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            anyhow::ensure!(report["passed"] == true, "Hardware clock check failed");
        }
        Some("keyboard-memory-info") => {
            let state = ajazz_studio::keyboard_memory::run(
                ajazz_studio::keyboard_memory::Operation::Read,
                &ajazz_studio::storage::backup_directory()?,
            )?;
            println!("{}", serde_json::to_string_pretty(&state)?);
        }
        Some("scan") => println!("{}", serde_json::to_string_pretty(&hardware::scan()?)?),
        Some("keyboard-info") => {
            let channel = args.get(2).map(String::as_str).unwrap_or("radio");
            anyhow::ensure!(["usb", "radio"].contains(&channel), "Unknown channel");
            println!(
                "{}",
                serde_json::to_string_pretty(&hardware::keyboard_info(channel == "usb")?)?
            );
        }
        Some("keyboard-link-watch") => {
            let seconds: u16 = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: keyboard-link-watch SECONDS OUTPUT"))?
                .parse()?;
            let output = Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing output path"))?,
            );
            println!("Passive control-interface monitor for {seconds} seconds; no settings writes");
            let report = ajazz_studio::link_diagnostics::watch(seconds, output)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("keyboard-legacy-rgb-check") => {
            let output = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing baseline path"))?,
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::keyboard_live::diagnose_marker(
                    output
                )?)?
            );
        }
        Some("keyboard-legacy-music-check") => {
            let output = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing baseline path"))?,
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::keyboard_live::diagnose_music(
                    output
                )?)?
            );
        }
        Some("keyboard-usb-snapshot") => {
            let snapshot = hardware::usb_snapshot()?;
            if let Some(path) = args.get(2) {
                snapshot.save(Path::new(path))?;
            }
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Some("keyboard-table-animation-check") => {
            let channel = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Missing channel: usb or radio"))?;
            anyhow::ensure!(
                ["usb", "radio"].contains(&channel.as_str()),
                "Unknown channel"
            );
            let output = Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing baseline path"))?,
            );
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &ajazz_studio::keyboard_live::diagnose_table_animation(
                        channel == "usb",
                        output
                    )?
                )?
            );
        }
        Some("lighting-play") => {
            let scene = Scene::load(Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing scene"))?,
            ))?;
            let channel = match args.get(3).map(String::as_str) {
                Some("usb") => ajazz_studio::keyboard_playback::Channel::Usb,
                Some("radio") => ajazz_studio::keyboard_playback::Channel::Radio,
                _ => bail!("Missing channel: usb or radio"),
            };
            let interval = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(250);
            let receipt = ajazz_studio::keyboard_playback::play(
                &scene,
                channel,
                interval,
                false,
                &std::sync::atomic::AtomicBool::new(false),
                |p| {
                    println!(
                        "frame {} time {:.2}s write {}ms transfer {}ms verified {} cycle {}ms",
                        p.frame, p.scene_time, p.write_ms, p.transfer_ms, p.verified, p.frame_ms
                    )
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Some("keyboard-usb-live-check") => {
            let mode = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Missing format: modern or legacy"))?;
            anyhow::ensure!(
                ["modern", "legacy"].contains(&mode.as_str()),
                "Unknown USB format"
            );
            let output = Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing baseline path"))?,
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::keyboard_live::diagnose_usb(
                    mode == "legacy",
                    output
                )?)?
            );
        }
        Some("keyboard-reapply-current-general") => {
            let output = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing diagnostic baseline path"))?,
            );
            let snapshot = ajazz_studio::link_diagnostics::reapply_current_general(output)?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Some("pc-sample") => {
            #[cfg(windows)]
            {
                let mut counters = ajazz_studio::dock_telemetry::windows::Counters::new()?;
                std::thread::sleep(std::time::Duration::from_secs(1));
                println!("{}", serde_json::to_string_pretty(&counters.sample(1)?)?);
            }
            #[cfg(not(windows))]
            anyhow::bail!("Windows only");
        }
        Some("dock-timer-info") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::timer(None, None)?)?
        ),
        Some("dock-timer-command") => {
            let command: u8 = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Missing command0–5"))?
                .parse()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::timer(
                    Some(command),
                    None
                )?)?
            );
        }
        Some("dock-screen-stop") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::stop_stream()?)?
        ),
        Some("dock-pages-info") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::pages_info()?)?
        ),
        Some("dock-pages-apply") => {
            let path = Path::new(args.get(2).ok_or_else(|| {
                anyhow::anyhow!("Usage: dock-pages-apply PLAYLIST.json [--save]")
            })?);
            let p = storage::read_json::<ajazz_studio::dock_pages::Playlist>(path, 8192)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::pages_apply(
                    &p,
                    args.get(3).is_some_and(|s| s == "--save")
                )?)?
            );
        }
        Some("dock-page-select") | Some("dock-asset-delete") => {
            let index: u8 = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Missing one-based index"))?
                .parse()?;
            anyhow::ensure!(index >= 1, "Index starts at1");
            let status = if args[1] == "dock-page-select" {
                ajazz_studio::dock_telemetry::select_page(index - 1)?
            } else {
                ajazz_studio::dock_telemetry::delete_asset(index - 1)?
            };
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Some("dock-native") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::native()?)?
        ),
        Some("dock-page-demo") => {
            use ajazz_studio::dock_pages::{Kind, Playlist};
            let index: usize = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: dock-page-demo KIND_0_4 OUTPUT.png"))?
                .parse()?;
            anyhow::ensure!(index < Kind::ALL.len(), "Unknown page kind");
            let kind = Kind::ALL[index];
            let path = Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing output PNG"))?,
            );
            let c = ajazz_studio::dock_telemetry::Config {
                style: args.get(4).map(|v| v.parse()).transpose()?.unwrap_or(0),
                ..Default::default()
            };
            let mut p = Playlist {
                selected: index.min(3) as u8,
                autoplay: false,
                ..Default::default()
            };
            if index >= 4 {
                p.pages.push(ajazz_studio::dock_pages::Page {
                    kind,
                    ..Default::default()
                });
                p.selected = 4;
            }
            let sample = ajazz_studio::dock_telemetry::Sample {
                cpu_permille: 234,
                ram_permille: 625,
                used_mib: 10000,
                total_mib: 16000,
                sequence: 42,
                graphics: Default::default(),
            };
            let now = chrono::DateTime::parse_from_rfc3339("2026-10-23T12:34:23+03:00")?
                .with_timezone(&chrono::Local);
            ajazz_studio::dock_telemetry_preview::render_page(&c, &p, kind, Some(&sample), now)?
                .save(path)?;
            storage::write_json_atomic(
                &path.with_extension("json"),
                &serde_json::json!({"config":c,"config_bytes":c.encode()?.as_slice(),"playlist":p,"pages_bytes":p.encode()?.as_slice(),"pages_report":ajazz_studio::dock_telemetry::packet(5,&p.encode()?)?.as_slice(),"kind":index}),
            )?;
            println!("{}", path.display());
        }
        Some("dock-screen-info") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::probe()?)?
        ),
        Some("dock-screen-apply") => {
            let path =
                Path::new(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("Usage: dock-screen-apply CONFIG.json [--save]")
                })?);
            let config = ajazz_studio::storage::read_json::<ajazz_studio::dock_telemetry::Config>(
                path, 8192,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::dock_telemetry::apply(
                    &config,
                    args.get(3).is_some_and(|s| s == "--save")
                )?)?
            );
        }
        Some("dock-screen-demo") => {
            let path = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Usage: dock-screen-demo OUTPUT.png"))?,
            );
            let config = ajazz_studio::dock_telemetry::Config::default();
            let sample = ajazz_studio::dock_telemetry::Sample {
                cpu_permille: 234,
                ram_permille: 625,
                used_mib: 10000,
                total_mib: 16000,
                sequence: 42,
                graphics: Default::default(),
            };
            ajazz_studio::dock_telemetry_preview::render(&config, Some(&sample), 12, 34)?
                .save(path)?;
            ajazz_studio::storage::write_json_atomic(
                &path.with_extension("json"),
                &serde_json::json!({
                    "config":config,"sample":sample,"config_bytes":config.encode()?.as_slice(),
                    "sample_bytes":sample.encode()?.as_slice(),
                    "config_report":ajazz_studio::dock_telemetry::packet(1,&config.encode()?)?.as_slice(),
                    "sample_report":ajazz_studio::dock_telemetry::packet(2,&sample.encode()?)?.as_slice(),
                }),
            )?;
            println!("{}", path.display());
        }
        Some("dock-info") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock::read_info()?)?
        ),
        Some("dock-clock-sync") => println!(
            "{}",
            serde_json::to_string_pretty(&ajazz_studio::dock::sync_clock()?)?
        ),
        Some("dock-pack") => {
            let slot: u8 = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: dock-pack SLOT_1_5 IMAGE OUTPUT.json"))?
                .parse()?;
            anyhow::ensure!((1..=5).contains(&slot), "Slot must be 1–5");
            let media = screen::Media::load(Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing image"))?,
            ))?;
            let frame =
                ajazz_studio::dock_display::DisplayFrame::from_image(slot - 1, &media.frames[0])?;
            let path = Path::new(
                args.get(4)
                    .ok_or_else(|| anyhow::anyhow!("Missing output"))?,
            );
            storage::write_json_atomic(path, &frame)?;
            frame.preview()?.save(path.with_extension("png"))?;
            println!(
                "Prepared slot {slot}, {} packets; no device writes",
                frame.data_reports()?.len()
            );
        }
        Some("dock-upload") => {
            let path = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Usage: dock-upload PREPARED.json"))?,
            );
            let frame = ajazz_studio::dock_display::DisplayFrame::load(path)?;
            let receipt = ajazz_studio::dock_display::upload(&frame, |_| {})?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Some("dock-animation-pack") => {
            let media = screen::Media::load(Path::new(
                args.get(2).ok_or_else(|| anyhow::anyhow!("Missing GIF"))?,
            ))?;
            let output = Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing output"))?,
            );
            let delay = args
                .get(4)
                .map(|v| v.parse::<u8>())
                .transpose()?
                .unwrap_or(50);
            let animation =
                ajazz_studio::dock_display::DisplayAnimation::from_images(&media.frames, delay)?;
            storage::write_json_atomic(output, &animation)?;
            println!(
                "Prepared {} animation frames; no device writes",
                animation.frames.len()
            );
        }
        Some("dock-animation-demo") => {
            let output = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing output"))?,
            );
            let font = screen::load_system_font();
            let images = [[200, 0, 0], [0, 0, 200]]
                .into_iter()
                .map(|color| {
                    let design = screen::ScreenDesign {
                        background: color,
                        title: "AJAZZ / TEST".into(),
                        show_clock: false,
                        show_date: false,
                        show_accent_line: false,
                        ..Default::default()
                    };
                    screen::render(&design, chrono::Local::now(), None, font.as_ref())
                })
                .collect::<Vec<_>>();
            let animation =
                ajazz_studio::dock_display::DisplayAnimation::from_images(&images, 250)?;
            storage::write_json_atomic(output, &animation)?;
            println!("Prepared two red/blue frames at 250 ms; no device writes");
        }
        Some("dock-animation-upload") => {
            let animation = ajazz_studio::dock_display::DisplayAnimation::load(Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Missing prepared animation"))?,
            ))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::dock_display::upload_animation(
                    &animation,
                    |_| {}
                )?)?
            );
        }
        Some("radio-status") => println!(
            "{}",
            serde_json::to_string_pretty(&local_bridge::read_radio_status()?)?
        ),
        Some("mouse-firmware-info") => {
            let report = ajazz_studio::mouse_firmware::read_via_vendor_service()?;
            if let Some(path) = args.get(2) {
                storage::write_json_atomic(Path::new(path), &report)?;
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("mouse-snapshot") => {
            let snapshot = ajazz_studio::mouse_hid::read_snapshot()?;
            let json = serde_json::to_string_pretty(&snapshot)?;
            if let Some(path) = args.get(2) {
                std::fs::write(path, &json)?;
            }
            println!("{json}");
        }
        Some("mouse-native-snapshot") => {
            let snapshot = ajazz_studio::mouse_hid::read_snapshot()?;
            if let Some(path) = args.get(2) {
                storage::write_json_atomic(Path::new(path), &snapshot)?;
            }
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Some("mouse-buttons") => {
            let snapshot = ajazz_studio::mouse_buttons::read()?;
            if let Some(path) = args.get(2) {
                storage::write_json_atomic(Path::new(path), &snapshot)?;
            }
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Some("mouse-buttons-apply") => {
            let before = ajazz_studio::mouse_buttons::ButtonSnapshot::load(Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Usage: mouse-buttons-apply BASELINE EDIT"))?,
            ))?;
            let edit: ajazz_studio::mouse_buttons::ButtonEdit = storage::read_json(
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing button edit"))?,
                ),
                4096,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::mouse_buttons::apply(&before, &edit)?)?
            );
        }
        Some("mouse-buttons-restore") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: mouse-buttons-restore BACKUP"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::mouse_buttons::restore(Path::new(
                    path
                ))?)?
            );
        }
        Some("general-apply") => {
            let expected: hardware::Snapshot = storage::read_json(
                Path::new(
                    args.get(2)
                        .ok_or_else(|| anyhow::anyhow!("Usage: general-apply BASELINE SETTINGS"))?,
                ),
                128 * 1024,
            )?;
            let settings: ajazz_studio::keyboard_general::GeneralSettings = storage::read_json(
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing general settings"))?,
                ),
                16384,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::apply_general(
                    &expected, &settings
                )?)?
            );
        }
        Some("keyboard-general-check") => {
            let field = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: keyboard-general-check FIELD BASELINE"))?;
            let output = Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing baseline path"))?,
            );
            let receipt = keyboard_settings::diagnose_general_cycle(field, output)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Some("general-restore") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: general-restore BACKUP"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::restore_general(Path::new(
                    path
                ))?)?
            );
        }
        Some("mouse-apply") => {
            let baseline =
                mouse::MouseSnapshot::load(Path::new(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("Usage: mouse-apply BASELINE.json SETTINGS.json")
                })?))?;
            let settings: mouse::MouseSettings = storage::read_json(
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing settings file"))?,
                ),
                16384,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&mouse::apply_settings(&baseline, &settings)?)?
            );
        }
        Some("mouse-restore") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: mouse-restore BACKUP.json"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&mouse::restore(Path::new(path))?)?
            );
        }
        Some("snapshot") => {
            let path = args
                .get(2)
                .map(String::as_str)
                .unwrap_or("keyboard-snapshot.json");
            let snap = hardware::keyboard_snapshot()?;
            snap.save(Path::new(path))?;
            println!("Read-only snapshot saved to {path}");
        }
        Some("hall-apply") => {
            let expected: hardware::Snapshot =
                storage::read_json(
                    Path::new(args.get(2).ok_or_else(|| {
                        anyhow::anyhow!("Usage: hall-apply BASELINE.json EDIT.json")
                    })?),
                    128 * 1024,
                )?;
            let edit: keyboard_settings::HallEdit = storage::read_json(
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing Hall edit"))?,
                ),
                16384,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::apply(&expected, &edit)?)?
            );
        }
        Some("hall-restore") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: hall-restore BACKUP.json"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::restore(Path::new(path))?)?
            );
        }
        Some("keymap-apply") => {
            let expected: hardware::Snapshot = storage::read_json(
                Path::new(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("Usage: keymap-apply BASELINE.json EDIT.json")
                })?),
                128 * 1024,
            )?;
            let edit: ajazz_studio::keymap::KeymapEdit = storage::read_json(
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing keymap edit"))?,
                ),
                16384,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::apply_keymap(&expected, &edit)?)?
            );
        }
        Some("keyboard-macros") => {
            let wired = match args.get(2).map(String::as_str) {
                Some("usb") => true,
                Some("radio") => false,
                _ => bail!("Usage: keyboard-macros usb|radio [OUTPUT.json]"),
            };
            let snapshot = ajazz_studio::keyboard_macros::read(wired)?;
            if let Some(path) = args.get(3) {
                storage::write_json_atomic(Path::new(path), &snapshot)?;
            }
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Some("macros-apply") => {
            let expected: ajazz_studio::keyboard_macros::MacroSnapshot = storage::read_json(
                Path::new(
                    args.get(2)
                        .context("Usage: macros-apply BASELINE.json EDIT.json")?,
                ),
                128 * 1024,
            )?;
            let edit: ajazz_studio::keyboard_macros::Edit = storage::read_json(
                Path::new(args.get(3).context("Missing macro edit")?),
                128 * 1024,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::keyboard_macros::apply(
                    &expected, &edit
                )?)?
            );
        }
        Some("macros-restore") => {
            let path = Path::new(args.get(2).context("Usage: macros-restore BACKUP.json")?);
            println!(
                "{}",
                serde_json::to_string_pretty(&ajazz_studio::keyboard_macros::restore(path)?)?
            );
        }
        Some("dks-apply") => {
            let expected: hardware::Snapshot = storage::read_json(
                Path::new(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("Usage: dks-apply BASELINE KEY_ID CONFIG.json")
                })?),
                128 * 1024,
            )?;
            let key: u8 = args
                .get(3)
                .ok_or_else(|| anyhow::anyhow!("Missing physical key ID"))?
                .parse()?;
            let settings: ajazz_studio::keyboard_dks::DksConfig = storage::read_json(
                Path::new(
                    args.get(4)
                        .ok_or_else(|| anyhow::anyhow!("Missing DKS config"))?,
                ),
                16 * 1024,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::apply_dks(
                    &expected, key, &settings
                )?)?
            );
        }
        Some("dks-restore") => {
            let path = Path::new(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("Usage: dks-restore BACKUP.json"))?,
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::restore_dks(path)?)?
            );
        }
        Some("keymap-restore") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: keymap-restore BACKUP.json"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&keyboard_settings::restore_keymap(Path::new(path))?)?
            );
        }
        Some("lighting-apply") => {
            let scene = Scene::load(Path::new(args.get(2).ok_or_else(|| {
                anyhow::anyhow!("Usage: lighting-apply SCENE [TIME] [radio|usb]")
            })?))?;
            let time: f32 = args.get(3).map(String::as_str).unwrap_or("0").parse()?;
            anyhow::ensure!(
                time.is_finite() && (0.0..=scene.duration).contains(&time),
                "Time outside scene"
            );
            let channel = args.get(4).map(String::as_str).unwrap_or("radio");
            anyhow::ensure!(
                ["radio", "usb"].contains(&channel),
                "Unknown keyboard channel"
            );
            let colors = lighting::keyboard_frame(&scene, time, &[]);
            let receipt = if let Some(path) = args.get(5) {
                let expected: hardware::Snapshot = storage::read_json(Path::new(path), 128 * 1024)?;
                anyhow::ensure!(
                    expected.is_wired()? == (channel == "usb"),
                    "Snapshot channel mismatch"
                );
                hardware::apply_keyboard_frame_from_snapshot(&expected, &colors)?
            } else {
                hardware::apply_keyboard_frame_on(&colors, channel == "usb")?
            };
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Some("lighting-restore") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: lighting-restore BACKUP.json"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&hardware::restore_keyboard_lighting(Path::new(
                    path
                ))?)?
            );
        }
        Some("lighting-effect") => {
            let baseline: hardware::Snapshot = storage::read_json(
                Path::new(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("Usage: lighting-effect BASELINE.json SETTINGS.json")
                })?),
                128 * 1024,
            )?;
            let settings: ajazz_studio::protocol::KeyboardLighting = storage::read_json(
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing lighting settings"))?,
                ),
                16384,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&hardware::apply_keyboard_effect(
                    &baseline, &settings
                )?)?
            );
        }
        Some("inspect") => {
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: ajazz-lab inspect FILE"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&firmware::inspect(Path::new(path))?)?
            );
        }
        Some("new-scene") => {
            Scene::default().save(Path::new(
                args.get(2)
                    .map(String::as_str)
                    .unwrap_or("scene.ajazz.json"),
            ))?;
        }
        Some("lighting-collection-audit") => {
            ajazz_studio::lighting_collection::audit(
                Path::new(args.get(2).context("Missing candidate JSON")?),
                Path::new(args.get(3).context("Missing output directory")?),
            )?;
        }
        Some("render") => {
            let scene =
                Scene::load(Path::new(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("Usage: ajazz-lab render SCENE OUTPUT.gif")
                })?))?;
            screen::export_lighting_gif(
                &scene,
                Path::new(
                    args.get(3)
                        .ok_or_else(|| anyhow::anyhow!("Missing GIF path"))?,
                ),
            )?;
        }
        Some("screen") => screen::export_png(
            &Default::default(),
            Path::new(args.get(2).map(String::as_str).unwrap_or("dock-screen.png")),
            None,
        )?,
        None | Some("help") => println!(
            "AJAZZ Lab {}\n  scan\n  radio-status          Read cached status from AJAZZ service\n  mouse-firmware-info [FILE] Read ID/version only via running AJAZZ service\n  mouse-snapshot [FILE] Read AJ159 APEX D3/D4 directly over HID\n  mouse-buttons [FILE] Read AJ159 main button matrix\n  mouse-buttons-apply BASELINE EDIT\n  mouse-buttons-restore BACKUP\n  mouse-apply BASELINE SETTINGS  Apply and verify mouse settings\n  mouse-restore BACKUP   Restore editable mouse settings\n  snapshot [FILE.json]   Read configuration only\n  general-apply BASELINE SETTINGS\n  general-restore BACKUP\n  hall-apply BASELINE EDIT\n  hall-restore BACKUP\n  keymap-apply BASELINE EDIT\n  keymap-restore BACKUP\n  lighting-effect BASELINE SETTINGS\n  lighting-apply SCENE [TIME]  Apply one static frame; automatic backup\n  lighting-play SCENE usb|radio [INTERVAL_MS]  One scene pass, restore afterward\n  lighting-restore BACKUP     Restore and verify lighting\n  dock-screen-info\n  dock-screen-stop\n  dock-pages-info\n  dock-pages-apply PLAYLIST.json [--save]\n  dock-page-select INDEX_1_8\n  dock-asset-delete SLOT_1_6\n  dock-native\n  dock-timer-info\n  dock-timer-command COMMAND_0_5\n  dock-info\n  dock-clock-sync\n  dock-pack SLOT_1_5 IMAGE OUTPUT.json\n  dock-upload PREPARED.json\n  dock-animation-pack GIF OUTPUT.json [DELAY_MS]\n  dock-animation-demo OUTPUT.json\n  dock-animation-upload PREPARED.json\n  keyboard-usb-snapshot [FILE]\n  keyboard-link-watch SECONDS OUTPUT\n  inspect FILE          Offline firmware inspection\n  new-scene [FILE.json]\n  render SCENE OUTPUT.gif\n  screen [OUTPUT.png]\nNo raw-send, firmware flash, bootloader or unbounded erase command is exposed.",
            ajazz_studio::version()
        ),
        Some(other) => bail!("Unknown command: {other}"),
    }
    Ok(())
}

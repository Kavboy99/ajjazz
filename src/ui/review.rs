//! Offline rendering of the real widgets. No native window, session, HID or workers.
use super::*;
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, path::Path};
type ReviewRects = Vec<(String, Rect)>;
fn rect_key() -> egui::Id {
    egui::Id::new("offline-layout-rects")
}
pub(super) fn record(ctx: &egui::Context, label: &str, rect: Rect) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<ReviewRects>(rect_key())
            .push((label.into(), rect))
    });
}

impl Studio {
    pub fn render_review(directory: &Path) -> Result<()> {
        std::fs::create_dir_all(directory)?;
        for (name, width, height) in [
            ("mouse-offline", 1100, 740),
            ("mouse-loading", 1100, 740),
            ("feedback-error", 1100, 740),
            ("preferences", 1100, 740),
            ("preferences-contrast", 1100, 740),
            ("preferences-large", 846, 569),
            ("dock-custom-large", 846, 569),
            ("dock-custom-reduced", 1100, 740),
            ("lighting", 1280, 880),
            ("dock", 1280, 880),
            ("keyboard", 1280, 880),
            ("encoder", 1280, 880),
            ("encoder-radio", 1100, 740),
            ("lighting-compact", 1100, 740),
            ("dock-compact", 1100, 740),
            ("keyboard-compact", 1100, 740),
            ("keyboard-rapid", 1100, 740),
            ("encoder-compact", 1100, 740),
            ("encoder-off", 1280, 880),
            ("lighting-saving", 1280, 880),
            ("dock-permission", 1100, 740),
            ("dock-gaming", 1100, 740),
            ("dock-thermals", 1100, 740),
            ("dock-gpu", 1100, 740),
            ("dock-pulse", 1100, 740),
            ("dock-custom", 1280, 880),
            ("dock-custom-compact", 1100, 740),
            ("dock-custom-effects", 1100, 740),
            ("updates", 1100, 740),
            ("updates-confirmation", 1100, 740),
            ("effects-gallery", 1280, 880),
            ("effects-gallery-compact", 1100, 740),
            ("effects-gallery-calm", 1100, 740),
            ("mouse", 1280, 880),
            ("mouse-compact", 1100, 740),
            ("collection", 1280, 880),
            ("collection-compact", 1100, 740),
            ("collection-full", 1100, 740),
            ("collection-offline", 1100, 740),
        ] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(2.0);
            let mut app = Self::build(&ctx, None, false);
            app.seed_review()?;
            app.page = if name.starts_with("preferences") {
                Page::Preferences
            } else if name.starts_with("updates") {
                Page::Research
            } else if name.starts_with("dock") {
                Page::Dock
            } else if name.starts_with("mouse") {
                Page::Mouse
            } else if name.starts_with("keyboard") || name.starts_with("encoder") {
                Page::Keyboard
            } else {
                Page::Lighting
            };
            app.preferences.reduce_motion = name.ends_with("reduced");
            app.preferences.high_contrast = name.ends_with("contrast");
            if name.ends_with("large") {
                app.preferences.scale = 130;
            }
            design::install(&ctx, &app.preferences);
            if name == "mouse-offline" || name == "mouse-loading" {
                app.mouse_snapshot = None;
                app.mouse_settings = None;
                app.busy = name == "mouse-loading";
            }
            if name == "feedback-error" {
                app.page = Page::Preferences;
                app.message("Не удалось сохранить настройки: устройство отключено. Подключите его и повторите сохранение.");
            }
            if name.starts_with("encoder") {
                app.keyboard_tab = KeyboardTab::Encoder;
                if name == "encoder-radio" {
                    app.memory_state = None;
                    app.playback_channel = PlaybackChannel::Radio;
                }
            }
            if app.page == Page::Lighting {
                app.selected.clear();
            }
            if name == "keyboard-rapid" {
                app.hall.rapid = true;
            }
            if name == "encoder-off" {
                app.memory_config.battery.enabled = false;
            }
            if name == "lighting-saving" {
                app.busy = true;
                app.memory_publishing = Some(0);
                app.memory_automatic = true;
            }
            if name == "dock-permission" {
                app.fps_requires_admin = true;
            }
            if name == "updates-confirmation" {
                app.update_prepared = Some(ajazz_studio::device_updates::Prepared {
                    target: ajazz_studio::device_updates::Target::Dock,
                    version: "OLED UI4 · b634c6a1d417".into(),
                    directory: std::path::PathBuf::new(),
                    executable: String::new(),
                    files: BTreeMap::new(),
                    backup: std::path::PathBuf::from("backup.json"),
                    notice: Some("Фото и GIF остаются в памяти дока.".into()),
                    image_sha256: None,
                    memory_fingerprint: None,
                });
                app.busy = true;
            }
            if name.starts_with("dock-custom") {
                if name.ends_with("effects") {
                    ctx.data_mut(|d| d.insert_temp(dock_composition_editor::open_key(), true));
                }
                app.screen_sample.as_mut().unwrap().graphics.temperatures =
                    ajazz_studio::temperatures::Readings {
                        cpu: Some(623),
                        gpu: Some(680),
                        storage: Some(530),
                        board: Some(500),
                    };
                app.screen_design = 3;
                app.dock_pages.pages = vec![dock_pages::Page {
                    kind: dock_pages::Kind::Custom,
                    ..Default::default()
                }];
                app.dock_editor_index = 0;
                app.dock_pages.selected = 0;
            }
            if let Some(kind) = match name {
                "dock-gaming" => Some(dock_pages::Kind::Gaming),
                "dock-thermals" => Some(dock_pages::Kind::Thermals),
                "dock-gpu" => Some(dock_pages::Kind::Gpu),
                "dock-pulse" => Some(dock_pages::Kind::Pulse),
                _ => None,
            } {
                app.screen_design = 2;
                app.dock_pages.pages = vec![dock_pages::Page {
                    kind,
                    ..Default::default()
                }];
                app.dock_pages.selected = 0;
                app.dock_editor_index = 0;
                app.screen_sample.as_mut().unwrap().graphics.temperatures =
                    ajazz_studio::temperatures::Readings {
                        cpu: Some(623),
                        gpu: Some(680),
                        storage: Some(530),
                        board: Some(500),
                    };
                app.screen_sample.as_mut().unwrap().graphics.power_deciwatts = Some(2270);
                app.screen_sample.as_mut().unwrap().graphics.load_history = (0..32)
                    .map(|i| ((i * 7 % 15) | ((i * 3 % 15) << 4)) as u8)
                    .collect();
            }
            if name.starts_with("effects-gallery") {
                let id = effect_gallery::open_key((false, 0));
                ctx.data_mut(|d| {
                    d.insert_temp(id, true);
                    d.insert_temp(
                        id.with("filter"),
                        if name.ends_with("calm") {
                            1usize
                        } else {
                            0usize
                        },
                    );
                });
            }
            if name.starts_with("collection") {
                lighting_collection::open(&ctx);
                if name == "collection-full" {
                    let page = app.memory_state.as_ref().unwrap().scenes[0].clone();
                    app.memory_state.as_mut().unwrap().scenes = vec![page; 8];
                }
                if name == "collection-offline" {
                    app.memory_state = None;
                    app.memory_clock = None;
                }
            }
            let mut textures: BTreeMap<u64, image::RgbaImage> = BTreeMap::new();
            let mut filters = BTreeMap::new();
            let mut final_output = None;
            // Settle egui's first-pass text/scroll measurements at the target DPI.
            for pass in 0..3 {
                ctx.data_mut(|d| d.insert_temp(rect_key(), ReviewRects::new()));
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(width as f32, height as f32),
                    )),
                    time: Some(pass as f64 * 0.25),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    app.sidebar(ctx);
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::new()
                                .fill(Color32::from_rgb(17, 19, 24))
                                .inner_margin(22.0),
                        )
                        .show(ctx, |ui| {
                            ui.add_space(4.0);
                            app.header(ui);
                            egui::ScrollArea::vertical()
                                .id_salt(("main_scroll", app.page))
                                .auto_shrink([false, false])
                                .show(ui, |ui| match app.page {
                                    Page::Lighting => app.lighting_page(ui),
                                    Page::Keyboard => app.keyboard_page(ui),
                                    Page::Dock => app.dock_page(ui, ctx),
                                    Page::Mouse => app.mouse_page(ui),
                                    Page::Research => app.research_page(ui),
                                    Page::Preferences => app.preferences_page(ui),
                                    _ => unreachable!(),
                                });
                        });
                    app.feedback(ctx);
                    app.update_confirmation(ctx);
                });
                for (id, delta) in &out.textures_delta.set {
                    let egui::TextureId::Managed(id) = id else {
                        anyhow::bail!("Unexpected native texture");
                    };
                    let egui::ImageData::Color(data) = &delta.image;
                    let raw: Vec<u8> = data.pixels.iter().flat_map(|p| p.to_array()).collect();
                    let patch =
                        image::RgbaImage::from_raw(data.size[0] as u32, data.size[1] as u32, raw)
                            .unwrap();
                    if let Some([x, y]) = delta.pos {
                        image::imageops::replace(
                            textures.get_mut(id).expect("Existing atlas"),
                            &patch,
                            x as i64,
                            y as i64,
                        );
                    } else {
                        textures.insert(*id, patch);
                    }
                    filters.insert(
                        *id,
                        delta.options.magnification == egui::TextureFilter::Nearest,
                    );
                }
                final_output = Some(out);
            }
            let out = final_output.unwrap();
            let rects = ctx
                .data(|d| d.get_temp::<ReviewRects>(rect_key()))
                .unwrap_or_default();
            let cards: Vec<_> = rects
                .iter()
                .filter(|(n, _)| n == "effect-card")
                .map(|(_, r)| *r)
                .collect();
            for pair in cards.windows(2) {
                ensure!(
                    pair[1].min.x - pair[0].max.x >= 8.0,
                    "Effect cards overlap: {name}"
                );
            }
            for (label, rect) in &rects {
                if matches!(
                    label.as_str(),
                    "dock-preview"
                        | "hall-save"
                        | "encoder-preview"
                        | "mouse-save"
                        | "effect-gallery"
                        | "collection-gallery"
                        | "update-card"
                        | "feedback-toast"
                ) {
                    ensure!(
                        rect.max.y <= height as f32 - 4.0,
                        "{name}: {label} below viewport: {}",
                        rect.max.y
                    );
                    ensure!(
                        rect.min.y >= 0.0 && rect.min.x >= 0.0 && rect.max.x <= width as f32,
                        "{name}: {label} outside viewport"
                    );
                }
            }
            let layout_rects: Vec<_> = rects
                .iter()
                .map(
                    |(n, r)| serde_json::json!({"name":n,"rect":[r.min.x,r.min.y,r.max.x,r.max.y]}),
                )
                .collect();
            let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
            let mut meshes = Vec::new();
            for p in primitives {
                let egui::epaint::Primitive::Mesh(mesh) = p.primitive else {
                    anyhow::bail!("Native paint callback in offline review");
                };
                let egui::TextureId::Managed(id) = mesh.texture_id else {
                    anyhow::bail!("Native texture in offline review");
                };
                let vertices: Vec<_> = mesh
                    .vertices
                    .iter()
                    .map(|v| {
                        serde_json::json!([v.pos.x, v.pos.y, v.uv.x, v.uv.y, v.color.to_array()])
                    })
                    .collect();
                meshes.push(serde_json::json!({"texture":id,"clip":[p.clip_rect.min.x,p.clip_rect.min.y,p.clip_rect.max.x,p.clip_rect.max.y],"vertices":vertices,"indices":mesh.indices}));
            }
            for (id, texture) in &textures {
                texture.save(directory.join(format!("{name}-texture-{id}.png")))?;
            }
            crate::ui::review::write_review(
                directory,
                name,
                &serde_json::json!({"width":width,"height":height,"scale":out.pixels_per_point,"nearest":filters,"meshes":meshes,"layout_rects":layout_rects,"hardware_access":false,"native_windows":false}),
            )?;
        }
        Ok(())
    }
    fn seed_review(&mut self) -> Result<()> {
        use ajazz_studio::keyboard_memory as memory;
        self.playback_channel = PlaybackChannel::Usb;
        self.endpoints = vec![
            Endpoint {
                path: "review".into(),
                vid: 0x0c45,
                pid: 0x80a0,
                product: "AK820MAX".into(),
                usage_page: 0xff68,
                usage: 97,
                interface: 2,
                role: "Keyboard".into(),
            },
            Endpoint {
                path: "review-dock".into(),
                vid: 0x3151,
                pid: 0x5007,
                product: "APEX Dock".into(),
                usage_page: 0xffff,
                usage: 2,
                interface: 1,
                role: "Dock".into(),
            },
        ];
        let mut scenes = vec![None; 8];
        for (slot, (name, effect, colors)) in [
            (
                "Северное сияние",
                Effect::Aurora,
                [[24, 56, 170], [40, 220, 185]],
            ),
            ("Плазма", Effect::Plasma, [[90, 50, 230], [255, 75, 155]]),
            ("Кометы", Effect::Comet, [[5, 12, 28], [80, 210, 255]]),
            (
                "Светлячки",
                Effect::Fireflies,
                [[5, 20, 12], [170, 245, 70]],
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut scene = Scene {
                name: name.into(),
                ..Default::default()
            };
            scene.layers[0].effect = effect;
            scene.layers[0].colors = colors;
            scene.layers.truncate(1);
            scenes[slot] = Some(memory::compile(&scene)?);
            if slot == 0 {
                self.scene = scene;
            }
        }
        self.memory_config.selected = Some(0);
        self.memory_config.battery.enabled = true;
        self.memory_config.encoder = [
            memory::Action::Brightness(-1),
            memory::Action::Brightness(1),
            memory::Action::Mode(1),
        ];
        let runtime = memory::Runtime {
            frame: 120,
            millis: 2400,
            selected: Some(0),
            brightness: 255,
            speed: 128,
            paused: false,
            color_transfer: 1,
            effect_count: 22,
            battery_supported: true,
            battery_percent: Some(78),
            battery_visible: false,
        };
        self.memory_state = Some(memory::MemoryState {
            config: self.memory_config.clone(),
            scenes,
            firmware: "1.36".into(),
            build_id: "review".into(),
            runtime: Some(runtime.clone()),
        });
        self.memory_clock = Some((runtime, Instant::now()));
        self.memory_slot = 0;
        self.memory_editor_slot = Some(0);
        self.time = 2.4;
        self.dock_telemetry = dock_telemetry::Config {
            style: 3,
            background: [5, 7, 11],
            foreground: [245, 247, 252],
            accent: [112, 174, 255],
            border: false,
            ..Default::default()
        };
        self.dock_pages = dock_pages::Playlist {
            enabled: true,
            autoplay: false,
            skip_offline: false,
            pages: [
                dock_pages::Kind::Summary,
                dock_pages::Kind::Clock,
                dock_pages::Kind::Calendar,
                dock_pages::Kind::Focus,
                dock_pages::Kind::Analog,
            ]
            .into_iter()
            .map(|kind| dock_pages::Page {
                kind,
                ..Default::default()
            })
            .collect(),
            ..Default::default()
        };
        self.page_status = Some(dock_pages::Status {
            capabilities: 255,
            playlist: self.dock_pages.clone(),
            active: 0,
            kind: 0,
            assets: 0,
            gif_frames: 0,
            erasing: None,
            progress: 0,
            save_pending: false,
            status: 0,
        });
        self.screen_online = true;
        self.screen_supported = Some(true);
        self.screen_sample = Some(dock_telemetry::Sample {
            cpu_permille: 230,
            ram_permille: 620,
            used_mib: 10000,
            total_mib: 16000,
            sequence: 1,
            graphics: ajazz_studio::graphics::Metrics {
                gpu_permille: Some(850),
                vram_used_mib: Some(6144),
                vram_total_mib: Some(8192),
                fps_tenths: Some(1440),
                ..Default::default()
            },
        });
        self.selected = keyboard_rows()
            .into_iter()
            .flatten()
            .filter(|k| matches!(k.physical.as_str(), "KeyW" | "KeyA" | "KeyS" | "KeyD"))
            .map(|k| k.id)
            .collect();
        self.hall_selection = Some(self.selected.clone());
        self.hall.actuation = 1.2;
        let mut info = [0; 48];
        info[8] = 0x36;
        info[9] = 1;
        info[17] = 78;
        self.snapshot = Some(Snapshot {
            schema: 1,
            endpoint: self.endpoints[0].clone(),
            info: KeyboardInfo::parse(&info)?,
            blocks: std::collections::BTreeMap::from([("rapid_trigger".into(), vec![0; 1024])]),
        });
        #[derive(serde::Deserialize)]
        struct MouseRaw {
            option0: Vec<u8>,
            option1: Vec<u8>,
        }
        let raw: MouseRaw =
            serde_json::from_str(include_str!("../../tests/fixtures/aj159-apex-read.json"))?;
        let options = ajazz_studio::protocol::MouseOptions::parse(&raw.option0)?;
        let mouse = mouse::MouseSnapshot {
            captured_utc: String::new(),
            source: "offline-review".into(),
            mouse_id: 0x800006db,
            last_battery_percent: Some(64),
            polling_hz: options.polling_hz()?,
            options,
            dpi: ajazz_studio::protocol::MouseDpi::parse(&raw.option1)?,
            raw_options: raw.option0,
            raw_dpi: raw.option1,
        };
        self.mouse_settings = Some(MouseSettings::from(&mouse));
        self.mouse_snapshot = Some(mouse);
        ensure!(
            !self.session_autosave && self.tray.is_none(),
            "Offline review must be isolated"
        );
        Ok(())
    }
}
fn write_review(directory: &Path, name: &str, data: &serde_json::Value) -> Result<()> {
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec(data)?,
    )?;
    Ok(())
}

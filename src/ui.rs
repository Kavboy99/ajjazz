use ajazz_studio::{
    dock_pages, dock_telemetry, firmware,
    hardware::{self, Endpoint, LightingReceipt, Snapshot},
    keyboard_animation::{Preset as AnimationPreset, Settings as AnimationSettings},
    keyboard_general::GeneralSettings,
    keyboard_macros::{
        self, Binding as MacroBinding, Edit as MacroEdit, Macro, MacroReceipt, MacroSnapshot,
    },
    keyboard_playback::{self, Channel as PlaybackChannel, Progress as PlaybackProgress},
    keyboard_settings::{self, HallEdit, KeyboardReceipt},
    keymap::{self, Assignment, KeymapEdit},
    lighting::{self, Pulse},
    model::{Blend, Effect, Layer, Scene, keyboard_rows},
    mouse::{self, MouseReceipt, MouseSettings},
    mouse_buttons::{
        self, Action as MouseAction, ButtonEdit, ButtonReceipt, ButtonSnapshot, Modifier,
    },
    protocol::{BUILTIN_LIGHTING, HallKey, KeyboardInfo, KeyboardLighting},
    screen::{self, Media, ScreenDesign},
};
use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const TEAL: Color32 = Color32::from_rgb(88, 157, 245);
const MUTED: Color32 = Color32::from_rgb(177, 184, 196);
const PANEL: Color32 = Color32::from_rgb(26, 29, 36);
const AMBER: Color32 = Color32::from_rgb(238, 185, 100);
const PRIMARY: Color32 = Color32::from_rgb(20, 100, 218);
fn surface() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(43, 48, 59)))
        .corner_radius(14.0)
        .inner_margin(18.0)
}
mod automatic;
mod design;
mod dock_composition_editor;
mod dock_editor;
mod effect_gallery;
mod keyboard_art;
mod keyboard_calibration;
mod keyboard_editor;
mod keyboard_memory;
mod lighting_collection;
mod lighting_editor;
mod mouse_editor;
mod review;
mod updates;
mod workflow;
#[derive(serde::Serialize, serde::Deserialize)]
struct Session {
    schema: u32,
    #[serde(default)]
    preferences: design::Preferences,
    #[serde(default)]
    last_page: Page,
    scene: Scene,
    #[serde(default)]
    memory_drafts: std::collections::BTreeMap<u8, Scene>,
    #[serde(default)]
    memory_editor_slot: Option<u8>,
    screen: ScreenDesign,
    #[serde(default)]
    dock_telemetry: dock_telemetry::Config,
    #[serde(default)]
    graphics_options: ajazz_studio::graphics::Options,
    #[serde(default)]
    dock_pages: dock_pages::Playlist,
    lighting_backup: Option<std::path::PathBuf>,
    hall_backup: Option<std::path::PathBuf>,
    mouse_backup: Option<std::path::PathBuf>,
    #[serde(default)]
    keymap_backup: Option<std::path::PathBuf>,
    #[serde(default)]
    dks_backup: Option<std::path::PathBuf>,
    #[serde(default)]
    macro_backup: Option<std::path::PathBuf>,
    #[serde(default)]
    macro_draft: Vec<Macro>,
    #[serde(default)]
    general_backup: Option<std::path::PathBuf>,
    #[serde(default)]
    buttons_backup: Option<std::path::PathBuf>,
    #[serde(default)]
    dock_slot: Option<u8>,
    #[serde(default)]
    media_path: Option<std::path::PathBuf>,
    #[serde(default = "default_animation_delay")]
    animation_delay_ms: u8,
    #[serde(default)]
    keyboard_animation_channel: PlaybackChannel,
    #[serde(default = "default_keyboard_interval")]
    keyboard_animation_interval: u64,
    #[serde(default)]
    keyboard_animation: AnimationSettings,
}
fn keyboard_usage_editor(ui: &mut egui::Ui, id: &str, usage: &mut u8) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(keymap::usage_label(*usage))
        .show_ui(ui, |ui| {
            for key in keyboard_rows()
                .into_iter()
                .flatten()
                .filter(|key| key.hid != 0)
            {
                ui.selectable_value(usage, key.hid, key.physical);
            }
        });
}
fn keyboard_pair_editor(ui: &mut egui::Ui, first: &mut u8, second: &mut u8) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "Клавиша 1: {} · клавиша 2: {}",
            keymap::usage_label(*first),
            keymap::usage_label(*second)
        ));
        if ui.button("Поменять порядок").clicked() {
            std::mem::swap(first, second);
        }
    });
    ui.small("Для создания или удаления пары выделите ровно две её клавиши на схеме.");
}
fn default_keyboard_interval() -> u64 {
    33
}
fn default_animation_delay() -> u8 {
    50
}
fn session_path() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|root| std::path::PathBuf::from(root).join("AjazzStudio/session.json"))
}
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
enum Page {
    #[default]
    Lighting,
    Keyboard,
    Mouse,
    Dock,
    Devices,
    Research,
    Preferences,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Lighting => "Подсветка",
            Self::Keyboard => "Клавиши",
            Self::Mouse => "Мышь",
            Self::Dock => "Экран дока",
            Self::Devices => "Устройства",
            Self::Research => "Обновления",
            Self::Preferences => "Настройки",
        }
    }
}
enum Event {
    UpdatePrepared(Result<ajazz_studio::device_updates::Prepared, String>),
    UpdateFinished(Result<String, String>),
    Connections(Result<Vec<Endpoint>, String>),
    DockSaved(Result<dock_pages::Status, String>),
    KeyboardRuntime(
        u64,
        Result<Option<ajazz_studio::keyboard_memory::Runtime>, String>,
    ),
    CalibrationUpdate(ajazz_studio::keyboard_calibration::Update),
    CalibrationFinished(Result<ajazz_studio::keyboard_calibration::Progress, String>),
    KeyboardMemory(Result<ajazz_studio::keyboard_memory::MemoryState, String>),
    EditorRead(Result<(dock_telemetry::Config, dock_pages::Status), String>),
    TimerResult(Result<ajazz_studio::dock_timer::Status, String>),
    PagesResult(Result<dock_pages::Status, String>, bool),
    PagesPoll(Result<dock_pages::Status, String>),
    DockBackground(Result<Option<dock_telemetry::BackgroundStatus>, String>),
    ScreenProbe(Result<dock_telemetry::Reply, String>),
    ScreenApplied(Result<(), String>, bool),
    ScreenUpdate(dock_telemetry::Update),
    ScreenFinished(Result<(), String>),
    KeyboardInfo(Result<KeyboardInfo, String>),
    PlaybackProgress(PlaybackProgress),
    PlaybackFinished(Result<keyboard_playback::Receipt, String>),
    DockProgress(f32),
    DockUpload(Result<ajazz_studio::dock_display::DisplayReceipt, String>),
    DockInfo(Result<ajazz_studio::dock::DockInfo, String>),
    MouseFirmwareInfo(Result<ajazz_studio::mouse_firmware::Identity, String>),
    Snapshot(Result<Snapshot, String>),
    Lighting(Result<LightingReceipt, String>),
    Mouse(Result<mouse::MouseSnapshot, String>),
    MouseWrite(Result<MouseReceipt, String>),
    ButtonsRead(Result<ButtonSnapshot, String>),
    ButtonsWrite(Result<ButtonReceipt, String>),
    KeyboardWrite(Result<KeyboardReceipt, String>),
    KeymapWrite(Result<KeyboardReceipt, String>),
    DksWrite(Result<KeyboardReceipt, String>),
    MacrosRead(Result<MacroSnapshot, String>, bool),
    MacrosWrite(Result<MacroReceipt, String>),
    GeneralWrite(Result<KeyboardReceipt, String>),
    Done(Result<String, String>),
}
#[derive(Clone, Copy, PartialEq)]
enum KeyboardTab {
    Calibration,
    Encoder,
    Hall,
    Keymap,
    General,
    Macros,
    Test,
}
#[derive(Clone, Copy, PartialEq)]
enum DockTab {
    Pages,
    Telemetry,
    Media,
}
pub struct Studio {
    preferences: design::Preferences,
    navigation: Vec<Page>,
    review_mode: bool,
    tray: Option<crate::tray::Tray>,
    hidden: bool,
    hide_requested: bool,
    window_hidden: Arc<AtomicBool>,
    exit_requested: bool,
    connection_polled: Instant,
    connection_pending: bool,
    memory_changes: std::collections::BTreeMap<u8, (Instant, Scene)>,
    memory_activation: Option<u8>,
    dock_change: Option<Instant>,
    dock_save_pending: bool,
    dock_save_error: Option<String>,
    memory_clock: Option<(ajazz_studio::keyboard_memory::Runtime, Instant)>,
    memory_clock_polled: Instant,
    memory_clock_pending: bool,
    memory_clock_failed: bool,
    battery_preview_percent: u8,
    memory_generation: u64,
    auto_reads: BTreeSet<(&'static str, bool)>,
    memory_encoder_pending: Option<Instant>,
    memory_error: Option<String>,
    memory_notice: String,
    memory_automatic: bool,
    memory_open_after_read: bool,
    memory_publishing: Option<u8>,
    memory_drafts: std::collections::BTreeMap<u8, Scene>,
    memory_editor_slot: Option<u8>,
    calibration_commands: Option<mpsc::Sender<ajazz_studio::keyboard_calibration::Command>>,
    calibration_progress: ajazz_studio::keyboard_calibration::Progress,
    calibration_started: bool,
    calibration_saving: bool,
    calibration_saved: bool,
    calibration_error: Option<String>,
    memory_state: Option<ajazz_studio::keyboard_memory::MemoryState>,
    memory_config: ajazz_studio::keyboard_memory::Config,
    memory_slot: usize,
    memory_delete: Option<u8>,
    memory_deleting: Option<u8>,
    memory_preview: bool,
    page: Page,
    scene: Scene,
    layer: usize,
    selected: BTreeSet<u8>,
    dock_surface: bool,
    playing: bool,
    playback_cancel: Option<Arc<AtomicBool>>,
    playback_progress: Option<PlaybackProgress>,
    playback_channel: PlaybackChannel,
    playback_interval: u64,
    playback_repeat: bool,
    keyboard_animation: AnimationSettings,
    time: f32,
    media_time: f32,
    last: Instant,
    pulses: Vec<Pulse>,
    endpoints: Vec<Endpoint>,
    snapshot: Option<Snapshot>,
    keyboard_info: Option<KeyboardInfo>,
    lighting_backup: Option<std::path::PathBuf>,
    keyboard_lighting: Option<(Vec<u8>, KeyboardLighting)>,
    keyboard_effect_command: Option<Vec<u8>>,
    lighting_auto_apply: bool,
    lighting_pending: Option<Instant>,
    busy: bool,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    status: String,
    status_changed: Instant,
    logs: Vec<String>,
    screen: ScreenDesign,
    dock_tab: DockTab,
    dock_telemetry: dock_telemetry::Config,
    graphics_options: ajazz_studio::graphics::Options,
    screen_commands: Option<mpsc::Sender<dock_telemetry::Command>>,
    graphics_change: Option<Instant>,
    screen_online: bool,
    fps_requires_admin: bool,
    screen_sample: Option<dock_telemetry::Sample>,
    screen_supported: Option<bool>,
    screen_design: u8,
    dock_pages: dock_pages::Playlist,
    page_status: Option<dock_pages::Status>,
    dock_editor_index: usize,
    dock_card_textures: Vec<egui::TextureHandle>,
    dock_preview_key: Vec<u8>,
    dock_drag_start: Option<(usize, Pos2)>,
    dock_timer: Option<ajazz_studio::dock_timer::Status>,
    dock_poll_pending: bool,
    pages_poll_inflight: bool,
    pages_polled: Instant,
    delete_asset_confirmation: Option<u8>,
    screen_sample_time: Instant,
    #[cfg(windows)]
    screen_counters: Option<dock_telemetry::windows::Counters>,
    media: Option<Media>,
    media_path: Option<std::path::PathBuf>,
    animation_delay_ms: u8,
    screen_texture: Option<egui::TextureHandle>,
    font: Option<ab_glyph::FontArc>,
    firmware_text: String,
    update_pending: Option<ajazz_studio::device_updates::Target>,
    update_prepared: Option<ajazz_studio::device_updates::Prepared>,
    update_running: bool,
    update_open: bool,
    update_notice: Option<String>,
    mouse_firmware_info: Option<ajazz_studio::mouse_firmware::Identity>,
    dock_info: Option<ajazz_studio::dock::DockInfo>,
    dock_slot: Option<u8>,
    dock_progress: Option<f32>,
    mouse_snapshot: Option<mouse::MouseSnapshot>,
    hall: HallKey,
    hall_selection: Option<BTreeSet<u8>>,
    hall_backup: Option<std::path::PathBuf>,
    keyboard_tab: KeyboardTab,
    keyboard_test_text: String,
    keyboard_test_log: Vec<String>,
    keyboard_test_started: Instant,
    general_settings: Option<GeneralSettings>,
    general_backup: Option<std::path::PathBuf>,
    keymap_assignment: Assignment,
    dks_config: Option<ajazz_studio::keyboard_dks::DksConfig>,
    dks_backup: Option<std::path::PathBuf>,
    macro_backup: Option<std::path::PathBuf>,
    macro_snapshot: Option<MacroSnapshot>,
    macro_draft: Vec<Macro>,
    macro_slot: u8,
    macro_mode: u8,
    macro_repeats: u8,
    keymap_selection: Option<BTreeSet<u8>>,
    keymap_backup: Option<std::path::PathBuf>,
    mouse_settings: Option<MouseSettings>,
    mouse_backup: Option<std::path::PathBuf>,
    mouse_buttons_tab: bool,
    buttons_snapshot: Option<ButtonSnapshot>,
    buttons_slot: u8,
    buttons_action: MouseAction,
    buttons_backup: Option<std::path::PathBuf>,
    session_bytes: Vec<u8>,
    session_autosave: bool,
    last_session_save: Instant,
}
impl Studio {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::build(
            &cc.egui_ctx,
            crate::tray::Tray::new(cc.egui_ctx.clone(), cc).ok(),
            true,
        )
    }
    fn build(ctx: &egui::Context, tray: Option<crate::tray::Tray>, live: bool) -> Self {
        ctx.set_theme(egui::Theme::Dark);
        if let Some(windir) = std::env::var_os("WINDIR") {
            let path = std::path::PathBuf::from(windir).join("Fonts/segoeui.ttf");
            if let Ok(data) = std::fs::read(path) {
                let mut fonts = egui::FontDefinitions::default();
                fonts
                    .font_data
                    .insert("segoe".into(), egui::FontData::from_owned(data).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "segoe".into());
                ctx.set_fonts(fonts);
            }
        }
        design::install(ctx, &design::Preferences::default());
        let (tx, rx) = mpsc::channel();
        let endpoints = if live {
            hardware::scan().unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut app = Self {
            preferences: Default::default(),
            navigation: Vec::new(),
            review_mode: !live,
            tray,
            hidden: false,
            hide_requested: false,
            window_hidden: Arc::new(AtomicBool::new(false)),
            exit_requested: false,
            connection_polled: Instant::now(),
            connection_pending: false,
            memory_changes: Default::default(),
            memory_activation: None,
            dock_change: None,
            dock_save_pending: false,
            dock_save_error: None,
            memory_clock: None,
            memory_clock_polled: Instant::now(),
            memory_clock_pending: false,
            memory_clock_failed: false,
            battery_preview_percent: 65,
            memory_generation: 0,
            auto_reads: BTreeSet::new(),
            memory_encoder_pending: None,
            memory_error: None,
            memory_notice: String::new(),
            memory_automatic: false,
            memory_open_after_read: false,
            memory_publishing: None,
            memory_drafts: Default::default(),
            memory_editor_slot: None,
            calibration_commands: None,
            calibration_progress: Default::default(),
            calibration_started: false,
            calibration_saving: false,
            calibration_saved: false,
            calibration_error: None,
            memory_state: None,
            memory_config: Default::default(),
            memory_slot: 0,
            memory_delete: None,
            memory_deleting: None,
            memory_preview: true,
            page: if std::env::args().any(|arg| arg == "--dock-screen") {
                Page::Dock
            } else {
                Page::Lighting
            },
            scene: Scene::default(),
            layer: 0,
            selected: BTreeSet::new(),
            dock_surface: false,
            playing: true,
            playback_cancel: None,
            playback_progress: None,
            playback_channel: if endpoints
                .iter()
                .any(|e| e.pid == 0x80a0 && e.usage_page == 0xff68)
            {
                PlaybackChannel::Usb
            } else {
                PlaybackChannel::Radio
            },
            playback_interval: default_keyboard_interval(),
            playback_repeat: true,
            keyboard_animation: AnimationSettings::default(),
            time: 0.0,
            media_time: 0.0,
            last: Instant::now(),
            pulses: Vec::new(),
            endpoints,
            snapshot: None,
            keyboard_info: None,
            lighting_backup: None,
            keyboard_lighting: None,
            keyboard_effect_command: None,
            lighting_auto_apply: true,
            lighting_pending: None,
            busy: false,
            tx,
            rx,
            status: String::new(),
            status_changed: Instant::now(),
            logs: vec!["Запуск: перечисление HID без отправки команд".into()],
            screen: Default::default(),
            dock_tab: DockTab::Pages,
            dock_telemetry: Default::default(),
            graphics_options: Default::default(),
            graphics_change: None,
            screen_commands: None,
            screen_online: false,
            fps_requires_admin: false,
            screen_sample: None,
            screen_supported: None,
            screen_design: 0,
            dock_pages: Default::default(),
            page_status: None,
            dock_editor_index: 0,
            dock_card_textures: Vec::new(),
            dock_preview_key: Vec::new(),
            dock_drag_start: None,
            dock_timer: None,
            dock_poll_pending: false,
            pages_poll_inflight: false,
            pages_polled: Instant::now(),
            delete_asset_confirmation: None,
            screen_sample_time: Instant::now(),
            #[cfg(windows)]
            screen_counters: if live {
                dock_telemetry::windows::Counters::new().ok()
            } else {
                None
            },
            media: None,
            media_path: None,
            animation_delay_ms: default_animation_delay(),
            screen_texture: None,
            font: screen::load_system_font(),
            firmware_text: String::new(),
            update_pending: None,
            update_prepared: None,
            update_running: false,
            update_open: false,
            update_notice: None,
            mouse_firmware_info: None,
            dock_info: None,
            dock_slot: None,
            dock_progress: None,
            mouse_snapshot: None,
            hall: HallKey {
                axis: 1,
                rapid: false,
                whole_travel: false,
                continuous: false,
                actuation: 1.2,
                press: 0.1,
                release: 0.1,
            },
            hall_selection: None,
            hall_backup: None,
            keyboard_tab: KeyboardTab::Hall,
            keyboard_test_text: String::new(),
            keyboard_test_log: Vec::new(),
            keyboard_test_started: Instant::now(),
            general_settings: None,
            general_backup: None,
            keymap_assignment: Assignment::Default,
            dks_config: None,
            dks_backup: None,
            macro_backup: None,
            macro_snapshot: None,
            macro_draft: Vec::new(),
            macro_slot: 0,
            macro_mode: 0,
            macro_repeats: 1,
            keymap_selection: None,
            keymap_backup: None,
            mouse_settings: None,
            mouse_backup: None,
            mouse_buttons_tab: false,
            buttons_snapshot: None,
            buttons_slot: 4,
            buttons_action: MouseAction::Back,
            buttons_backup: None,
            session_bytes: Vec::new(),
            session_autosave: live,
            last_session_save: Instant::now(),
        };
        if live && let Some(path) = session_path().filter(|path| path.exists()) {
            let saved =
                ajazz_studio::storage::read_json::<Session>(&path, 2_100_000).and_then(|s| {
                    anyhow::ensure!(s.schema == 1, "Unknown session format");
                    s.scene.validate()?;
                    anyhow::ensure!(
                        s.memory_drafts.len() <= 9 && s.memory_drafts.keys().all(|v| *v <= 8),
                        "Invalid saved lighting drafts"
                    );
                    for draft in s.memory_drafts.values() {
                        draft.validate()?;
                    }
                    s.screen.validate()?;
                    s.dock_telemetry.validate()?;
                    s.dock_pages.validate()?;
                    s.keyboard_animation.validate()?;
                    Ok(s)
                });
            match saved {
                Ok(saved) => {
                    app.preferences = saved.preferences;
                    app.preferences.scale = app.preferences.scale.clamp(100, 130);
                    app.page = saved.last_page;
                    design::install(ctx, &app.preferences);
                    ctx.data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new("studio-opaque-chrome"),
                            app.preferences.reduce_transparency,
                        )
                    });
                    if app.preferences.reduce_motion {
                        app.playing = false;
                    }
                    app.keyboard_animation = saved.keyboard_animation;
                    app.scene = saved.scene;
                    app.memory_drafts = saved.memory_drafts;
                    app.memory_editor_slot = saved.memory_editor_slot.filter(|v| *v < 8);
                    if let Some(slot) = app.memory_editor_slot {
                        app.memory_slot = usize::from(slot);
                    }
                    app.screen = saved.screen;
                    app.dock_telemetry = saved.dock_telemetry;
                    app.graphics_options = saved.graphics_options;
                    app.dock_pages = saved.dock_pages;
                    app.lighting_backup = saved.lighting_backup;
                    app.hall_backup = saved.hall_backup;
                    app.mouse_backup = saved.mouse_backup;
                    app.keymap_backup = saved.keymap_backup;
                    app.dks_backup = saved.dks_backup;
                    app.macro_backup = saved.macro_backup;
                    app.macro_draft = saved.macro_draft;
                    app.general_backup = saved.general_backup;
                    app.buttons_backup = saved.buttons_backup;
                    app.dock_slot = saved.dock_slot.filter(|v| *v < 5);
                    app.animation_delay_ms = saved.animation_delay_ms.clamp(20, 255);
                    app.playback_channel = saved.keyboard_animation_channel;
                    app.playback_interval = saved
                        .keyboard_animation_interval
                        .clamp(app.playback_channel.minimum_interval(), 5000);
                    if let Some(path) = saved.media_path {
                        match Media::load(&path) {
                            Ok(media) => {
                                app.media = Some(media);
                                app.media_path = Some(path);
                            }
                            Err(error) => {
                                app.message(format!(
                                    "Не удалось восстановить медиа {}: {error:#}",
                                    path.display()
                                ));
                                app.media_path = Some(path);
                            }
                        }
                    }
                }
                Err(e) => {
                    app.session_autosave = false;
                    app.message(format!("Не удалось открыть рабочую сцену: {e:#}. Автосохранение приостановлено; исходный файл сохранён. Откройте исправную сцену для продолжения."));
                }
            }
        }
        // Choose only the initial UI channel from detected control interfaces.
        // Actual writes remain pinned to the subsequently read snapshot path.
        if let Some(channel) = PlaybackChannel::detect(&app.endpoints) {
            app.playback_channel = channel;
        }
        app
    }
    fn save_session(&mut self) {
        if !self.session_autosave {
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            let saved = Session {
                schema: 1,
                preferences: self.preferences.clone(),
                last_page: self.page,
                scene: self.scene.clone(),
                memory_drafts: self.memory_drafts.clone(),
                memory_editor_slot: self.memory_editor_slot,
                screen: self.screen.clone(),
                dock_telemetry: self.dock_telemetry.clone(),
                graphics_options: self.graphics_options.clone(),
                dock_pages: self.dock_pages.clone(),
                lighting_backup: self.lighting_backup.clone(),
                hall_backup: self.hall_backup.clone(),
                mouse_backup: self.mouse_backup.clone(),
                keymap_backup: self.keymap_backup.clone(),
                dks_backup: self.dks_backup.clone(),
                macro_backup: self.macro_backup.clone(),
                macro_draft: self.macro_draft.clone(),
                general_backup: self.general_backup.clone(),
                buttons_backup: self.buttons_backup.clone(),
                dock_slot: self.dock_slot,
                media_path: self.media_path.clone(),
                animation_delay_ms: self.animation_delay_ms,
                keyboard_animation_channel: self.playback_channel,
                keyboard_animation_interval: self.playback_interval,
                keyboard_animation: self.keyboard_animation.clone(),
            };
            saved.scene.validate()?;
            saved.screen.validate()?;
            saved.dock_telemetry.validate()?;
            saved.dock_pages.validate()?;
            saved.keyboard_animation.validate()?;
            let bytes = serde_json::to_vec(&saved)?;
            if bytes != self.session_bytes {
                let path =
                    session_path().ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA unavailable"))?;
                ajazz_studio::storage::write_json_atomic(&path, &saved)?;
                self.session_bytes = bytes;
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.message(format!("Ошибка сохранения рабочей сцены: {e:#}"));
        }
        self.last_session_save = Instant::now();
    }
    fn message(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.status = msg.clone();
        self.status_changed = Instant::now();
        self.logs.push(msg);
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }
    fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            if !matches!(
                event,
                Event::DockProgress(_)
                    | Event::PlaybackProgress(_)
                    | Event::ScreenUpdate(_)
                    | Event::PagesPoll(_)
                    | Event::DockBackground(_)
                    | Event::CalibrationUpdate(_)
                    | Event::KeyboardRuntime(..)
                    | Event::ScreenFinished(_)
                    | Event::Connections(_)
                    | Event::DockSaved(_)
            ) {
                self.busy = self.calibration_commands.is_some();
            }
            match event {
                Event::UpdatePrepared(result) => {
                    self.update_running = false;
                    match result {
                        Ok(p) => {
                            self.update_prepared = Some(p);
                            self.busy = true;
                        }
                        Err(e) => {
                            self.busy = false;
                            self.update_notice = Some(e);
                        }
                    }
                }
                Event::UpdateFinished(result) => {
                    self.update_running = false;
                    self.busy = false;
                    self.update_notice = Some(result.unwrap_or_else(|e| e));
                    self.screen_design = 0;
                    self.memory_clock_failed = false;
                    self.connection_polled = Instant::now() - Duration::from_secs(10);
                }
                Event::Connections(result) => {
                    self.connection_pending = false;
                    if let Ok(endpoints) = result {
                        let previous = PlaybackChannel::detect(&self.endpoints);
                        let channel = PlaybackChannel::detect(&endpoints);
                        self.endpoints = endpoints;
                        if channel.is_none()
                            && !self.busy
                            && (previous.is_some()
                                || self.memory_state.is_some()
                                || self.snapshot.is_some())
                        {
                            self.reset_keyboard_connection();
                        }
                        if !self.busy
                            && self.playback_cancel.is_none()
                            && let Some(channel) = channel
                            && channel != self.playback_channel
                        {
                            self.playback_channel = channel;
                            self.reset_keyboard_connection();
                        }
                    }
                }
                Event::DockSaved(result) => {
                    self.dock_save_pending = false;
                    match result {
                        Ok(status) => {
                            self.dock_save_error = None;
                            self.update_pages_status(status, false);
                        }
                        Err(error) => {
                            self.dock_save_error = Some(error.clone());
                            self.message(format!("Не удалось сохранить экраны: {error}"));
                        }
                    }
                }
                Event::KeyboardRuntime(generation, result) => {
                    self.memory_clock_pending = false;
                    if generation == self.memory_generation && !self.busy {
                        match result {
                            Ok(Some(runtime)) => self.accept_memory_clock(runtime),
                            Ok(None) => {}
                            Err(_) => {
                                self.memory_clock_failed = true;
                                self.memory_clock = None;
                            }
                        }
                    }
                }
                Event::CalibrationUpdate(update) => match update {
                    ajazz_studio::keyboard_calibration::Update::Started => {
                        self.calibration_started = true;
                        self.message("Калибровка началась: полностью нажмите и отпустите каждую из 82 клавиш.");
                    }
                    ajazz_studio::keyboard_calibration::Update::Progress(progress) => {
                        self.calibration_progress = progress
                    }
                    ajazz_studio::keyboard_calibration::Update::Notice(message) => {
                        self.message(message)
                    }
                },
                Event::CalibrationFinished(result) => {
                    self.busy = false;
                    self.calibration_commands = None;
                    self.calibration_started = false;
                    self.calibration_saving = false;
                    match result {
                        Ok(progress) => {
                            self.calibration_saved = true;
                            self.calibration_progress = progress;
                            self.message(
                                "Калибровка 82 клавиш сохранена; остальные настройки проверены.",
                            );
                        }
                        Err(error) => {
                            self.calibration_error = Some(error.clone());
                            self.message(format!("Калибровка: {error}"));
                        }
                    }
                }
                Event::KeyboardMemory(result) => match result {
                    Ok(state) => {
                        if let Some(slot) = self.memory_deleting.take() {
                            self.forget_deleted_memory_slot(slot);
                        }
                        if let Some(runtime) = state.runtime.clone() {
                            self.accept_memory_clock(runtime);
                        }
                        if self.memory_encoder_pending.is_none() {
                            self.memory_config = state.config.clone();
                        }
                        let initial_slot = state.config.selected.unwrap_or(0);
                        self.memory_state = Some(state);
                        self.memory_delete = None;
                        self.memory_error = None;
                        if let Some(slot) = self.memory_publishing.take()
                            && self
                                .memory_drafts
                                .get(&slot)
                                .and_then(|draft| {
                                    ajazz_studio::keyboard_memory::compile(draft).ok()
                                })
                                .zip(
                                    self.memory_state
                                        .as_ref()
                                        .and_then(|s| s.scenes[usize::from(slot)].as_ref()),
                                )
                                .is_some_and(|(draft, saved)| {
                                    ajazz_studio::keyboard_memory::same_program(&draft, saved)
                                })
                        {
                            self.memory_drafts.remove(&slot);
                        }
                        if self.memory_open_after_read {
                            self.choose_memory_slot(usize::from(initial_slot));
                            self.memory_open_after_read = false;
                        }
                        if !self.memory_automatic {
                            self.message(self.memory_notice.clone());
                        }
                        self.memory_automatic = false;
                    }
                    Err(error) => {
                        self.memory_automatic = false;
                        self.memory_deleting = None;
                        self.memory_publishing = None;
                        self.memory_open_after_read = false;
                        self.memory_error = Some(error.clone());
                        self.message(format!("Память клавиатуры: {error}"));
                    }
                },
                Event::EditorRead(result) => match result {
                    Ok((config, status)) => {
                        self.dock_telemetry = config;
                        self.dock_editor_index = usize::from(status.playlist.selected);
                        self.update_pages_status(status, true);
                        self.message("Оформление и порядок экранов прочитаны с дока.");
                    }
                    Err(e) => self.message(format!("Чтение экрана: {e}")),
                },
                Event::ScreenUpdate(dock_telemetry::Update::Editor(config, status)) => {
                    let replace = self.dock_change.is_none() && !self.dock_save_pending;
                    if replace {
                        self.dock_telemetry = config;
                        self.dock_editor_index = usize::from(status.playlist.selected);
                    }
                    self.update_pages_status(status, replace);
                }
                Event::ScreenUpdate(dock_telemetry::Update::Connection(online)) => {
                    self.screen_online = online;
                    if !online {
                        self.page_status = None;
                        if self.dock_save_pending {
                            self.dock_save_pending = false;
                            self.dock_change = Some(Instant::now());
                        }
                    }
                }
                Event::TimerResult(result) => match result {
                    Ok(s) => self.dock_timer = Some(s),
                    Err(e) => self.message(format!("Таймер: {e}")),
                },
                Event::ScreenUpdate(dock_telemetry::Update::Timer(s)) => self.dock_timer = Some(s),
                Event::DockBackground(result) => {
                    self.dock_poll_pending = false;
                    if !self.busy
                        && self.screen_commands.is_none()
                        && let Ok(Some((status, timer))) = result
                    {
                        self.update_pages_status(status, false);
                        if let Some(timer) = timer {
                            self.dock_timer = Some(timer);
                        }
                    }
                }
                Event::PagesPoll(result) => {
                    if self.pages_poll_inflight {
                        self.pages_poll_inflight = false;
                        self.busy =
                            self.calibration_commands.is_some() || self.playback_cancel.is_some();
                    }
                    match result {
                        Ok(status) => self.update_pages_status(status, false),
                        Err(error) => {
                            self.page_status = None;
                            self.message(format!("Связь с экраном: {error}. Откройте меню «•••» → «Обновить подключение»."));
                        }
                    }
                }
                Event::PagesResult(result, replace) => match result {
                    Ok(status) => {
                        self.update_pages_status(status, replace);
                        self.message("Состояние страниц прочитано и подтверждено доком.");
                    }
                    Err(error) => self.message(format!("Страницы дока: {error}")),
                },
                Event::ScreenUpdate(dock_telemetry::Update::Pages(status, replace)) => {
                    self.update_pages_status(status, replace);
                }
                Event::ScreenUpdate(dock_telemetry::Update::Design(version)) => {
                    self.screen_design = version
                }
                Event::ScreenUpdate(dock_telemetry::Update::Notice(message)) => {
                    if self.dock_save_pending && message.starts_with("Настройки дока:")
                    {
                        self.dock_save_pending = false;
                        self.dock_save_error = Some(message.clone());
                    }
                    if message.contains("access denied")
                        || message.contains("Разрешите фоновый сборщик")
                        || message.contains("Native FPS StartTrace: Windows 5")
                    {
                        self.fps_requires_admin = true;
                        self.message(
                            "Для игровых FPS нужно разрешить фоновый сборщик показателей.",
                        );
                        continue;
                    }
                    self.message(message)
                }
                Event::ScreenProbe(Ok(reply)) => {
                    self.screen_design = reply.design_version;
                    self.screen_supported = Some(true);
                    self.dock_telemetry = reply.config;
                    self.message(
                        "Экранная прошивка AJSC v1 ответила; оформление прочитано с дока.",
                    );
                }
                Event::ScreenProbe(Err(error)) => {
                    self.screen_supported = Some(false);
                    self.message(format!("Проверка экранной прошивки: {error}"));
                }
                Event::ScreenApplied(result, save) => match result {
                    Ok(()) => {
                        self.screen_supported = Some(true);
                        self.message(if save {
                            "Оформление принято; экрану отправлен запрос сохранения."
                        } else {
                            "Оформление подтверждено экраном и применяется в RAM."
                        });
                    }
                    Err(error) => self.message(format!("Настройка экрана остановлена: {error}")),
                },
                Event::ScreenUpdate(dock_telemetry::Update::Sample(sample)) => {
                    if sample.graphics.fps_tenths.is_some()
                        || ajazz_studio::metrics_agent::snapshot(&self.graphics_options)
                            .is_some_and(|s| s.notice.is_none())
                    {
                        self.fps_requires_admin = false;
                    }
                    if sample.sequence == 1 {
                        self.message("Показатели подключены");
                    }
                    self.screen_sample = Some(sample);
                    self.screen_sample_time = Instant::now();
                    self.screen_supported = Some(true);
                }
                Event::ScreenUpdate(dock_telemetry::Update::Settings(_config, save)) => {
                    if save {
                        self.dock_save_pending = false;
                        self.dock_save_error = None;
                    }
                    self.message(if save {
                        "Изменения сохранены"
                    } else {
                        "Оформление применено"
                    });
                }
                Event::ScreenFinished(result) => {
                    self.screen_online = false;
                    if self.dock_save_pending {
                        self.dock_save_pending = false;
                        self.dock_save_error =
                            Some("Передача остановлена до подтверждения сохранения".into());
                    }
                    self.screen_commands = None;
                    self.dock_poll_pending = false;
                    self.message(match result {
                        Ok(()) => {
                            "Поток показателей остановлен; док продолжает автономный показ страниц."
                                .into()
                        }
                        Err(error) => format!("Передача показателей остановлена: {error}"),
                    });
                }
                Event::KeyboardInfo(result) => match result {
                    Ok(info) => {
                        self.message(format!(
                            "Возможности прошивки {} прочитаны без изменения настроек",
                            info.version
                        ));
                        self.keyboard_info = Some(info);
                    }
                    Err(error) => self.message(format!("Чтение возможностей остановлено: {error}")),
                },
                Event::PlaybackProgress(progress) => {
                    self.playback_progress = Some(progress);
                    if self
                        .playback_cancel
                        .as_ref()
                        .is_some_and(|cancel| !cancel.load(Ordering::Relaxed))
                    {
                        self.status = format!(
                            "Анимация на клавиатуре · {} · {} кадров отправлено",
                            self.playback_channel.label(),
                            progress.frame
                        );
                    }
                }
                Event::PlaybackFinished(result) => {
                    self.playback_cancel = None;
                    match result {
                        Ok(receipt) => {
                            self.lighting_backup = Some(receipt.backup_path);
                            self.message(format!("Анимация завершена · {} кадров · исходная подсветка восстановлена, настройки проверены", receipt.frames));
                            self.logs.push(format!(
                                "Копия настроек: {}",
                                receipt.configuration_path.display()
                            ));
                        }
                        Err(error) => {
                            self.snapshot = None;
                            self.keyboard_effect_command = None;
                            self.message(format!("Анимация остановлена: {error}"));
                        }
                    }
                }
                Event::DockProgress(progress) => self.dock_progress = Some(progress),
                Event::DockUpload(Ok(receipt)) => {
                    self.dock_progress = None;
                    self.logs.push(format!(
                        "Исходные данные новой загрузки: {}",
                        receipt.upload_record.display()
                    ));
                    self.message(if receipt.frame_count > 1 {
                        format!(
                            "Анимация, {} кадров: {}",
                            receipt.frame_count, receipt.verification
                        )
                    } else {
                        format!("Слот {}: {}", receipt.slot + 1, receipt.verification)
                    });
                }
                Event::DockUpload(Err(error)) => {
                    self.dock_progress = None;
                    self.message(format!("Загрузка экрана остановлена: {error}"));
                }
                Event::DockInfo(Ok(info)) => {
                    self.message(format!(
                        "Версия дока {} прочитана напрямую через HID",
                        info.label()
                    ));
                    self.dock_info = Some(info);
                }
                Event::DockInfo(Err(e)) => self.message(format!("Чтение дока остановлено: {e}")),
                Event::MouseFirmwareInfo(result) => match result {
                    Ok(info) => {
                        self.message(if info.mouse_version.is_some() {
                            "ID и версии комплекта прочитаны. Это сведения о версиях, не дамп прошивки."
                        } else {
                            "Версия OLED прочитана; мышь не дала полного ответа. Подробности в диагностике."
                        });
                        self.mouse_firmware_info = Some(info);
                    }
                    Err(error) => {
                        self.mouse_firmware_info = None;
                        self.message(format!("Чтение версий остановлено: {error}"));
                    }
                },
                Event::Snapshot(Ok(snapshot)) => {
                    self.message(format!(
                        "Настройки прочитаны · прошивка {}",
                        snapshot.info.version
                    ));
                    self.accept_keyboard_snapshot(snapshot);
                }
                Event::Snapshot(Err(e)) => self.message(format!("Чтение остановлено: {e}")),
                Event::KeymapWrite(Ok(receipt)) => {
                    self.message(if receipt.changed {
                        "Назначения клавиш записаны и проверены"
                    } else {
                        "Назначения клавиш уже совпадают"
                    });
                    self.accept_keyboard_snapshot(receipt.snapshot);
                    if let Some(path) = receipt.backup_path {
                        self.logs
                            .push(format!("Копия назначений: {}", path.display()));
                        self.keymap_backup = Some(path);
                    }
                }
                Event::KeymapWrite(Err(e)) => {
                    self.message(format!("Запись назначений остановлена: {e}"))
                }
                Event::DksWrite(result) => match result {
                    Ok(receipt) => {
                        let changed = receipt.changed;
                        self.accept_keyboard_snapshot(receipt.snapshot);
                        if let Some(path) = receipt.backup_path {
                            self.dks_backup = Some(path);
                        }
                        self.message(if changed {
                            "Таблица DKS и назначения записаны и проверены"
                        } else {
                            "Таблица DKS и назначения уже совпадают"
                        });
                    }
                    Err(error) => self.message(format!("Запись DKS остановлена: {error}")),
                },
                Event::MacrosRead(result, preserve_draft) => match result {
                    Ok(snapshot) => {
                        self.accept_keyboard_snapshot(snapshot.configuration.clone());
                        if !preserve_draft {
                            self.macro_draft = snapshot.macros.clone();
                        }
                        self.macro_snapshot = Some(snapshot);
                        self.message("Макросы и назначения прочитаны");
                    }
                    Err(error) => self.message(format!("Чтение макросов остановлено: {error}")),
                },
                Event::MacrosWrite(result) => match result {
                    Ok(receipt) => {
                        self.accept_keyboard_snapshot(receipt.snapshot.configuration.clone());
                        self.macro_draft = receipt.snapshot.macros.clone();
                        self.macro_snapshot = Some(receipt.snapshot);
                        if let Some(path) = receipt.backup_path {
                            self.macro_backup = Some(path);
                        }
                        self.message(if receipt.changed {
                            "Макросы и назначения записаны и проверены"
                        } else {
                            "Макросы и назначения уже совпадают"
                        });
                    }
                    Err(error) => self.message(format!("Запись макросов остановлена: {error}")),
                },
                Event::KeyboardWrite(Ok(receipt)) => {
                    self.message(if receipt.changed {
                        "Магнитные настройки записаны и проверены"
                    } else {
                        "Магнитные настройки уже совпадают"
                    });
                    self.accept_keyboard_snapshot(receipt.snapshot);
                    if let Some(path) = receipt.backup_path {
                        self.logs
                            .push(format!("Копия клавиатуры: {}", path.display()));
                        self.hall_backup = Some(path);
                    }
                }
                Event::KeyboardWrite(Err(e)) => {
                    self.message(format!("Запись клавиатуры остановлена: {e}"))
                }
                Event::GeneralWrite(Ok(receipt)) => {
                    self.message(if receipt.changed {
                        "Общие настройки клавиатуры записаны и проверены"
                    } else {
                        "Общие настройки уже совпадают"
                    });
                    self.accept_keyboard_snapshot(receipt.snapshot);
                    if let Some(path) = receipt.backup_path {
                        self.logs
                            .push(format!("Копия общих настроек: {}", path.display()));
                        self.general_backup = Some(path);
                    }
                }
                Event::GeneralWrite(Err(e)) => {
                    self.message(format!("Запись общих настроек остановлена: {e}"))
                }
                Event::Lighting(Ok(receipt)) => {
                    self.message(if receipt.effect_color_verified {
                        "Подсветка применена · параметры проверены · копия сохранена"
                    } else {
                        "Цвет принят клавиатурой · режим, яркость и скорость проверены. Прошивка не возвращает выбранный цвет."
                    });
                    self.logs
                        .push(format!("Копия: {}", receipt.backup_path.display()));
                    if let Some(snapshot) = &mut self.snapshot {
                        if snapshot.info.profile == receipt.info.profile
                            && snapshot.info.work_mode == receipt.info.work_mode
                        {
                            snapshot.info = receipt.info.clone();
                            snapshot
                                .blocks
                                .insert("lighting".into(), receipt.state.lighting.clone());
                            if receipt.rgb_verified {
                                snapshot
                                    .blocks
                                    .insert("custom_rgb".into(), receipt.state.custom_rgb.clone());
                            }
                        } else {
                            self.snapshot = None;
                        }
                    }
                    self.macro_snapshot = None;
                    self.keyboard_info = Some(receipt.info);
                    self.lighting_backup = Some(receipt.backup_path);
                    self.keyboard_effect_command = receipt.requested_effect.clone();
                    self.keyboard_lighting = KeyboardLighting::parse(
                        receipt
                            .requested_effect
                            .as_ref()
                            .unwrap_or(&receipt.state.lighting),
                    )
                    .ok()
                    .map(|v| (receipt.state.lighting, v));
                }
                Event::Lighting(Err(e)) => self.message(format!("Запись остановлена: {e}")),
                Event::Mouse(Ok(snapshot)) => {
                    self.message(format!(
                        "AJ159 APEX прочитана · {} Гц · debounce {} мс",
                        snapshot.polling_hz, snapshot.options.debounce_ms
                    ));
                    self.mouse_settings = Some(MouseSettings::from(&snapshot));
                    self.mouse_snapshot = Some(snapshot);
                }
                Event::Mouse(Err(e)) => self.message(format!("Чтение остановлено: {e}")),
                Event::MouseWrite(Ok(receipt)) => {
                    self.message(if receipt.changed {
                        "Параметры мыши записаны и проверены чтением"
                    } else {
                        "Параметры мыши уже совпадают; запись не потребовалась"
                    });
                    if let Some(path) = receipt.backup_path {
                        self.logs.push(format!("Копия мыши: {}", path.display()));
                        self.mouse_backup = Some(path);
                    }
                    self.mouse_settings = Some(MouseSettings::from(&receipt.snapshot));
                    self.mouse_snapshot = Some(receipt.snapshot);
                }
                Event::MouseWrite(Err(e)) => self.message(format!("Запись мыши остановлена: {e}")),
                Event::ButtonsRead(Ok(snapshot)) => {
                    self.buttons_action = snapshot
                        .action(self.buttons_slot)
                        .unwrap_or(MouseAction::Disabled);
                    self.buttons_snapshot = Some(snapshot);
                    self.message("Назначения кнопок прочитаны");
                }
                Event::ButtonsRead(Err(e)) => {
                    self.message(format!("Чтение кнопок остановлено: {e}"))
                }
                Event::ButtonsWrite(Ok(receipt)) => {
                    self.message(if receipt.changed {
                        "Назначения записаны и проверены чтением"
                    } else {
                        "Назначения уже совпадают; запись не потребовалась"
                    });
                    if let Some(path) = receipt.backup_path {
                        self.logs.push(format!("Копия кнопок: {}", path.display()));
                        self.buttons_backup = Some(path);
                    }
                    self.buttons_action = receipt
                        .snapshot
                        .action(self.buttons_slot)
                        .unwrap_or(MouseAction::Disabled);
                    self.buttons_snapshot = Some(receipt.snapshot);
                }
                Event::ButtonsWrite(Err(e)) => {
                    self.message(format!("Запись кнопок остановлена: {e}"))
                }
                Event::Done(Ok(msg)) => self.message(msg),
                Event::Done(Err(e)) => self.message(format!("Ошибка: {e}")),
            }
        }
    }
    fn read_snapshot(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.lighting_pending = None;
        self.message("Читаю настройки клавиатуры…");
        let tx = self.tx.clone();
        let wired = self.playback_channel == PlaybackChannel::Usb;
        std::thread::spawn(move || {
            let result = hardware::keyboard_snapshot_on(wired).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Snapshot(result));
        });
    }
    fn accept_keyboard_snapshot(&mut self, snapshot: Snapshot) {
        self.keyboard_effect_command = None;
        self.macro_snapshot = None;
        self.keyboard_info = Some(snapshot.info.clone());
        self.general_settings = snapshot
            .blocks
            .get("game_mode")
            .and_then(|b| GeneralSettings::parse(b).ok());
        self.keyboard_lighting = snapshot
            .blocks
            .get("lighting")
            .and_then(|b| KeyboardLighting::parse(b).ok().map(|v| (b.clone(), v)));
        self.snapshot = Some(snapshot);
        self.hall_selection = None;
        self.keymap_selection = None;
    }
    fn apply_general(&mut self) {
        if self.busy {
            return;
        }
        let (Some(expected), Some(settings)) =
            (self.snapshot.clone(), self.general_settings.clone())
        else {
            return;
        };
        self.busy = true;
        self.message("Сохраняю копию и применяю общие настройки клавиатуры…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::GeneralWrite(
                keyboard_settings::apply_general(&expected, &settings)
                    .map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_general(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю общие настройки клавиатуры…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::GeneralWrite(
                keyboard_settings::restore_general(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn layers(&mut self) -> &mut Vec<Layer> {
        if self.dock_surface {
            &mut self.scene.dock_layers
        } else {
            &mut self.scene.layers
        }
    }
    fn apply_builtin_lighting(&mut self) {
        if self.busy {
            return;
        }
        let Some((expected, settings)) = self.keyboard_lighting.clone() else {
            return;
        };
        self.lighting_pending = None;
        self.busy = true;
        self.message("Сохраняю копию и применяю встроенный режим подсветки…");
        let tx = self.tx.clone();
        let wired = self.playback_channel == PlaybackChannel::Usb;
        let profile = self.keyboard_info.as_ref().map(|info| info.profile);
        let cached = self.snapshot.clone();
        let previous_command = self.keyboard_effect_command.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<LightingReceipt> {
                let baseline = if let Some(snapshot) = cached {
                    snapshot
                } else {
                    hardware::keyboard_snapshot_on(wired)?
                };
                anyhow::ensure!(
                    profile == Some(baseline.info.profile),
                    "Профиль изменился. Прочитайте режим заново."
                );
                anyhow::ensure!(
                    baseline.blocks.get("lighting").is_some_and(|raw| {
                        ajazz_studio::protocol::lighting_equivalent(raw, &expected)
                    }),
                    "Подсветка изменилась. Прочитайте режим заново."
                );
                hardware::apply_keyboard_effect_with_previous(
                    &baseline,
                    &settings,
                    previous_command.as_deref(),
                )
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Lighting(result));
        });
    }
    fn read_mouse(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Читаю параметры мыши напрямую через HID…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = ajazz_studio::mouse_hid::read_snapshot().map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Mouse(result));
        });
    }
    fn restore_lighting(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        let tx = self.tx.clone();
        self.busy = true;
        self.message("Восстанавливаю подсветку из резервной копии…");
        let previous_command = self.keyboard_effect_command.clone();
        std::thread::spawn(move || {
            let result = hardware::restore_keyboard_lighting_with_previous(
                &path,
                previous_command.as_deref(),
            )
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Lighting(result));
        });
    }
    fn apply_mouse(&mut self) {
        if self.busy {
            return;
        }
        let (Some(expected), Some(settings)) =
            (self.mouse_snapshot.clone(), self.mouse_settings.clone())
        else {
            return;
        };
        self.busy = true;
        self.message("Сохраняю копию и применяю параметры мыши…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = mouse::apply_settings(&expected, &settings).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::MouseWrite(result));
        });
    }
    fn apply_hall(&mut self) {
        if self.busy {
            return;
        }
        let Some(expected) = self.snapshot.clone() else {
            return;
        };
        let edit = HallEdit {
            keys: self.selected.clone(),
            settings: self.hall.clone(),
        };
        self.busy = true;
        self.message("Сохраняю копию и записываю магнитные настройки выбранных клавиш…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::KeyboardWrite(
                keyboard_settings::apply(&expected, &edit).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn apply_keymap(&mut self) {
        if self.busy {
            return;
        }
        let Some(expected) = self.snapshot.clone() else {
            return;
        };
        let edit = KeymapEdit {
            keys: self.selected.clone(),
            assignment: self.keymap_assignment.clone(),
        };
        self.busy = true;
        self.message("Сохраняю копию и записываю назначения выбранных клавиш…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::KeymapWrite(
                keyboard_settings::apply_keymap(&expected, &edit).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn apply_dks(&mut self) {
        if self.busy || self.selected.len() != 1 {
            return;
        }
        let (Some(expected), Some(settings), Some(key)) = (
            self.snapshot.clone(),
            self.dks_config.clone(),
            self.selected.first().copied(),
        ) else {
            return;
        };
        self.busy = true;
        self.message("Сохраняю копию DKS и назначений…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::DksWrite(
                keyboard_settings::apply_dks(&expected, key, &settings)
                    .map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_dks(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю таблицу DKS и назначения…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::DksWrite(
                keyboard_settings::restore_dks(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_keymap(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю назначения клавиш…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::KeymapWrite(
                keyboard_settings::restore_keymap(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_hall(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю магнитные настройки…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::KeyboardWrite(
                keyboard_settings::restore(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_mouse(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю параметры мыши…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::MouseWrite(
                mouse::restore(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn save_scene(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AJAZZ scene", &["json"])
            .set_file_name("scene.ajazz.json")
            .save_file()
        {
            match self.scene.save(&path) {
                Ok(()) => self.message(format!("Сцена сохранена: {}", path.display())),
                Err(e) => self.message(e.to_string()),
            }
        }
    }
    fn open_scene(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AJAZZ scene", &["json"])
            .pick_file()
        {
            match Scene::load(&path) {
                Ok(scene) => {
                    if let Some(slot) = self.memory_editor_slot {
                        self.memory_drafts.insert(slot, self.scene.clone());
                    }
                    self.memory_editor_slot = None;
                    self.scene = scene;
                    self.session_autosave = true;
                    self.layer = 0;
                    self.time = 0.0;
                    self.pulses.clear();
                    self.message("Сцена открыта");
                }
                Err(e) => self.message(e.to_string()),
            }
        }
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        if matches!(self.page, Page::Research | Page::Devices) {
            if ui
                .button(if self.navigation.is_empty() {
                    "‹  Настройки"
                } else {
                    "‹  Назад"
                })
                .clicked()
            {
                self.page = self.navigation.pop().unwrap_or(Page::Preferences);
            }
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.page.title()).size(26.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if matches!(
                    self.page,
                    Page::Lighting | Page::Preferences | Page::Devices | Page::Research
                ) {
                    return;
                }
                if self.page == Page::Dock && self.dock_tab == DockTab::Pages {
                    self.dock_editor_actions(ui);
                    return;
                }
                ui.menu_button("•••", |ui| {
                    if self.page == Page::Lighting {
                        if ui.button("Импортировать эффект…").clicked() {
                            self.open_scene();
                            ui.close();
                        }
                        if ui.button("Экспортировать эффект…").clicked() {
                            self.save_scene();
                            ui.close();
                        }
                        ui.separator();
                    }
                    if !matches!(self.page, Page::Devices | Page::Research)
                        && ui
                            .add_enabled(
                                !self.busy && self.playback_cancel.is_none(),
                                egui::Button::new("Обновить с устройства"),
                            )
                            .clicked()
                    {
                        self.refresh_visible_data();
                        ui.close();
                    }
                    if self.page == Page::Keyboard
                        && self.snapshot.is_some()
                        && ui.button("Экспортировать копию настроек…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_file_name("ak820max-backup.json")
                            .save_file()
                    {
                        let result = self.snapshot.as_ref().unwrap().save(&path);
                        self.message(
                            result
                                .map(|()| "Копия сохранена".into())
                                .unwrap_or_else(|e| e.to_string()),
                        );
                        ui.close();
                    }
                    if self.page == Page::Mouse && !self.mouse_buttons_tab {
                        self.mouse_editor_actions(ui);
                    }
                });
            });
        });
        ui.add_space(14.0);
    }
    fn reset_keyboard_connection(&mut self) {
        self.memory_generation = self.memory_generation.wrapping_add(1);
        self.memory_clock = None;
        self.memory_clock_failed = false;
        self.auto_reads.clear();
        self.memory_state = None;
        self.memory_error = None;
        self.lighting_pending = None;
        self.snapshot = None;
        self.macro_snapshot = None;
        self.keyboard_info = None;
        self.keyboard_lighting = None;
        self.keyboard_effect_command = None;
        self.general_settings = None;
        self.hall_selection = None;
        self.keymap_selection = None;
        self.playback_interval = self
            .playback_interval
            .max(self.playback_channel.minimum_interval());
        self.message(format!(
            "Подключение {} определено автоматически.",
            self.playback_channel.label()
        ));
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        if !ctx.wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft))
            && let Some(page) = self.navigation.pop()
        {
            self.page = page;
        }
        self.modern_sidebar(ctx);
    }
    fn keyboard_canvas(&mut self, ui: &mut egui::Ui) {
        let scene = self.scene.clone();
        self.keyboard_canvas_for(ui, &scene, self.time, true);
    }
    fn keyboard_canvas_for(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        time: f32,
        selectable: bool,
    ) {
        let rows = keyboard_rows();
        let leds = lighting::keyboard_leds();
        let mut colors = if self.memory_preview && self.page == Page::Lighting
            || self.page == Page::Keyboard && self.keyboard_tab == KeyboardTab::Encoder
        {
            ajazz_studio::keyboard_memory::device_preview(
                scene,
                time,
                &self.pulses,
                self.memory_config.brightness,
                self.memory_config.speed,
                self.memory_clock
                    .as_ref()
                    .is_some_and(|(rt, _)| rt.color_transfer == 1),
            )
            .unwrap_or_else(|_| {
                lighting::render(&scene.layers, &leds, time, scene.brightness, &self.pulses)
            })
        } else {
            lighting::render(&scene.layers, &leds, time, scene.brightness, &self.pulses)
        };
        if self.page == Page::Keyboard && self.keyboard_tab == KeyboardTab::Encoder {
            ajazz_studio::keyboard_memory::battery_preview(
                &mut colors,
                self.battery_preview_percent,
                &self.memory_config.battery,
            );
        }
        let neutral = self.page == Page::Keyboard
            && matches!(
                self.keyboard_tab,
                KeyboardTab::Hall | KeyboardTab::Keymap | KeyboardTab::Macros
            );
        if neutral {
            colors.fill([30, 35, 44]);
        }
        let width = ui.available_width().min(if self.page == Page::Lighting {
            860.0
        } else {
            1000.0
        });
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, width * 0.41), Sense::hover());
        let geometry = keyboard_art::geometry(rect);
        let unit = geometry.unit;
        keyboard_art::case(ui.painter(), &geometry);
        let mut index = 0;
        for keys in &rows {
            for key in keys {
                let r = geometry.keys[index];
                let color = colors[index];
                let chosen = self.selected.contains(&key.id)
                    && !(self.page == Page::Keyboard && self.keyboard_tab == KeyboardTab::Encoder);
                let c = if neutral && chosen {
                    Color32::from_rgb(35, 65, 106)
                } else {
                    Color32::from_rgb(color[0], color[1], color[2])
                };
                let luminance: f32 = color
                    .iter()
                    .zip([0.2126, 0.7152, 0.0722])
                    .map(|(&channel, weight)| {
                        let value = f32::from(channel) / 255.0;
                        weight
                            * if value <= 0.04045 {
                                value / 12.92
                            } else {
                                ((value + 0.055) / 1.055).powf(2.4)
                            }
                    })
                    .sum();
                let text_color = if luminance > 0.179 {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                ui.painter().rect_filled(r, 4.0, c);
                ui.painter().rect_stroke(
                    r,
                    4.0,
                    Stroke::new(
                        if chosen { 2.0_f32 } else { 1.0_f32 },
                        if chosen {
                            if neutral { TEAL } else { text_color }
                        } else {
                            c.gamma_multiply(0.65)
                        },
                    ),
                    StrokeKind::Inside,
                );
                let label = match key.physical.as_str() {
                    "ControlLeft" | "ControlRight" => "Ctrl",
                    "ShiftLeft" | "ShiftRight" => "Shift",
                    "AltLeft" | "AltRight" => "Alt",
                    "MetaLeft" => "Win",
                    "ContextMenu" => "Menu",
                    "Space" => "",
                    _ => &key.label,
                };
                let base_size = (unit * 0.32).clamp(9.0, 15.0);
                let measured = ui.painter().layout_no_wrap(
                    label.into(),
                    FontId::proportional(base_size),
                    text_color,
                );
                let label_size =
                    base_size * ((r.width() - 4.0) / measured.size().x.max(1.0)).min(1.0);
                ui.painter()
                    .with_clip_rect(r.intersect(ui.clip_rect()))
                    .text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(label_size),
                        text_color,
                    );
                let response = ui.interact(
                    r,
                    ui.id().with(("key", key.id)),
                    if selectable {
                        Sense::click_and_drag()
                    } else {
                        Sense::hover()
                    },
                );
                if response.clicked() {
                    if !self.selected.insert(key.id) {
                        self.selected.remove(&key.id);
                    }
                    let led = leds[index];
                    self.pulses.push(Pulse {
                        x: led.x,
                        y: led.y,
                        time,
                    });
                }
                if response.dragged() {
                    self.selected.insert(key.id);
                }
                response.on_hover_text(format!(
                    "{} · ID {}\nНажмите или протяните для выделения",
                    key.label, key.id
                ));
                index += 1;
            }
        }
    }
    fn groups(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Выбрано: {}", self.selected.len())).color(MUTED));
            for (name, physical) in [
                ("WASD", vec!["KeyW", "KeyA", "KeyS", "KeyD"]),
                (
                    "Стрелки",
                    vec!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"],
                ),
                (
                    "F-ряд",
                    vec![
                        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
                    ],
                ),
            ] {
                if ui.small_button(name).clicked() {
                    self.selected = keyboard_rows()
                        .into_iter()
                        .flatten()
                        .filter(|k| physical.contains(&k.physical.as_str()))
                        .map(|k| k.id)
                        .collect();
                }
            }
            if ui.small_button("Все").clicked() {
                self.selected = keyboard_rows()
                    .into_iter()
                    .flatten()
                    .map(|k| k.id)
                    .collect();
            }
            if ui.small_button("Снять").clicked() {
                self.selected.clear();
            }
        });
    }
    fn dock_ring(&self, ui: &mut egui::Ui) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 250.0), Sense::hover());
        let center = rect.center();
        let leds = lighting::dock_leds(16);
        let colors = lighting::render(
            &self.scene.dock_layers,
            &leds,
            self.time,
            self.scene.brightness,
            &[],
        );
        ui.painter()
            .circle_filled(center, 95.0, Color32::from_rgb(10, 15, 23));
        ui.painter().rect_filled(
            Rect::from_center_size(center, Vec2::splat(105.0)),
            10.0,
            PANEL,
        );
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            "APEX",
            FontId::proportional(22.0),
            Color32::WHITE,
        );
        for (led, c) in leds.iter().zip(colors) {
            let p = center + Vec2::new((led.x - 0.5) * 186.0, (led.y - 0.5) * 186.0);
            ui.painter()
                .circle_filled(p, 6.0, Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    fn lighting_page(&mut self, ui: &mut egui::Ui) {
        if !self.dock_surface {
            self.keyboard_memory_panel(ui);
        }
        ui.add_enabled_ui(!self.busy || self.memory_publishing.is_some(), |ui| {
            self.lighting_editor(ui)
        });
        if !self.dock_surface {
            self.collection_gallery(ui);
        }
        if !self.dock_surface {
            ui.add_space(12.0);
            egui::CollapsingHeader::new("Возможности устройства и диагностика")
                .open(self.playback_cancel.is_some().then_some(true))
                .show(ui, |ui| self.lighting_device_diagnostics(ui));
        }
    }
    fn lighting_device_diagnostics(&mut self, ui: &mut egui::Ui) {
        if !self.dock_surface {
            ui.label(
                RichText::new("ПОДСВЕТКА В ПАМЯТИ КЛАВИАТУРЫ")
                    .size(11.0)
                    .color(TEAL),
            );
            ui.label("Встроенные эффекты работают автономно. Свои цвета клавиш можно сохранить одним неподвижным кадром.");
            if self.memory_state.is_some() {
                ui.label("Автономный движок подключён: 8 режимов, до 8 слоёв. Кадры с компьютера не передаются.");
            } else if let Some(info) = &self.keyboard_info {
                match &info.animation_limits {
                    Some(limits) if limits.gif_frames == 0 => {
                        ui.label(RichText::new(format!("Прошивка {} сообщает: пользовательская RGB-анимация — 0 кадров. Загрузка своей последовательности в память для этой ревизии не подтверждена.", info.version)).color(AMBER));
                    }
                    Some(limits) => {
                        ui.label(format!("Прошивка сообщает {} кадров RGB-анимации. Формат загрузки для этой ревизии ещё не проверен.", limits.gif_frames));
                    }
                    None => {
                        ui.label("Возможности памяти ещё не прочитаны. Загрузка своей анимации в устройство пока не реализована.");
                    }
                }
            } else {
                ui.label("Загрузка своей анимации в устройство пока не реализована. Прочитайте заявленные прошивкой возможности памяти.");
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.label(format!("Подключение: {}", self.playback_channel.label()));
                    if ui.button("Прочитать возможности памяти").clicked()
                    {
                        self.busy = true;
                        self.message("Читаю сведения о памяти анимаций…");
                        let tx = self.tx.clone();
                        let wired = self.playback_channel == PlaybackChannel::Usb;
                        std::thread::spawn(move || {
                            let result =
                                hardware::keyboard_info(wired).map_err(|e| format!("{e:#}"));
                            let _ = tx.send(Event::KeyboardInfo(result));
                        });
                    }
                });
            });
            egui::CollapsingHeader::new("Диагностика · передача кадров с компьютера")
                .open(self.playback_cancel.is_some().then_some(true))
                .show(ui, |ui| self.host_playback_controls(ui));
            ui.separator();
        }
    }
    fn host_playback_controls(&mut self, ui: &mut egui::Ui) {
        if !self.dock_surface {
            ui.label(
                RichText::new("ЭКСПЕРИМЕНТАЛЬНЫЙ ВЫВОД С КОМПЬЮТЕРА")
                    .size(11.0)
                    .color(TEAL),
            );
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(self.playback_cancel.is_none(), |ui| {
                    ui.label(format!("Подключение: {}", self.playback_channel.label()));
                    self.playback_interval = self
                        .playback_interval
                        .max(self.playback_channel.minimum_interval());
                    egui::ComboBox::from_id_salt("playback_interval")
                        .selected_text(format!("{} мс", self.playback_interval))
                        .show_ui(ui, |ui| {
                            for interval in [100, 125, 200, 250, 500, 1000, 2000, 5000] {
                                if interval >= self.playback_channel.minimum_interval() {
                                    ui.selectable_value(
                                        &mut self.playback_interval,
                                        interval,
                                        format!("{interval} мс"),
                                    );
                                }
                            }
                        });
                    ui.checkbox(&mut self.playback_repeat, "Повторять");
                });
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new("Начать диагностический вывод"),
                    )
                    .clicked()
                {
                    self.start_keyboard_animation();
                }
                if ui
                    .add_enabled(
                        self.playback_cancel.is_some(),
                        egui::Button::new("Остановить и вернуть подсветку"),
                    )
                    .clicked()
                {
                    if let Some(cancel) = &self.playback_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    self.message("Останавливаю анимацию и возвращаю исходную подсветку…");
                }
            });
            if let Some(p) = self.playback_progress {
                ui.label(format!(
                    "Кадров отправлено: {} · время сцены {:.2} с · передача {} мс · с проверкой {} мс",
                    p.frame, p.scene_time, p.write_ms, p.transfer_ms
                ));
            }
            ui.label("Передача идёт по частям и может быть заметно прерывистой. Работает, пока приложение открыто; остановка возвращает исходную подсветку.");
            ui.small("При запуске берётся копия сцены. Редактор и его предпросмотр работают независимо; новые правки передаются после следующего запуска. Запись только в RAM и ресурс сохранений этой прошивки пока не установлены.");
            ui.separator();
        }
    }
    fn start_keyboard_animation(&mut self) {
        self.start_animation_scene(self.scene.clone());
    }
    fn start_animation_scene(&mut self, scene: Scene) {
        if self.busy || self.playback_cancel.is_some() {
            return;
        }
        self.lighting_pending = None;
        let channel = self.playback_channel;
        let previous = self.keyboard_effect_command.clone();
        let interval = self.playback_interval;
        let repeat = self.playback_repeat;
        let cancel = Arc::new(AtomicBool::new(false));
        self.playback_cancel = Some(cancel.clone());
        self.playback_progress = None;
        self.busy = true;
        self.message("Сохраняю исходную подсветку и запускаю анимацию…");
        let tx = self.tx.clone();
        let hidden = self.window_hidden.clone();
        std::thread::spawn(move || {
            let result = keyboard_playback::play_with_previous(
                &scene,
                channel,
                interval,
                repeat,
                &cancel,
                |p| {
                    if !hidden.load(Ordering::Relaxed) {
                        let _ = tx.send(Event::PlaybackProgress(p));
                    }
                },
                previous.as_deref(),
            )
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::PlaybackFinished(result));
        });
    }
    fn lighting_editor(&mut self, ui: &mut egui::Ui) {
        if self.dock_surface {
            ui.horizontal(|ui| {
                ui.label("Эскиз кольца дока");
                if ui.button("Вернуться к клавиатуре").clicked() {
                    self.dock_surface = false;
                    self.layer = 0;
                }
            });
        }
        if self.dock_surface || self.memory_preview {
            self.lighting_scene_editor(ui);
        } else {
            self.lighting_hardware_controls(ui);
        }
    }
    fn lighting_hardware_controls(&mut self, ui: &mut egui::Ui) {
        if !self.dock_surface {
            ui.separator();
            egui::CollapsingHeader::new("Подсветка клавиатуры")
                .id_salt("keyboard_lighting_panel")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("Встроенные эффекты работают автономно. Для своих анимаций выберите «Сохранённые цвета клавиш».");
                    if self.keyboard_lighting.is_none() && ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new("Повторить подключение"),
                        )
                        .clicked()
                    {
                        self.read_snapshot();
                    }
                    if ui.add_enabled(!self.busy && self.keyboard_lighting.is_some(), egui::Button::new("Своя анимация…")).clicked() {
                        if let Some((_, settings)) = &mut self.keyboard_lighting { settings.mode = 20; }
                        self.lighting_pending = None;
                    }
                    let mut apply = false;
                    if let Some((raw, settings)) = &mut self.keyboard_lighting {
                        let previous = settings.clone();
                        ui.add_enabled_ui(!self.busy, |ui| {
                            ui.small(RichText::new("Изменения применяются автоматически").color(MUTED));
                            egui::ComboBox::from_id_salt("keyboard_builtin_mode")
                                .selected_text(
                                    BUILTIN_LIGHTING
                                        .iter()
                                        .find(|(v, _)| *v == settings.mode)
                                        .map(|(_, label)| *label)
                                        .unwrap_or("Неизвестный режим"),
                                )
                                .show_ui(ui, |ui| {
                                    for &(code, label) in BUILTIN_LIGHTING {
                                        ui.selectable_value(&mut settings.mode, code, label);
                                    }
                                });
                            if settings.mode != 20 {
                            ui.add_enabled_ui(![0, 6, 8, 20].contains(&settings.mode), |ui| {
                                ui.checkbox(&mut settings.rainbow, "Радужная палитра");
                                ui.horizontal(|ui| {
                                    ui.label("Цвет");
                                    if ui.color_edit_button_srgb(&mut settings.color).changed() {
                                        settings.rainbow = false;
                                    }
                                });
                                ui.horizontal(|ui| {
                                    for (label, color) in [("Красный",[255,0,0]), ("Зелёный",[0,255,0]), ("Синий",[0,0,255]), ("Белый",[255,255,255])] {
                                        if ui.small_button(label).clicked() {
                                            settings.color = color;
                                            settings.rainbow = false;
                                        }
                                    }
                                });
                            });
                            ui.label(RichText::new("1.30 возвращает белый/RGB вместо текущего цвета. Для возврата сохраняется предыдущая команда этой сессии; после нового чтения исходный цвет неизвестен.").size(12.0).color(MUTED));
                            ui.add(
                                egui::Slider::new(&mut settings.brightness, 1..=5)
                                    .text("Яркость устройства"),
                            );
                            ui.add_enabled(
                                ![0, 1, 20].contains(&settings.mode),
                                egui::Slider::new(&mut settings.speed, 1..=5).text("Скорость"),
                            );
                            ui.add_enabled_ui(
                                [10, 11, 12, 16, 18].contains(&settings.mode),
                                |ui| {
                                    egui::ComboBox::from_id_salt("keyboard_builtin_direction")
                                        .selected_text(
                                            ["Вправо", "Влево", "Вниз", "Вверх"]
                                                .get(usize::from(settings.direction))
                                                .copied()
                                                .unwrap_or("Неизвестно"),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (i, label) in ["Вправо", "Влево", "Вниз", "Вверх"]
                                                .iter()
                                                .enumerate()
                                            {
                                                ui.selectable_value(
                                                    &mut settings.direction,
                                                    i as u8,
                                                    *label,
                                                );
                                            }
                                        });
                                },
                            );
                            }
                            if let Err(error)=settings.patch(raw) {ui.colored_label(AMBER,error.to_string());}
                            if settings.mode==20 {
                                apply=ui.button("Показать сохранённые цвета без анимации").clicked();
                            }
                        });
                        if self.lighting_auto_apply && *settings != previous {
                            self.lighting_pending = Some(Instant::now());
                        }
                    }
                    if apply {
                        self.apply_builtin_lighting();
                    }
                    if self.playback_cancel.is_some() || self.keyboard_lighting.as_ref().is_some_and(|(_, settings)| settings.mode == 20) {
                        self.saved_colors_animation_controls(ui);
                    }

            ui.add_space(12.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.busy && self.lighting_backup.is_some(),
                        egui::Button::new("Вернуть предыдущую подсветку"),
                    )
                    .clicked()
                    && let Some(path) = self.lighting_backup.clone()
                {
                    self.restore_lighting(path);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Восстановить из файла…"))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Lighting backup", &["json"])
                        .pick_file()
                {
                    self.restore_lighting(path);
                }
            });
            if let Some(path) = &self.lighting_backup {
                ui.label(
                    RichText::new(format!("Последняя копия: {}", path.display()))
                        .size(11.0)
                        .color(MUTED),
                );
            }
                });
        }
    }
    fn saved_colors_animation_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(
            RichText::new("Свои анимации · из приложения")
                .color(TEAL)
                .size(19.0),
        );
        ui.label("Кадры отправляются, пока AJAZZ Studio открыт. «Стоп» возвращает исходные цвета и режим; закрытие окна сначала останавливает анимацию.");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && self.keyboard_animation.validate().is_ok(),
                    egui::Button::new("Запустить анимацию"),
                )
                .clicked()
            {
                self.start_animation_scene(self.keyboard_animation.scene());
            }
            if ui
                .add_enabled(
                    self.playback_cancel.is_some(),
                    egui::Button::new("Стоп и возврат подсветки"),
                )
                .clicked()
                && let Some(cancel) = &self.playback_cancel
            {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        if let Some(p) = self.playback_progress {
            ui.label(format!(
                "Отправлено {} кадров · запись {} мс · обработка {} мс · фактически {:.1} кадр/с{}",
                p.frame,
                p.write_ms,
                p.transfer_ms,
                p.actual_fps,
                if p.verified {
                    " · таблица проверена"
                } else {
                    " · ACK получены"
                }
            ));
        }
        ui.add_enabled_ui(!self.busy, |ui| {
            let previous = self.keyboard_animation.preset;
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("saved_colors_preset")
                    .selected_text(self.keyboard_animation.preset.label())
                    .show_ui(ui, |ui| {
                        for preset in AnimationPreset::ALL {
                            ui.selectable_value(
                                &mut self.keyboard_animation.preset,
                                preset,
                                preset.label(),
                            );
                        }
                    });
                if previous != self.keyboard_animation.preset {
                    self.keyboard_animation.colors = self.keyboard_animation.preset.colors();
                }
                ui.label("Палитра");
                for color in &mut self.keyboard_animation.colors {
                    ui.color_edit_button_srgb(color);
                }
                if ui.small_button("Сбросить палитру").clicked() {
                    self.keyboard_animation.colors = self.keyboard_animation.preset.colors();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.keyboard_animation.speed, 0.02..=2.0)
                        .logarithmic(true)
                        .text("Скорость"),
                );
                ui.add(
                    egui::Slider::new(&mut self.keyboard_animation.brightness, 0.0..=1.0)
                        .text("Яркость анимации"),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.keyboard_animation.scale, 0.25..=4.0)
                        .text("Масштаб"),
                );
                ui.add(
                    egui::Slider::new(&mut self.keyboard_animation.angle, 0.0..=180.0).text("Угол"),
                );
                ui.checkbox(&mut self.keyboard_animation.reverse, "Развернуть рисунок");
            });
            ui.horizontal_wrapped(|ui| {
                self.playback_interval = self
                    .playback_interval
                    .max(self.playback_channel.minimum_interval());
                egui::ComboBox::from_id_salt("saved_colors_interval")
                    .selected_text(format!("Цель: кадр каждые {} мс", self.playback_interval))
                    .show_ui(ui, |ui| {
                        for interval in [16, 33, 50, 100, 125, 200, 250, 500, 1000, 2000] {
                            if interval >= self.playback_channel.minimum_interval() {
                                ui.selectable_value(
                                    &mut self.playback_interval,
                                    interval,
                                    format!("{interval} мс"),
                                );
                            }
                        }
                    });
                ui.add(
                    egui::Slider::new(&mut self.keyboard_animation.duration, 5.0..=120.0)
                        .text("Длительность, с"),
                );
                ui.checkbox(&mut self.playback_repeat, "Повторять");
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Сохранить пресет…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("keyboard-animation.json")
                        .save_file()
                {
                    let result = self.keyboard_animation.validate().and_then(|()| {
                        ajazz_studio::storage::write_json_atomic(&path, &self.keyboard_animation)
                    });
                    if let Err(error) = result {
                        self.message(format!("Ошибка сохранения пресета: {error:#}"));
                    }
                }
                if ui.button("Открыть пресет…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Пресет", &["json"])
                        .pick_file()
                {
                    let result =
                        ajazz_studio::storage::read_json::<AnimationSettings>(&path, 64 * 1024)
                            .and_then(|settings| {
                                settings.validate()?;
                                Ok(settings)
                            });
                    match result {
                        Ok(settings) => self.keyboard_animation = settings,
                        Err(error) => self.message(format!("Ошибка пресета: {error:#}")),
                    }
                }
            });
        });
        let scene = self.keyboard_animation.scene();
        let time = self
            .playback_progress
            .filter(|_| self.playback_cancel.is_some())
            .map(|p| p.scene_time)
            .unwrap_or_else(|| (ui.input(|i| i.time) as f32).rem_euclid(scene.duration));
        ui.label(
            "Предпросмотр · нажмите или протяните по клавишам, чтобы выбрать область анимации",
        );
        let other_selection =
            std::mem::replace(&mut self.selected, self.keyboard_animation.keys.clone());
        self.keyboard_canvas_for(ui, &scene, time, !self.busy);
        ui.add_enabled_ui(!self.busy, |ui| self.groups(ui));
        self.keyboard_animation.keys = std::mem::replace(&mut self.selected, other_selection);
        ui.small("Без выделения — вся клавиатура. Вне выделения сохраняются прежние цвета пользовательского режима.");
        ui.small("Передаются только изменившиеся участки. Все пакеты ждут ACK; полная проверка — первый кадр и раз в секунду. 16/33 мс — целевой интервал, фактическая частота ограничена клавиатурой. Очереди кадров нет. Настройки сохраняются автоматически, запуск — вручную.");
        ui.small("Передача использует сохранённые RGB-таблицы. Запись только в RAM и ресурс сохранений прошивки не установлены. Правки настроек — после остановки.");
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
    fn keyboard_page(&mut self, ui: &mut egui::Ui) {
        self.keyboard_navigation(ui);
        ui.add_space(12.0);
        match self.keyboard_tab {
            KeyboardTab::Hall => self.hall_page(ui),
            KeyboardTab::Encoder => self.keyboard_memory_panel(ui),
            KeyboardTab::Calibration => self.calibration_editor(ui),
            KeyboardTab::General => self.general_editor(ui),
            KeyboardTab::Test => self.keyboard_input_test(ui),
            KeyboardTab::Keymap | KeyboardTab::Macros => {
                self.keyboard_canvas(ui);
                self.groups(ui);
                ui.add_space(12.0);
                if self.keyboard_tab == KeyboardTab::Keymap {
                    self.keymap_editor(ui);
                } else {
                    self.macro_editor(ui);
                }
            }
        }
    }
    fn keyboard_input_test(&mut self, ui: &mut egui::Ui) {
        ui.heading("Проверка назначений и макросов");
        ui.label("Нажмите в поле ниже и проверьте физические клавиши. Здесь виден ввод, который получила Windows после обработки клавиатурой. Текст и журнал хранятся только до закрытия окна.");
        if ui.button("Очистить тест").clicked() {
            self.keyboard_test_text.clear();
            self.keyboard_test_log.clear();
            self.keyboard_test_started = Instant::now();
        }
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.keyboard_test_text)
                .hint_text("Нажмите здесь, затем проверяемую клавишу…")
                .char_limit(4096)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        if response.has_focus() {
            let events = ui.ctx().input(|input| input.events.clone());
            for event in events {
                if let egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } = event
                {
                    self.keyboard_test_log.push(format!(
                        "{:>7.3} с · {} {:?} · physical {:?} · Ctrl:{} Shift:{} Alt:{} Win:{}{}",
                        self.keyboard_test_started.elapsed().as_secs_f32(),
                        if pressed { "↓" } else { "↑" },
                        key,
                        physical_key,
                        modifiers.ctrl,
                        modifiers.shift,
                        modifiers.alt,
                        modifiers.mac_cmd || modifiers.command && !modifiers.ctrl,
                        if repeat { " · повтор" } else { "" }
                    ));
                }
            }
            if self.keyboard_test_log.len() > 32 {
                self.keyboard_test_log
                    .drain(..self.keyboard_test_log.len() - 32);
            }
        }
        ui.label(RichText::new("Время показывает получение событий приложением и не измеряет задержку или ход магнитного свитча.").color(MUTED));
        for line in self.keyboard_test_log.iter().rev() {
            ui.monospace(line);
        }
    }
    fn macro_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Макросы в памяти клавиатуры");
        ui.small("Создайте последовательность нажатий и отпусканий, затем сохраните её или назначьте одной клавише на схеме.");
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("Прочитать макросы · заменить черновик")).clicked() {
                self.read_macros(false);
            }
            if ui.add_enabled(!self.busy && self.macro_snapshot.is_none(),egui::Button::new("Обновить связь · сохранить черновик")).clicked() {
                self.read_macros(true);
            }
            if ui.add_enabled(!self.busy,egui::Button::new("Импорт JSON…")).clicked()
                && let Some(path)=rfd::FileDialog::new().add_filter("Macros",&["json"]).pick_file() {
                let loaded=(||->anyhow::Result<Vec<Macro>> {
                    let items:Vec<Macro>=ajazz_studio::storage::read_json(&path,128*1024)?;
                    for item in &items {item.validate()?;}
                    keyboard_macros::encode(&items,&vec![0;keyboard_macros::BANK_BYTES])?;
                    Ok(items)
                })();
                match loaded {Ok(items)=>{self.macro_draft=items;self.macro_slot=self.macro_draft.first().map(|m|m.slot).unwrap_or(0);self.message("Макросы импортированы в редактор; нажмите «Сохранить макросы» для записи"); if self.macro_snapshot.is_none() {self.read_macros(true);}},Err(e)=>self.message(format!("Импорт: {e:#}"))}
            }
            if ui.add_enabled(!self.busy,egui::Button::new("Экспорт JSON…")).clicked()
                && let Some(path)=rfd::FileDialog::new().set_file_name("keyboard-macros.json").save_file() {
                let result=ajazz_studio::storage::write_json_atomic(&path,&self.macro_draft);
                self.message(result.map(|()|"Макросы сохранены в файл".into()).unwrap_or_else(|e|format!("{e:#}")));
            }
        });
        let used: usize = self
            .macro_draft
            .iter()
            .map(|m| 4 + m.actions.len() * 4)
            .sum();
        ui.label(format!(
            "Память: {used} / 512 байт · {} макросов",
            self.macro_draft.len()
        ));
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("macro_slot")
                    .selected_text(format!("Макрос {}", self.macro_slot + 1))
                    .show_ui(ui, |ui| {
                        for item in &self.macro_draft {
                            ui.selectable_value(
                                &mut self.macro_slot,
                                item.slot,
                                format!(
                                    "Макрос {} · {} действий",
                                    item.slot + 1,
                                    item.actions.len()
                                ),
                            );
                        }
                    });
                if ui
                    .add_enabled(used + 12 <= 512, egui::Button::new("Новый макрос"))
                    .clicked()
                    && let Some(slot) =
                        (0..100).find(|slot| !self.macro_draft.iter().any(|m| m.slot == *slot))
                {
                    self.macro_slot = slot;
                    self.macro_draft.push(Macro {
                        slot,
                        actions: vec![
                            keyboard_macros::Action {
                                delay_ms: 20,
                                usage: 4,
                                pressed: true,
                                mouse: false,
                            },
                            keyboard_macros::Action {
                                delay_ms: 20,
                                usage: 4,
                                pressed: false,
                                mouse: false,
                            },
                        ],
                    });
                }
                if ui.button("Удалить выбранный").clicked() {
                    self.macro_draft.retain(|m| m.slot != self.macro_slot);
                }
            });
            if let Some(item) = self
                .macro_draft
                .iter_mut()
                .find(|m| m.slot == self.macro_slot)
            {
                let mut remove = None;
                let mut swap = None;
                egui::ScrollArea::vertical()
                    .id_salt("macro_action_scroll")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        egui::Grid::new("macro_actions")
                            .striped(true)
                            .show(ui, |ui| {
                                for text in
                                    ["№", "Устройство", "Клавиша", "Действие", "Задержка, мс", ""]
                                {
                                    ui.label(text);
                                }
                                ui.end_row();
                                let count = item.actions.len();
                                for (index, action) in item.actions.iter_mut().enumerate() {
                                    ui.label((index + 1).to_string());
                                    let was_mouse = action.mouse;
                                    egui::ComboBox::from_id_salt(("macro_device", index))
                                        .width(95.0)
                                        .selected_text(if action.mouse {
                                            "Мышь"
                                        } else {
                                            "Клавиатура"
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut action.mouse,
                                                false,
                                                "Клавиатура",
                                            );
                                            ui.selectable_value(&mut action.mouse, true, "Мышь");
                                        });
                                    if was_mouse != action.mouse {
                                        action.usage = if action.mouse { 1 } else { 4 };
                                    }
                                    if action.mouse {
                                        let buttons = [
                                            (1, "Левая"),
                                            (2, "Правая"),
                                            (4, "Средняя"),
                                            (8, "Назад"),
                                            (16, "Вперёд"),
                                        ];
                                        egui::ComboBox::from_id_salt(("macro_mouse", index))
                                            .width(90.0)
                                            .selected_text(
                                                buttons
                                                    .iter()
                                                    .find(|(v, _)| *v == action.usage)
                                                    .map(|(_, s)| *s)
                                                    .unwrap_or("Неизвестно"),
                                            )
                                            .show_ui(ui, |ui| {
                                                for (value, label) in buttons {
                                                    ui.selectable_value(
                                                        &mut action.usage,
                                                        value,
                                                        label,
                                                    );
                                                }
                                            });
                                    } else {
                                        keyboard_usage_editor(
                                            ui,
                                            &format!("macro_usage_{index}"),
                                            &mut action.usage,
                                        );
                                    }
                                    egui::ComboBox::from_id_salt(("macro_press", index))
                                        .width(100.0)
                                        .selected_text(if action.pressed {
                                            "Нажать"
                                        } else {
                                            "Отпустить"
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut action.pressed,
                                                true,
                                                "Нажать",
                                            );
                                            ui.selectable_value(
                                                &mut action.pressed,
                                                false,
                                                "Отпустить",
                                            );
                                        });
                                    ui.add(
                                        egui::DragValue::new(&mut action.delay_ms)
                                            .range(0..=60000)
                                            .speed(5),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_enabled(index > 0, egui::Button::new("↑"))
                                            .clicked()
                                        {
                                            swap = Some((index, index - 1));
                                        }
                                        if ui
                                            .add_enabled(index + 1 < count, egui::Button::new("↓"))
                                            .clicked()
                                        {
                                            swap = Some((index, index + 1));
                                        }
                                        if ui.button("×").clicked() {
                                            remove = Some(index);
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                if let Some(index) = remove {
                    item.actions.remove(index);
                }
                if let Some((a, b)) = swap {
                    item.actions.swap(a, b);
                }
                if ui
                    .add_enabled(
                        used + 8 <= 512,
                        egui::Button::new("Добавить нажатие и отпускание"),
                    )
                    .clicked()
                {
                    item.actions.extend([
                        keyboard_macros::Action {
                            delay_ms: 20,
                            usage: 4,
                            pressed: true,
                            mouse: false,
                        },
                        keyboard_macros::Action {
                            delay_ms: 20,
                            usage: 4,
                            pressed: false,
                            mouse: false,
                        },
                    ]);
                }
                if let Err(error) = item.validate() {
                    ui.colored_label(AMBER, error.to_string());
                }
            }
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("macro_mode")
                    .selected_text(match self.macro_mode {
                        1 => "Заданное число повторов",
                        2 => "До повторного нажатия",
                        _ => "Обычный",
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (0, "Обычный"),
                            (1, "Заданное число повторов"),
                            (2, "До повторного нажатия"),
                        ] {
                            ui.selectable_value(&mut self.macro_mode, value, label);
                        }
                    });
                if self.macro_mode == 1 {
                    ui.add(
                        egui::DragValue::new(&mut self.macro_repeats)
                            .range(1..=255)
                            .prefix("Повторов: "),
                    );
                }
            });
        });
        let edit = MacroEdit {
            macros: self.macro_draft.clone(),
            binding: None,
        };
        let validation = self
            .macro_snapshot
            .as_ref()
            .map(|before| keyboard_macros::target(before, &edit));
        if let Some(Err(error)) = &validation {
            ui.colored_label(AMBER, error.to_string());
        }
        if self.macro_snapshot.is_none() {
            ui.colored_label(AMBER,"Прочитайте макросы перед записью. Чтение заменит черновик данными клавиатуры; его можно предварительно экспортировать.");
        }
        let valid = validation.as_ref().is_some_and(|r| r.is_ok());
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy && valid, egui::Button::new("Сохранить макросы"))
                .clicked()
            {
                self.apply_macros(edit.clone());
            }
            let binding = self.selected.first().copied().map(|key| MacroBinding {
                key,
                slot: self.macro_slot,
                mode: self.macro_mode,
                repeats: if self.macro_mode == 1 {
                    self.macro_repeats
                } else {
                    0
                },
            });
            let bound = MacroEdit {
                macros: self.macro_draft.clone(),
                binding,
            };
            let can_bind = self.selected.len() == 1
                && self
                    .macro_snapshot
                    .as_ref()
                    .is_some_and(|before| keyboard_macros::target(before, &bound).is_ok());
            if ui
                .add_enabled(
                    !self.busy && can_bind,
                    egui::Button::new("Сохранить и назначить выбранной клавише"),
                )
                .clicked()
            {
                self.apply_macros(bound);
            }
            if ui
                .add_enabled(
                    !self.busy && self.macro_backup.is_some(),
                    egui::Button::new("Вернуть макросы и назначения"),
                )
                .clicked()
                && let Some(path) = self.macro_backup.clone()
            {
                self.restore_macros(path);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Восстановить из копии…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Macro backup", &["json"])
                    .pick_file()
            {
                self.restore_macros(path);
            }
        });
    }
    fn read_macros(&mut self, preserve_draft: bool) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let wired = self.playback_channel == PlaybackChannel::Usb;
        self.message(if preserve_draft {
            "Читаю исходные настройки; черновик макросов сохранён…"
        } else {
            "Читаю макросы и назначения…"
        });
        std::thread::spawn(move || {
            let _ = tx.send(Event::MacrosRead(
                keyboard_macros::read(wired).map_err(|e| format!("{e:#}")),
                preserve_draft,
            ));
        });
    }
    fn apply_macros(&mut self, edit: MacroEdit) {
        if self.busy {
            return;
        }
        let Some(expected) = self.macro_snapshot.clone() else {
            return;
        };
        self.busy = true;
        self.message("Сохраняю копию и записываю макросы…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::MacrosWrite(
                keyboard_macros::apply(&expected, &edit).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_macros(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю макросы и назначения…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::MacrosWrite(
                keyboard_macros::restore(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn general_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && self.general_backup.is_some(),
                    egui::Button::new("Вернуть предыдущие настройки"),
                )
                .clicked()
                && let Some(path) = self.general_backup.clone()
            {
                self.restore_general(path);
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Восстановить общие настройки…"),
                )
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Keyboard backup", &["json"])
                    .pick_file()
            {
                self.restore_general(path);
            }
        });
        if let Some(path) = &self.general_backup {
            ui.label(
                RichText::new(format!("Копия до последней записи: {}", path.display()))
                    .size(11.0)
                    .color(MUTED),
            );
        }
        let (Some(snapshot), Some(settings)) = (&self.snapshot, &mut self.general_settings) else {
            ui.colored_label(AMBER, "Настройки загружаются автоматически. Если данные не появились, проверьте подключение и нажмите «Обновить».");
            return;
        };
        ui.add_space(16.0);
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.columns(2, |cols| {
                cols[0].heading("Питание и стабильность");
                cols[0].add(egui::Slider::new(&mut settings.sleep_minutes,0..=30).text("Сон через, мин"));
                cols[0].label(RichText::new("0 — не переходить в сон").size(12.0).color(MUTED));
                cols[0].checkbox(&mut settings.stability,"Стабилизация магнитных свитчей");
                cols[0].checkbox(&mut settings.auto_calibration,"Автоматическая калибровка");
                cols[0].add(egui::Slider::new(&mut settings.key_delay,0..=5).text("Уровень задержки клавиш"));
                cols[0].label(RichText::new("0 сохраняет заводское значение уровня задержки").size(12.0).color(MUTED));
                cols[1].heading("Мёртвые зоны");
                let mut top=f32::from(settings.top_dead_zone)/100.0;
                let mut bottom=f32::from(settings.bottom_dead_zone)/100.0;
                if cols[1].add(egui::Slider::new(&mut top,0.0..=0.5).step_by(0.01).text("Сверху, мм")).changed() { settings.top_dead_zone=(top*100.0).round() as u8; }
                if cols[1].add(egui::Slider::new(&mut bottom,0.0..=0.5).step_by(0.01).text("Снизу, мм")).changed() { settings.bottom_dead_zone=(bottom*100.0).round() as u8; }
                cols[1].label("Участки хода, в которых движение свитча не изменяет состояние клавиши. 0 отключает соответствующую мёртвую зону.");
            });
        });
        let original = snapshot.blocks["game_mode"].clone();
        let changed = settings.patch(&original).is_ok_and(|v| v != original);
        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && changed,
                    egui::Button::new("Применить общие настройки"),
                )
                .clicked()
            {
                self.apply_general();
            }
            if ui
                .add_enabled(!self.busy && changed, egui::Button::new("Сбросить правки"))
                .clicked()
            {
                self.general_settings = GeneralSettings::parse(&original).ok();
            }
        });
    }
    fn keymap_editor(&mut self, ui: &mut egui::Ui) {
        let Some(snapshot) = self.snapshot.clone() else {
            ui.colored_label(AMBER, "Настройки загружаются автоматически. Если данные не появились, проверьте подключение и нажмите «Обновить».");
            return;
        };
        if self.keymap_selection.as_ref() != Some(&self.selected) {
            self.keymap_assignment = self
                .selected
                .first()
                .and_then(|id| {
                    let at = usize::from(*id) * 4;
                    Assignment::decode(&snapshot.blocks["keymap"][at..at + 4])
                })
                .unwrap_or(Assignment::Default);
            self.keymap_selection = Some(self.selected.clone());
            self.dks_config = self
                .selected
                .first()
                .and_then(|key| ajazz_studio::keyboard_dks::for_key(&snapshot, *key).ok());
        }
        ui.heading("Назначение для выбранных клавиш");
        ui.add_enabled_ui(!self.busy, |ui| {
            let mut mode = match self.keymap_assignment {
                Assignment::Default => 0,
                Assignment::Key { .. } => 1,
                Assignment::Media { .. } => 2,
                Assignment::Toggle { .. } => 3,
                Assignment::ModTap { .. } => 4,
                Assignment::Socd { .. } => 5,
                Assignment::RappySnappy { .. } => 6,
                Assignment::Dks { .. } => 7,
                Assignment::Macro { .. } => 8,
                Assignment::Combo { .. } => 9,
                Assignment::Mouse { .. } => 10,
            };
            let before = mode;
            egui::ComboBox::from_id_salt("keymap_kind")
                .selected_text(["По умолчанию", "Клавиша", "Мультимедиа", "TGL · переключатель", "MT · нажатие и удержание", "SOCD · приоритет пары", "RS · Rappy Snappy", "DKS · действия по ходу", "Макрос", "Сочетание клавиш", "Функция мыши"][mode])
                .show_ui(ui, |ui| {
                    for (value, label) in ["По умолчанию", "Клавиша", "Мультимедиа", "TGL · переключатель", "MT · нажатие и удержание", "SOCD · приоритет пары", "RS · Rappy Snappy", "DKS · действия по ходу"].iter().enumerate() {
                        ui.selectable_value(&mut mode, value, *label);
                    }
                    ui.selectable_value(&mut mode,9,"Сочетание клавиш");
                    ui.selectable_value(&mut mode,10,"Функция мыши");
                });
            if before != mode {
                let usages: Vec<_> = keyboard_rows().into_iter().flatten().filter(|key| self.selected.contains(&key.id) && key.hid != 0).map(|key| key.hid).collect();
                let first = usages.first().copied().unwrap_or(4);
                let second = usages.get(1).copied().unwrap_or(7);
                self.keymap_assignment = match mode {
                    1 => Assignment::Key { usage: 4 },
                    2 => Assignment::Media { usage: 205 },
                    3 => Assignment::Toggle { usage: first },
                    4 => Assignment::ModTap { hold: 224, tap: first, hold_ms: 400 },
                    5 => Assignment::Socd { mode: 3, first, second },
                    6 => Assignment::RappySnappy { first, second },
                    7 => Assignment::Dks {slot:0},
                    9 => Assignment::Combo {first:224,second:0,key:4},
                    10 => Assignment::Mouse {kind:1,value:1},
                    _ => Assignment::Default,
                };
            }
            let text = self.keymap_assignment.label();
            match &mut self.keymap_assignment {
                Assignment::Combo {first,second,key} => {
                    for (index,value) in [first,second].into_iter().enumerate() {
                        egui::ComboBox::from_id_salt(("combo_modifier",index)).selected_text(keymap::MODIFIERS.iter().find(|(code,_)|code==value).map(|(_,label)|*label).unwrap_or("Не задан")).show_ui(ui,|ui|{
                            ui.selectable_value(value,0,"Не задан");
                            for &(code,label) in keymap::MODIFIERS {ui.selectable_value(value,code,label);}
                        });
                    }
                    keyboard_usage_editor(ui,"combo_key",key);
                }
                Assignment::Mouse {kind,value} => {
                    egui::ComboBox::from_id_salt("keyboard_mouse_action").selected_text(text).show_ui(ui,|ui|{
                        for &(k,v,label) in keymap::MOUSE_ACTIONS {
                            if ui.selectable_label(*kind==k && *value==v,label).clicked() {*kind=k;*value=v;}
                        }
                    });
                }
                Assignment::Macro {..} => {ui.label("Редактирование и назначение доступны на вкладке «Макросы». Выберите «По умолчанию», чтобы снять назначение.");},
                Assignment::Default => {
                    ui.label("Восстановить штатное действие выбранных клавиш. Для пары RS/SOCD выберите обе клавиши.");
                }
                Assignment::Key { usage } => keyboard_usage_editor(ui, "keymap_keyboard_usage", usage),
                Assignment::Media { usage } => {
                    egui::ComboBox::from_id_salt("keymap_media_usage")
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            for &(code, label) in keymap::MEDIA {
                                ui.selectable_value(usage, code, label);
                            }
                        });
                }
                Assignment::Toggle { usage } => {
                    ui.label("Первое нажатие удерживает действие, следующее отпускает его.");
                    keyboard_usage_editor(ui, "keymap_toggle_usage", usage);
                }
                Assignment::ModTap { hold, tap, hold_ms } => {
                    ui.label("Короткое нажатие");
                    keyboard_usage_editor(ui, "keymap_mt_tap", tap);
                    ui.label("Удержание");
                    keyboard_usage_editor(ui, "keymap_mt_hold", hold);
                    let mut displayed_ms = if *hold_ms == 0 { 400 } else { *hold_ms };
                    if ui.add(egui::Slider::new(&mut displayed_ms, 10..=1000).step_by(10.0).text("Время удержания, мс")).changed() {
                        *hold_ms = displayed_ms;
                    }
                }
                Assignment::Socd { mode, first, second } => {
                    keyboard_pair_editor(ui, first, second);
                    egui::ComboBox::from_id_salt("keymap_socd_mode")
                        .selected_text(keymap::SOCD_MODES.iter().find(|(value, _)| value == mode).map(|(_, label)| *label).unwrap_or("Неизвестный режим"))
                        .show_ui(ui, |ui| {
                            for &(value, label) in keymap::SOCD_MODES { ui.selectable_value(mode, value, label); }
                        });
                    ui.small("Порог срабатывания и Rapid Trigger настраиваются отдельно в магнитных настройках.");
                }
                Assignment::RappySnappy { first, second } => {
                    keyboard_pair_editor(ui, first, second);
                    ui.label("Rappy Snappy выбирает действие пары по глубине нажатия.");
                    ui.small("Порог срабатывания и Rapid Trigger настраиваются отдельно в магнитных настройках.");
                }
                Assignment::Dks {..} => {
                    ui.label("Четыре действия на двух точках нажатия и двух точках отпускания. Выберите одну физическую клавишу.");
                    if let Some(settings)=&mut self.dks_config {
                        for (index,label) in ["Нажатие 1, мм","Нажатие 2, мм","Отпускание 1, мм","Отпускание 2, мм"].iter().enumerate() {
                            let mut mm=f32::from(settings.points[index])/10.0;
                            if ui.add(egui::Slider::new(&mut mm,0.1..=3.3).step_by(0.1).text(*label)).changed() {settings.points[index]=(mm*10.0).round() as u8;}
                        }
                        egui::Grid::new("dks_actions").show(ui,|ui| {
                            ui.label("Действие");for label in ["↓ 1","↓ 2","↑ 1","↑ 2"] {ui.label(label);}ui.end_row();
                            for action in 0..4 {
                                egui::ComboBox::from_id_salt(("dks_usage",action)).selected_text(if settings.actions[action]==0 {"Не задано".into()} else {keymap::usage_label(settings.actions[action])}).show_ui(ui,|ui| {
                                    ui.selectable_value(&mut settings.actions[action],0,"Не задано");
                                    for key in keyboard_rows().into_iter().flatten().filter(|key|key.hid!=0) {ui.selectable_value(&mut settings.actions[action],key.hid,key.physical);}
                                });
                                if settings.actions[action]==0 {settings.states[action]=[0;4];}
                                for stage in 0..4 {
                                    ui.add_enabled_ui(settings.actions[action]!=0,|ui| {
                                        egui::ComboBox::from_id_salt(("dks_stage",action,stage)).selected_text(["—","Нажать","Удерживать"][usize::from(settings.states[action][stage])]).width(90.0).show_ui(ui,|ui| {
                                            for (value,label) in ["—","Нажать","Удерживать"].iter().enumerate() {ui.selectable_value(&mut settings.states[action][stage],value as u8,*label);}
                                        });
                                    });
                                }
                                ui.end_row();
                            }
                        });
                    } else {ui.colored_label(AMBER,"Не удалось прочитать запись DKS. Прочитайте настройки и выберите одну поддерживаемую клавишу.");}
                }
            }
        });
        ui.add_space(10.0);
        for key in keyboard_rows()
            .into_iter()
            .flatten()
            .filter(|k| self.selected.contains(&k.id))
        {
            let at = usize::from(key.id) * 4;
            let label = Assignment::decode(&snapshot.blocks["keymap"][at..at + 4])
                .map(|a| a.label())
                .unwrap_or_else(|| "Сложное назначение; редактор ещё переносится".into());
            ui.label(format!("{} → {}", key.physical, label));
        }
        let edit = KeymapEdit {
            keys: self.selected.clone(),
            assignment: self.keymap_assignment.clone(),
        };
        let dks = matches!(self.keymap_assignment, Assignment::Dks { .. });
        let target = if dks {
            (|| -> anyhow::Result<bool> {
                anyhow::ensure!(self.selected.len() == 1, "DKS: выберите ровно одну клавишу");
                let settings = self
                    .dks_config
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Нет настроек DKS"))?;
                let (bank, map) = ajazz_studio::keyboard_dks::target(
                    &snapshot,
                    *self.selected.first().unwrap(),
                    settings,
                )?;
                Ok(bank != snapshot.blocks["dks"] || map != snapshot.blocks["keymap"])
            })()
        } else {
            keymap::target(&snapshot, &edit).map(|t| t != snapshot.blocks["keymap"])
        };
        let changed = target.as_ref().is_ok_and(|changed| *changed);
        if let Err(e) = target {
            ui.colored_label(AMBER, e.to_string());
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && changed,
                    egui::Button::new("Применить назначения"),
                )
                .clicked()
            {
                if dks {
                    self.apply_dks();
                } else {
                    self.apply_keymap();
                }
            }
            if ui
                .add_enabled(
                    !self.busy && self.keymap_backup.is_some(),
                    egui::Button::new("Отменить назначения"),
                )
                .clicked()
                && let Some(path) = self.keymap_backup.clone()
            {
                self.restore_keymap(path);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Восстановить назначения…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Keyboard backup", &["json"])
                    .pick_file()
            {
                self.restore_keymap(path);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && self.dks_backup.is_some(),
                    egui::Button::new("Вернуть DKS и назначения"),
                )
                .clicked()
                && let Some(path) = self.dks_backup.clone()
            {
                self.restore_dks(path);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Восстановить DKS из копии…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Keyboard backup", &["json"])
                    .pick_file()
            {
                self.restore_dks(path);
            }
        });
        ui.label(RichText::new("Fn, сочетания и макросы переносятся отдельно. Для группы загружается назначение первой выбранной клавиши.").color(MUTED));
    }
    fn read_buttons(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Читаю назначения кнопок…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::ButtonsRead(
                mouse_buttons::read().map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn apply_buttons(&mut self) {
        if self.busy {
            return;
        }
        let Some(snapshot) = self.buttons_snapshot.clone() else {
            return;
        };
        let edit = ButtonEdit {
            slot: self.buttons_slot,
            action: self.buttons_action,
        };
        self.busy = true;
        self.message("Сохраняю копию, записываю и проверяю назначения…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::ButtonsWrite(
                mouse_buttons::apply(&snapshot, &edit).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn restore_buttons(&mut self, path: std::path::PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Восстанавливаю назначения кнопок…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::ButtonsWrite(
                mouse_buttons::restore(&path).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    fn mouse_buttons_page(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Восстановление", |ui| {
            if ui
                .add_enabled(
                    !self.busy && self.buttons_backup.is_some(),
                    egui::Button::new("Отменить последнюю запись"),
                )
                .clicked()
                && let Some(path) = self.buttons_backup.clone()
            {
                self.restore_buttons(path);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Восстановить из копии…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Копия кнопок", &["json"])
                    .pick_file()
            {
                self.restore_buttons(path);
            }
        });

        let Some(snapshot) = self.buttons_snapshot.clone() else {
            ui.label("Если настройки не появились, разбудите мышь и нажмите «Обновить».");
            return;
        };
        ui.label(
            RichText::new(format!("AJ159 APEX · профиль {}", snapshot.profile + 1)).color(TEAL),
        );
        ui.add_space(12.0);
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.columns(2, |cols| {
                cols[0].heading("Кнопка");
                for &(slot, label) in mouse_buttons::BUTTONS {
                    if cols[0]
                        .selectable_value(&mut self.buttons_slot, slot, label)
                        .changed()
                    {
                        self.buttons_action =
                            snapshot.action(slot).unwrap_or(MouseAction::Disabled);
                    }
                }
                let current = snapshot.action(self.buttons_slot);
                cols[1].heading("Действие");
                cols[1].label(format!(
                    "Сейчас: {}",
                    current
                        .map(MouseAction::label)
                        .unwrap_or_else(|| "Сложное назначение (сохраняется)".into())
                ));
                cols[1].add_enabled_ui(current.is_some(), |ui| {
                    egui::ComboBox::from_id_salt("mouse_action")
                        .selected_text(self.buttons_action.label())
                        .show_ui(ui, |ui| {
                            for action in MouseAction::choices() {
                                let label = if matches!(action, MouseAction::Key { .. }) {
                                    "Клавиша / сочетание…".into()
                                } else {
                                    action.label()
                                };
                                let selected = self.buttons_action == action
                                    || (matches!(action, MouseAction::Key { .. })
                                        && matches!(self.buttons_action, MouseAction::Key { .. }));
                                if ui.selectable_label(selected, label).clicked() {
                                    self.buttons_action = action;
                                }
                            }
                        });
                    if let MouseAction::Key { usage, modifier } = &mut self.buttons_action {
                        egui::ComboBox::from_id_salt("mouse_modifier")
                            .selected_text(modifier.label())
                            .show_ui(ui, |ui| {
                                for item in Modifier::ALL {
                                    ui.selectable_value(modifier, item, item.label());
                                }
                            });
                        let keys = keyboard_rows()
                            .into_iter()
                            .flatten()
                            .filter(|k| k.hid > 0)
                            .collect::<Vec<_>>();
                        let name = keys
                            .iter()
                            .find(|k| k.hid == *usage)
                            .map(|k| k.physical.as_str())
                            .unwrap_or("Клавиша");
                        egui::ComboBox::from_id_salt("mouse_key")
                            .selected_text(name)
                            .show_ui(ui, |ui| {
                                for key in keys {
                                    ui.selectable_value(usage, key.hid, key.physical);
                                }
                            });
                    }
                    if ui.button("Штатное действие этой кнопки").clicked()
                        && let Some(action) = MouseAction::default_for(self.buttons_slot)
                    {
                        self.buttons_action = action;
                    }
                });
            });
        });
        let edit = ButtonEdit {
            slot: self.buttons_slot,
            action: self.buttons_action,
        };
        let validation = snapshot.target(&edit);
        let changed = validation.as_ref().is_ok_and(|v| !v.equivalent(&snapshot));
        if let Err(error) = validation {
            ui.colored_label(AMBER, error.to_string());
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && changed,
                    egui::Button::new("Применить назначение"),
                )
                .clicked()
            {
                self.apply_buttons();
            }
            if ui
                .add_enabled(!self.busy && changed, egui::Button::new("Сбросить правки"))
                .clicked()
            {
                self.buttons_action = snapshot
                    .action(self.buttons_slot)
                    .unwrap_or(MouseAction::Disabled);
            }
        });
        ui.label(RichText::new("Одна физическая кнопка должна сохранить левый клик. Макросы и Fn пока не редактируются; служебные записи сохраняются.").color(MUTED));
        if let Some(path) = &self.buttons_backup {
            ui.label(
                RichText::new(format!("Копия до последней записи: {}", path.display()))
                    .size(11.0)
                    .color(MUTED),
            );
        }
    }
    fn update_pages_status(&mut self, status: dock_pages::Status, replace: bool) {
        if replace {
            self.dock_pages = status.playlist.clone();
        } else if status.erasing.is_none()
            && status.progress == 100
            && let Some(asset) = self.page_status.as_ref().and_then(|s| s.erasing)
        {
            self.dock_pages
                .pages
                .retain(|p| p.kind != dock_pages::Kind::Media || p.asset != asset);
            if self.dock_pages.pages.is_empty() {
                self.dock_pages.pages.push(dock_pages::Page {
                    kind: dock_pages::Kind::Clock,
                    ..Default::default()
                });
            }
            if !self.dock_pages.pages.iter().any(|p| p.enabled) {
                self.dock_pages.pages[0].enabled = true;
            }
            self.dock_pages.selected = self
                .dock_pages
                .selected
                .min(self.dock_pages.pages.len() as u8 - 1);
            self.message(format!(
                "{} удалён из памяти дока.",
                dock_pages::asset_label(asset)
            ));
        }
        self.page_status = Some(status);
    }
    fn start_screen_stream(&mut self) {
        if self.screen_commands.is_some()
            || self.exit_requested
            || self.update_pending.is_some()
            || self.update_prepared.is_some()
            || self.update_running
        {
            return;
        }
        let mut graphics_options = self.graphics_options.clone();
        let tx = self.tx.clone();
        let hidden = self.window_hidden.clone();
        let (commands, receiver) = mpsc::channel();
        self.screen_commands = Some(commands);
        self.fps_requires_admin = false;
        std::thread::spawn(move || {
            let mut last_error = String::new();
            let mut notify = |update| {
                let transient = matches!(
                    &update,
                    dock_telemetry::Update::Sample(_)
                        | dock_telemetry::Update::Pages(_, false)
                        | dock_telemetry::Update::Timer(_)
                );
                if !transient || !hidden.load(Ordering::Relaxed) {
                    let _ = tx.send(Event::ScreenUpdate(update));
                }
            };
            let result = loop {
                match dock_telemetry::run(
                    Default::default(),
                    Default::default(),
                    &mut graphics_options,
                    &receiver,
                    true,
                    &mut notify,
                ) {
                    Ok(()) => break Ok(()),
                    Err(e) => {
                        notify(dock_telemetry::Update::Connection(false));
                        let error = format!("{e:#}");
                        if error != last_error {
                            notify(dock_telemetry::Update::Notice(format!(
                                "Соединение дока: {error}"
                            )));
                            last_error = error;
                        }
                        match receiver.recv_timeout(Duration::from_secs(3)) {
                            Ok(dock_telemetry::Command::Stop)
                            | Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            _ => notify(dock_telemetry::Update::Notice(
                                "Настройки дока: устройство отключено. Черновик сохранён в Studio."
                                    .into(),
                            )),
                        }
                    }
                }
            };
            let _ = tx.send(Event::ScreenFinished(result));
        });
    }
    fn enable_fps(&mut self) {
        let tx = self.tx.clone();
        self.message("Подтвердите запрос Windows для фонового сборщика показателей.");
        std::thread::spawn(move || {
            let result = ajazz_studio::metrics_agent::request()
                .map(|()| "Сборщик подключается. Показатели появятся автоматически.".into())
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Done(result));
        });
    }
    fn pages_read(&mut self, replace: bool) {
        if let Some(commands) = &self.screen_commands {
            let _ = commands.send(dock_telemetry::Command::ReadPages(replace));
            return;
        }
        self.busy = true;
        self.pages_poll_inflight = !replace;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if replace {
                let result = dock_telemetry::read_editor().map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::EditorRead(result));
            } else {
                let result = dock_telemetry::pages_info().map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::PagesPoll(result));
            }
        });
    }
    fn pages_apply(&mut self, save: bool) {
        let mut p = self.dock_pages.clone();
        p.enabled = true;
        let mut config = self.dock_telemetry.clone();
        config.enabled = true;
        if self
            .page_status
            .as_ref()
            .is_some_and(|s| s.capabilities & 128 == 0)
            && (config.style != 0
                || config.focus_minutes != 25
                || p.pages.iter().any(|e| e.kind as u8 > 4))
        {
            self.message("Новые экраны и оформление готовы в редакторе; для применения требуется обновление OLED.");
            return;
        }
        if let Err(error) = p.validate().and_then(|_| config.validate()) {
            self.message(error.to_string());
            return;
        }
        if let Some(commands) = &self.screen_commands {
            let _ = commands.send(dock_telemetry::Command::Editor(config, p, save, None));
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result =
                dock_telemetry::apply_editor(&config, &p, save).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::PagesResult(result, false));
        });
    }
    fn dock_telemetry_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.dock_telemetry.style != 0 {
            ui.label("Для сводки выбран готовый шаблон. Его цвета и яркость настраиваются рядом с предпросмотром во вкладке «Страницы дока».");
            if ui
                .button("Перейти к классической раскладке с координатами")
                .clicked()
            {
                self.dock_telemetry.style = 0;
            } else {
                return;
            }
        }
        use chrono::Timelike;
        let running = self.screen_commands.is_some();
        #[cfg(windows)]
        if !running && self.screen_sample_time.elapsed() >= Duration::from_secs(1) {
            if let Some(counters) = self.screen_counters.as_mut() {
                match counters.sample(0) {
                    Ok(sample) => self.screen_sample = Some(sample),
                    Err(error) => {
                        self.message(format!("Не удалось обновить показатели ПК: {error:#}"));
                        self.screen_counters = dock_telemetry::windows::Counters::new().ok();
                    }
                }
            }
            self.screen_sample_time = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        ui.horizontal_wrapped(|ui| {
            ui.strong("Загрузка CPU и физическая память");
            ui.label(
                RichText::new(if running {
                    "● ПЕРЕДАЧА НА ДОК"
                } else {
                    "● ПРЕДПРОСМОТР ПК"
                })
                .color(TEAL),
            );
        });
        ui.label("Показатели обновляются раз в секунду, пока приложение открыто. Часы синхронизируются при запуске передачи.");
        ui.columns(2, |cols| {
            let now = chrono::Local::now();
            match ajazz_studio::dock_telemetry_preview::render(
                &self.dock_telemetry,
                self.screen_sample.as_ref(),
                now.hour() as u8,
                now.minute() as u8,
            ) {
                Ok(img) => {
                    let color = egui::ColorImage::from_rgba_unmultiplied([128, 128], img.as_raw());
                    if let Some(texture) = self.screen_texture.as_mut() {
                        texture.set(color, egui::TextureOptions::NEAREST);
                    } else {
                        self.screen_texture = Some(ctx.load_texture(
                            "cpu_ram_screen",
                            color,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                    cols[0].vertical_centered(|ui| {
                        ui.add_space(18.0);
                        egui::Frame::new()
                            .fill(Color32::from_rgb(5, 8, 13))
                            .inner_margin(16.0)
                            .corner_radius(18.0)
                            .show(ui, |ui| {
                                ui.image((
                                    self.screen_texture.as_ref().unwrap().id(),
                                    Vec2::splat(300.0),
                                ));
                            });
                        ui.small(
                            "Шрифт, расположение и цвета соответствуют экранной прошивке AJSC v1.",
                        );
                        if let Some(s) = &self.screen_sample {
                            ui.label(format!(
                                "CPU {:.1}% · RAM {:.1}%",
                                f32::from(s.cpu_permille) / 10.0,
                                f32::from(s.ram_permille) / 10.0
                            ));
                            ui.label(format!(
                                "{:.2} / {:.2} ГиБ",
                                f64::from(s.used_mib) / 1024.0,
                                f64::from(s.total_mib) / 1024.0
                            ));
                        }
                    });
                }
                Err(error) => {
                    cols[0].colored_label(AMBER, error.to_string());
                }
            }
            let ui = &mut cols[1];
            let c = &mut self.dock_telemetry;
            ui.strong("Оформление");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut c.percentages, "Проценты");
                ui.checkbox(&mut c.bars, "Полосы");
                ui.checkbox(&mut c.memory_size, "Объём памяти");
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut c.clock, "Часы на сводке");
                ui.checkbox(&mut c.show_title, "Заголовок");
                ui.checkbox(&mut c.border, "Рамка");
            });
            ui.text_edit_singleline(&mut c.title);
            ui.small("Заголовок: до 20 латинских букв, цифр и знаков.");
            ui.horizontal(|ui| {
                ui.label("Фон");
                ui.color_edit_button_srgb(&mut c.background);
                ui.label("Текст");
                ui.color_edit_button_srgb(&mut c.foreground);
                ui.label("Акцент");
                ui.color_edit_button_srgb(&mut c.accent);
            });
            ui.add(egui::Slider::new(&mut c.brightness, 1..=100).text("Яркость экрана, %"));
            ui.add(egui::Slider::new(&mut c.label_scale, 1..=2).text("Масштаб CPU/RAM"));
            ui.add(egui::Slider::new(&mut c.clock_scale, 1..=3).text("Масштаб часов"));
            ui.add(egui::Slider::new(&mut c.bar_width, 16..=124).text("Ширина полос"));
            ui.add(egui::Slider::new(&mut c.bar_height, 2..=12).text("Высота полос"));
            egui::Grid::new("cpu_screen_positions").show(ui, |ui| {
                for (label, position) in [
                    ("CPU", &mut c.cpu_position),
                    ("RAM", &mut c.ram_position),
                    ("Часы", &mut c.clock_position),
                    ("Заголовок", &mut c.title_position),
                ] {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut position[0])
                            .range(0..=127)
                            .prefix("X "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut position[1])
                            .range(0..=127)
                            .prefix("Y "),
                    );
                    ui.end_row();
                }
            });
        });
        let validation = self.dock_telemetry.validate();
        if let Err(error) = &validation {
            ui.colored_label(AMBER, error.to_string());
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Прочитать оформление с дока"))
                .clicked()
            {
                self.busy = true;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = dock_telemetry::probe().map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::ScreenProbe(result));
                });
            }
            for (label, save) in [
                ("Применить оформление", false),
                ("Сохранить оформление в доке", true),
            ] {
                if ui
                    .add_enabled(
                        (!self.busy || running) && validation.is_ok(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    let mut config = self.dock_telemetry.clone();
                    config.enabled = true;
                    if let Some(commands) = &self.screen_commands {
                        if commands
                            .send(dock_telemetry::Command::Config(config, save))
                            .is_err()
                        {
                            self.message("Передача уже завершилась; повторите применение.");
                        }
                    } else {
                        self.busy = true;
                        let tx = self.tx.clone();
                        let pages = self.dock_pages.clone();
                        std::thread::spawn(move || {
                            let result = dock_telemetry::apply_with_pages(&config, &pages, save)
                                .map(|_| ())
                                .map_err(|e| format!("{e:#}"));
                            let _ = tx.send(Event::ScreenApplied(result, save));
                        });
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Сбросить макет").clicked() {
                self.dock_telemetry = Default::default();
            }
            if ui.button("Экспорт оформления…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("dock-cpu-ram.json")
                    .save_file()
            {
                let result = self.dock_telemetry.validate().and_then(|()| {
                    ajazz_studio::storage::write_json_atomic(&path, &self.dock_telemetry)
                });
                self.message(
                    result
                        .map(|()| "Оформление сохранено в JSON".into())
                        .unwrap_or_else(|e| format!("{e:#}")),
                );
            }
            if ui.button("Импорт оформления…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Оформление", &["json"])
                    .pick_file()
            {
                let result =
                    ajazz_studio::storage::read_json::<dock_telemetry::Config>(&path, 8192)
                        .and_then(|c| {
                            c.validate()?;
                            Ok(c)
                        });
                match result {
                    Ok(c) => {
                        self.dock_telemetry = c;
                        self.message("Оформление открыто для редактирования.");
                    }
                    Err(e) => self.message(format!("{e:#}")),
                }
            }
        });
        if self.screen_supported != Some(true) {
            ui.colored_label(AMBER,"Для вывода CPU/RAM нужна экранная прошивка AJSC v1. Штатная OLEDV104 не принимает поток показателей.");
            ui.small(
                "Сборка прошивки и её аппаратная проверка выполняются отдельно от предпросмотра.",
            );
        }
    }
    fn dock_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        design::tabs(ui, |ui| {
            ui.selectable_value(&mut self.dock_tab, DockTab::Pages, "Экраны");
            ui.selectable_value(&mut self.dock_tab, DockTab::Media, "Фото и GIF");
            if self.dock_tab == DockTab::Telemetry {
                ui.label(RichText::new("Ручной макет").color(MUTED));
            }
        });
        ui.add_space(12.0);
        if self.dock_tab == DockTab::Pages {
            self.dock_pages_page(ui, ctx);
            return;
        }
        if self.dock_tab == DockTab::Telemetry {
            self.dock_telemetry_page(ui, ctx);
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("APEX / 128 × 128").strong());
            ui.label(RichText::new("●  ПРЕДПРОСМОТР").size(11.0).color(TEAL));
        });
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Установить время штатных часов дока с компьютера"),
            )
            .clicked()
        {
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = ajazz_studio::dock::sync_clock()
                    .map(|receipt| format!("{} · {}", receipt.local_time, receipt.verification))
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Done(result));
            });
        }
        ui.columns(2, |cols| {
            let img = screen::render(
                &self.screen,
                chrono::Local::now(),
                self.media.as_ref().map(|m| {
                    m.at_fixed_delay((self.media_time * 1000.0) as u64, self.animation_delay_ms)
                }),
                self.font.as_ref(),
            );
            let color = egui::ColorImage::from_rgba_unmultiplied([128, 128], img.as_raw());
            if let Some(texture) = self.screen_texture.as_mut() {
                texture.set(color, egui::TextureOptions::NEAREST);
            } else {
                self.screen_texture =
                    Some(ctx.load_texture("dock_screen", color, egui::TextureOptions::NEAREST));
            }
            cols[0].vertical_centered(|ui| {
                ui.add_space(15.0);
                egui::Frame::new()
                    .fill(Color32::from_rgb(5, 8, 13))
                    .inner_margin(16.0)
                    .corner_radius(18.0)
                    .show(ui, |ui| {
                        ui.image((
                            self.screen_texture.as_ref().unwrap().id(),
                            Vec2::splat(300.0),
                        ));
                    });
                ui.label(
                    RichText::new("Часы используют время компьютера")
                        .size(12.0)
                        .color(MUTED),
                );
            });
            let ui = &mut cols[1];
            ui.label("Макет экрана");
            ui.text_edit_singleline(&mut self.screen.title);
            ui.checkbox(&mut self.screen.show_clock, "Часы");
            ui.checkbox(&mut self.screen.show_seconds, "Секунды");
            ui.checkbox(&mut self.screen.show_date, "Дата");
            ui.checkbox(
                &mut self.screen.show_accent_line,
                "Декоративная линия снизу",
            );
            ui.add(egui::Slider::new(&mut self.screen.clock_x, 0.0..=110.0).text("Позиция X"));
            ui.add(egui::Slider::new(&mut self.screen.clock_y, 0.0..=110.0).text("Позиция Y"));
            ui.add(
                egui::Slider::new(&mut self.screen.clock_size, 10.0..=48.0).text("Размер часов"),
            );
            ui.horizontal(|ui| {
                ui.label("Цвета");
                ui.color_edit_button_srgb(&mut self.screen.background);
                ui.color_edit_button_srgb(&mut self.screen.foreground);
                ui.color_edit_button_srgb(&mut self.screen.accent);
            });
            if ui.button("Импорт PNG / JPG / GIF").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Изображение", &["png", "jpg", "jpeg", "gif"])
                    .pick_file()
            {
                match Media::load(&path) {
                    Ok(media) => {
                        self.message(format!("Медиа: {} кадров", media.frames.len()));
                        self.media = Some(media);
                        self.media_path = Some(path);
                        self.media_time = 0.0;
                    }
                    Err(e) => self.message(e.to_string()),
                }
            }
            if let Some(path) = &self.media_path {
                ui.label(
                    RichText::new(path.file_name().unwrap_or_default().to_string_lossy())
                        .size(11.0)
                        .color(MUTED),
                );
                if ui.button("Убрать медиа").clicked() {
                    self.media = None;
                    self.media_path = None;
                }
            }
            ui.add(
                egui::Slider::new(&mut self.screen.image_opacity, 0.0..=1.0)
                    .text("Видимость изображения"),
            );
            if ui.button("Экспорт PNG 128×128").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("dock-screen.png")
                    .save_file()
            {
                let result = screen::export_png(&self.screen, &path, self.media.as_ref());
                self.message(
                    result
                        .map(|()| "PNG сохранён".into())
                        .unwrap_or_else(|e| e.to_string()),
                );
            }
            if ui.button("Открыть макет JSON").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Макет", &["json"])
                    .pick_file()
            {
                match ScreenDesign::load(&path) {
                    Ok(design) => {
                        self.screen = design;
                        self.message("Макет открыт; медиа подключается отдельно");
                    }
                    Err(e) => self.message(e.to_string()),
                }
            }
            if ui.button("Сохранить макет JSON").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("dock-design.json")
                    .save_file()
            {
                let result = ajazz_studio::storage::write_json_atomic(&path, &self.screen);
                self.message(
                    result
                        .map(|()| "Макет сохранён; исходное медиа хранится отдельно".into())
                        .unwrap_or_else(|e| e.to_string()),
                );
            }
        });
        ui.add_space(18.0);
        ui.heading("Сохранить текущий кадр на док");
        ui.label("Один неподвижный кадр 128×128. Нарисованные часы сохранятся как картинка.");
        ui.add_enabled_ui(!self.busy, |ui| {
            egui::ComboBox::from_id_salt("dock_slot")
                .selected_text(
                    self.dock_slot
                        .map(|v| format!("Слот {}", v + 1))
                        .unwrap_or_else(|| "Выберите слот…".into()),
                )
                .show_ui(ui, |ui| {
                    for slot in 0..5 {
                        ui.selectable_value(
                            &mut self.dock_slot,
                            Some(slot),
                            format!("Слот {}", slot + 1),
                        );
                    }
                });
        });
        ui.colored_label(AMBER,"Изображение выбранного слота будет заменено. Штатный протокол не отдаёт старые пиксели для резервной копии.");
        if ui
            .add_enabled(
                !self.busy && self.dock_slot.is_some(),
                egui::Button::new("Загрузить текущий кадр в выбранный слот"),
            )
            .clicked()
            && let Some(slot) = self.dock_slot
        {
            let image = screen::render(
                &self.screen,
                chrono::Local::now(),
                self.media.as_ref().map(|m| {
                    m.at_fixed_delay((self.media_time * 1000.0) as u64, self.animation_delay_ms)
                }),
                self.font.as_ref(),
            );
            match ajazz_studio::dock_display::DisplayFrame::from_image(slot, &image) {
                Ok(frame) => {
                    self.busy = true;
                    self.dock_progress = Some(0.0);
                    self.message("Подготавливаю док и передаю изображение…");
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let last = std::cell::Cell::new(255u8);
                        let result = ajazz_studio::dock_display::upload(&frame, |p| {
                            let percent = (p * 100.0).round() as u8;
                            if last.replace(percent) != percent {
                                let _ = tx.send(Event::DockProgress(p));
                            }
                        })
                        .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Event::DockUpload(result));
                    });
                }
                Err(error) => self.message(format!("Некорректный кадр: {error:#}")),
            }
        }
        if let Some(progress) = self.dock_progress {
            ui.add(egui::ProgressBar::new(progress).show_percentage());
        }
        ui.add_space(16.0);
        ui.heading("Анимация на доке");
        ui.checkbox(&mut self.playing, "Воспроизводить предпросмотр");
        ui.label("Импортируйте GIF: от 2 до 222 кадров. Док использует одинаковую задержку для всех кадров.");
        ui.add_enabled(
            !self.busy,
            egui::Slider::new(&mut self.animation_delay_ms, 20..=255).text("Задержка кадра, мс"),
        );
        if let Some(media) = &self.media {
            ui.label(format!(
                "{} кадров · {:.2} с на цикл",
                media.frames.len(),
                media.frames.len() as f32 * f32::from(self.animation_delay_ms) / 1000.0
            ));
        }
        ui.colored_label(
            AMBER,
            "Загрузка заменит текущую анимацию дока. Часы и дата в кадрах будут неподвижными.",
        );
        if ui
            .add_enabled(
                !self.busy && self.media.as_ref().is_some_and(|m| m.frames.len() >= 2),
                egui::Button::new("Загрузить анимацию на док"),
            )
            .clicked()
        {
            let now = chrono::Local::now();
            let images: Vec<_> = self
                .media
                .as_ref()
                .unwrap()
                .frames
                .iter()
                .map(|frame| screen::render(&self.screen, now, Some(frame), self.font.as_ref()))
                .collect();
            match ajazz_studio::dock_display::DisplayAnimation::from_images(
                &images,
                self.animation_delay_ms,
            ) {
                Ok(animation) => {
                    self.busy = true;
                    self.dock_progress = Some(0.0);
                    self.message("Передаю анимацию на док…");
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let last = std::cell::Cell::new(255u8);
                        let result =
                            ajazz_studio::dock_display::upload_animation(&animation, |p| {
                                let percent = (p * 100.0).round() as u8;
                                if last.replace(percent) != percent {
                                    let _ = tx.send(Event::DockProgress(p));
                                }
                            })
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Event::DockUpload(result));
                    });
                }
                Err(error) => self.message(format!("Некорректная анимация: {error:#}")),
            }
        }
        ui.label(
            RichText::new(
                "Постоянные виджеты без запущенного приложения потребуют исследования прошивки.",
            )
            .color(MUTED),
        );
    }
    fn devices_page(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Прочитать версию дока через HID"),
            )
            .clicked()
        {
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Event::DockInfo(
                    ajazz_studio::dock::read_info().map_err(|e| format!("{e:#}")),
                ));
            });
        }
        if let Some(info) = &self.dock_info {
            ui.label(
                RichText::new(format!(
                    "Док: {} · raw flash version {:04X} · {}",
                    info.label(),
                    info.flash_version,
                    info.captured_utc
                ))
                .color(TEAL),
            );
            ui.label(
                RichText::new(
                    "Поле flash version не определяет объём памяти или совместимость прошивки.",
                )
                .size(12.0)
                .color(MUTED),
            );
        }
        ui.add_space(10.0);
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Прочитать батарею и настройки мыши через HID"),
            )
            .clicked()
        {
            self.read_mouse();
        }
        ui.label(RichText::new("Управление периферией работает напрямую через HID. При сне мыши нажмите её физическую кнопку.").size(12.0).color(MUTED));
        if ui.button("Обновить список USB").clicked() {
            match hardware::scan() {
                Ok(e) => {
                    self.endpoints = e;
                    self.message("Список HID обновлён без отправки команд");
                }
                Err(e) => self.message(e.to_string()),
            }
        }
        for e in self.endpoints.iter().filter(|e| e.usage_page >= 0xff00) {
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(16.0)
                .corner_radius(10.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(&e.role).strong());
                    ui.label(&e.product);
                    ui.label(
                        RichText::new(format!(
                            "{:04X}:{:04X}  ·  interface {}  ·  usage {:04X}:{:04X}",
                            e.vid, e.pid, e.interface, e.usage_page, e.usage
                        ))
                        .monospace()
                        .color(MUTED),
                    );
                });
        }
        ui.add_space(18.0);
        ui.label("Наличие USB-приёмника не подтверждает связь с устройством по радио.");
    }
    fn research_page(&mut self, ui: &mut egui::Ui) {
        self.updates_page(ui);
        ui.add_space(16.0);
        ui.collapsing("Диагностика и анализ файлов", |ui| {
            self.research_details(ui);
        });
    }
    fn research_details(&mut self, ui: &mut egui::Ui) {
        ui.heading("AJ159 APEX · версии мыши и дока");
        ui.label("Чтение через запущенную службу AJAZZ. Обновление и смена режима не выполняются.");
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Прочитать ID и версии комплекта"),
            )
            .clicked()
        {
            self.mouse_firmware_info = None;
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Event::MouseFirmwareInfo(
                    ajazz_studio::mouse_firmware::read_via_vendor_service()
                        .map_err(|e| format!("{e:#}")),
                ));
            });
        }
        if let Some(info) = &self.mouse_firmware_info {
            ui.label(format!(
                "Экран дока: OLEDV{:X} · flash version 0x{:04X}",
                info.dock_oled_version, info.dock_flash_version
            ));
            ui.small(format!(
                "Прочитано: {} · {}",
                info.captured_utc, info.source
            ));
            if info.mouse_id_verified {
                ui.label("ID1755 подтверждён ответом мыши");
            }
            if let Some(version) = info.mouse_version {
                ui.label(format!("Версия мыши: raw 0x{version:04X}"));
            }
            if let Some(error) = &info.mouse_read_error {
                ui.colored_label(AMBER, error);
            }
            ui.small(
                "Версия flash не является размером памяти. Эти ответы не содержат код firmware.",
            );
            if ui.button("Сохранить отчёт версий…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("aj159-firmware-identity.json")
                    .save_file()
                && let Err(error) = ajazz_studio::storage::write_json_atomic(&path, info)
            {
                self.message(format!("Ошибка сохранения: {error:#}"));
            }
        }
        ui.add_space(12.0);
        ui.small("AJ159 APEX ID1755/док ID1759 отличаются от ID2349/2350. Имя APEX и похожий USB PID не подтверждают совместимость.");
        ui.separator();
        ui.label("Прошивки · только анализ файла");
        if ui.button("Открыть BIN / HEX / EXE для анализа").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_file()
        {
            match firmware::inspect(&path) {
                Ok(report) => self.firmware_text = serde_json::to_string_pretty(&report).unwrap(),
                Err(e) => self.message(e.to_string()),
            }
        }
        if !self.firmware_text.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("firmware")
                .max_height(270.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.firmware_text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
        }
        ui.add_space(12.0);
        ui.label(
            "Исходные образы, хеши кандидатов и отчёты проверок сохранены в проекте. Аппаратное восстановление пока не испытано.",
        );
        ui.add_space(18.0);
        ui.separator();
        ui.label("Журнал");
        for line in self.logs.iter().rev().take(12) {
            ui.label(RichText::new(line).size(13.0).color(MUTED));
        }
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.advance_update();
        while let Some(action) = self.tray.as_ref().and_then(|t| t.action()) {
            match action {
                crate::tray::Action::Open => {
                    self.hidden = false;
                    self.hide_requested = false;
                    self.window_hidden.store(false, Ordering::Relaxed);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                crate::tray::Action::Exit => self.exit_requested = true,
            }
        }
        let closing = ctx.input(|i| i.viewport().close_requested());
        let minimized = ctx.input(|i| i.viewport().minimized == Some(true));
        if !self.exit_requested && (closing || minimized) && self.tray.is_some() {
            self.hide_to_tray(ctx);
        } else if closing {
            self.exit_requested = true;
        }
        if let Some(changed) = self.lighting_pending
            && self.lighting_auto_apply
            && !self.busy
            && !ctx.input(|i| i.pointer.any_down())
            && (self.exit_requested
                || self.hide_requested
                || changed.elapsed() >= Duration::from_millis(400))
        {
            self.apply_builtin_lighting();
        }
        self.flush_encoder_change(ctx);
        self.flush_automatic_changes(ctx);
        if self.exit_requested {
            // An unplugged device must not trap Quit. Local drafts survive.
            if self.page_status.is_none() {
                self.dock_change = None;
            }
            if let Some(commands) = &self.screen_commands {
                let _ = commands.send(dock_telemetry::Command::Stop);
            }
            if let Some(cancel) = &self.playback_cancel {
                cancel.store(true, Ordering::Relaxed);
            }
            if !self.busy
                && self.screen_commands.is_none()
                && !self.update_running
                && self.update_pending.is_none()
                && self.update_prepared.is_none()
                && self.playback_cancel.is_none()
                && !self.memory_clock_pending
                && !self.dock_save_pending
                && self.dock_change.is_none()
                && self.memory_encoder_pending.is_none()
                && (self.memory_changes.is_empty()
                    || self.playback_channel != PlaybackChannel::Usb
                    || self.memory_state.is_none())
                && (self.memory_activation.is_none() || self.memory_state.is_none())
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.hide_requested
            && !self.exit_requested
            && (!self.busy || self.playback_cancel.is_some())
            && !self.dock_save_pending
            && self.dock_change.is_none()
            && self.memory_encoder_pending.is_none()
            && !self.memory_clock_pending
            && (self.memory_changes.is_empty()
                || self.memory_state.is_none()
                || self.playback_channel != PlaybackChannel::Usb)
            && (self.memory_activation.is_none() || self.memory_state.is_none())
        {
            self.hide_requested = false;
            self.hidden = true;
            self.window_hidden.store(true, Ordering::Relaxed);
            self.save_session();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        self.poll_connections();
        self.start_screen_stream();
        if self.hidden {
            if self.last_session_save.elapsed() >= Duration::from_secs(3) {
                self.save_session();
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        let scene_before = serde_json::to_vec(&self.scene).unwrap_or_default();
        let slot_before = self.memory_editor_slot;
        let dock_before =
            serde_json::to_vec(&(&self.dock_telemetry, &self.dock_pages)).unwrap_or_default();
        let graphics_before = serde_json::to_vec(&self.graphics_options).unwrap_or_default();
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        if self.playing {
            self.media_time += dt;
            if self.memory_preview && !self.dock_surface {
                self.time = if let Some((clock, at)) = &self.memory_clock
                    && clock.selected.is_some()
                {
                    clock.frame_after(at.elapsed().as_millis() as u64) as f32 / 50.0
                } else {
                    self.time + dt
                };
            } else {
                self.time += dt;
                if self.time >= self.scene.duration {
                    self.time = 0.0;
                    self.pulses.clear();
                }
            }
        }
        self.pulses
            .retain(|p| self.time >= p.time && self.time - p.time < 2.0);
        self.sidebar(ctx);
        self.load_visible_data();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(17, 19, 24))
                    .inner_margin(22.0),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                self.header(ui);
                egui::ScrollArea::vertical()
                    .id_salt(("main_scroll", self.page))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Lighting => self.lighting_page(ui),
                        Page::Keyboard => self.keyboard_page(ui),
                        Page::Mouse => self.mouse_page(ui),
                        Page::Dock => self.dock_page(ui, ctx),
                        Page::Devices => self.devices_page(ui),
                        Page::Research => self.research_page(ui),
                        Page::Preferences => self.preferences_page(ui),
                    });
            });
        if self.memory_preview
            && !self.dock_surface
            && self.page == Page::Lighting
            && let Some(slot) = self.memory_editor_slot
            && Some(slot) == slot_before
            && serde_json::to_vec(&self.scene).unwrap_or_default() != scene_before
        {
            self.memory_drafts.insert(slot, self.scene.clone());
            self.memory_changes
                .insert(slot, (Instant::now(), self.scene.clone()));
        }
        if serde_json::to_vec(&self.graphics_options).unwrap_or_default() != graphics_before {
            self.graphics_change = Some(Instant::now());
        }
        if self
            .graphics_change
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(650))
            && let Some(commands) = &self.screen_commands
        {
            let _ = commands.send(dock_telemetry::Command::Graphics(
                self.graphics_options.clone(),
            ));
            self.graphics_change = None;
            if !self.graphics_options.fps_enabled {
                self.fps_requires_admin = false;
            }
        }
        if self.page == Page::Dock
            && self.dock_tab == DockTab::Pages
            && serde_json::to_vec(&(&self.dock_telemetry, &self.dock_pages)).unwrap_or_default()
                != dock_before
        {
            self.dock_change = Some(Instant::now());
            self.dock_save_error = None;
        }
        if self.playing || self.busy || self.page == Page::Dock {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        if self.last_session_save.elapsed() >= Duration::from_secs(3) {
            self.save_session();
        }
        ctx.request_repaint_after(Duration::from_secs(3));
        self.feedback(ctx);
        self.update_confirmation(ctx);
        if !self.hide_requested && !self.exit_requested {
            self.poll_memory_clock(ctx);
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_session();
    }
}

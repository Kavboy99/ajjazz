pub mod device_updates;
pub mod dock;
pub mod dock_composition;
pub mod dock_display;
pub mod dock_telemetry;
pub mod dock_telemetry_preview;
pub mod firmware;
pub mod graphics;
pub mod hardware;
pub mod keyboard_animation;
pub mod keyboard_calibration;
pub mod keyboard_dks;
pub mod keyboard_general;
pub mod keyboard_live;
pub mod keyboard_macros;
pub mod keyboard_memory;
pub mod keyboard_playback;
pub mod keyboard_settings;
pub mod keymap;
pub mod lighting;
pub mod lighting_collection;
pub mod link_diagnostics;
pub mod local_bridge;
pub mod metrics_agent;
pub mod model;
pub mod mouse;
pub mod mouse_buttons;
pub mod mouse_firmware;
pub mod mouse_hid;
pub mod protocol;
pub mod screen;
pub mod storage;
pub mod temperatures;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub mod dock_pages;
pub mod dock_timer;

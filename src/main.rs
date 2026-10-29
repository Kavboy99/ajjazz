#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod tray;
mod ui;
fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--metrics-agent") {
        if let (Some(address), Some(token)) = (args.get(2), args.get(3)) {
            let _ = ajazz_studio::metrics_agent::run(address, token);
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "--render-review") {
        let directory =
            std::path::Path::new(args.get(2).expect("--render-review OUTPUT_DIRECTORY"));
        ui::Studio::render_review(directory).expect("Offline UI rendering failed");
        return Ok(());
    }
    if std::env::args().any(|a| a == "--resume-fps") && !tray::wait_for_previous_exit() {
        return Ok(());
    }
    if tray::activate_existing() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size([1100.0, 740.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "AJAZZ Studio",
        options,
        Box::new(|cc| Ok(Box::new(ui::Studio::new(cc)))),
    )
}

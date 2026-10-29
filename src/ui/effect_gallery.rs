//! Visual effect selection. Browsing is local; only a selection changes the edited layer.
use super::*;

pub(super) fn open_key(id: (bool, usize)) -> egui::Id {
    egui::Id::new(("effect-gallery-open", id))
}

#[derive(Clone, Copy)]
pub(super) struct Preview {
    pub time: f32,
    pub brightness: u8,
    pub speed: u8,
    pub transfer: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn picker(
    ui: &mut egui::Ui,
    layer: &mut Layer,
    id: (bool, usize),
    time: f32,
    brightness: u8,
    speed: u8,
    transfer: bool,
    review_mode: bool,
) {
    let ctx = ui.ctx().clone();
    let key = open_key(id);
    let preview = Preview {
        time: design::preview_time(ui.ctx(), time),
        brightness,
        speed,
        transfer,
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 60.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 10.0, Color32::from_rgb(34, 39, 49));
    if response.hovered() || response.has_focus() {
        p.rect_stroke(
            rect.shrink(1.0),
            10.0,
            Stroke::new(1.0_f32, TEAL),
            StrokeKind::Inside,
        );
    }
    thumbnail(
        ui,
        Rect::from_min_size(rect.min + Vec2::splat(6.0), Vec2::splat(48.0)),
        layer,
        preview,
    );
    p.text(
        rect.min + Vec2::new(66.0, 30.0),
        egui::Align2::LEFT_CENTER,
        layer.effect.label(),
        FontId::proportional(15.0),
        Color32::WHITE,
    );
    p.text(
        rect.right_center() - Vec2::new(16.0, 0.0),
        egui::Align2::CENTER_CENTER,
        "›",
        FontId::proportional(23.0),
        MUTED,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Выбрать эффект: {}", layer.effect.label()),
        )
    });
    if review_mode {
        review::record(&ctx, "effect-picker", rect);
    }
    if response.clicked() {
        ctx.data_mut(|d| d.insert_temp(key, true));
    }
    if !ctx.data(|d| d.get_temp::<bool>(key).unwrap_or(false)) {
        return;
    }

    let mut selected = None;
    let mut close = false;
    let filter_id = key.with("filter");
    let mut filter = ctx.data(|d| d.get_temp::<usize>(filter_id).unwrap_or(0));
    let modal_id = key.with("modal");
    let modal = egui::Modal::new(modal_id)
        .area(egui::Modal::default_area(modal_id).default_size(Vec2::new(
            850.0,
            (ctx.content_rect().height() - 50.0).max(400.0),
        )))
        .frame(surface().inner_margin(24.0))
        .show(&ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 96.0).clamp(400.0, 800.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Эффекты").size(23.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Готово").clicked();
                });
            });
            ui.horizontal(|ui| {
                for (index, name) in ["Все", "Спокойные", "Динамичные", "Отклик"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut filter, index, *name);
                }
            });
            let max_height = (ctx.content_rect().height() - 240.0).clamp(220.0, 620.0);
            let columns = if ui.available_width() >= 700.0 { 4 } else { 3 };
            let card_width =
                (ui.available_width() - 12.0 * (columns - 1) as f32 - 8.0) / columns as f32;
            egui::ScrollArea::vertical()
                .id_salt(key.with("scroll"))
                .max_height(max_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let effects: Vec<_> = Effect::ALL
                        .into_iter()
                        .filter(|e| filter == 0 || category(*e) == filter)
                        .collect();
                    for row in effects.chunks(columns) {
                        ui.horizontal(|ui| {
                            for &effect in row {
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(card_width, 180.0),
                                    Sense::click(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::WidgetType::RadioButton,
                                        ui.is_enabled(),
                                        effect == layer.effect,
                                        effect.label(),
                                    )
                                });
                                if ui.is_rect_visible(rect) {
                                    let chosen = effect == layer.effect;
                                    let p = ui.painter();
                                    p.rect_filled(
                                        rect,
                                        12.0,
                                        if response.hovered() {
                                            Color32::from_rgb(38, 43, 54)
                                        } else {
                                            Color32::from_rgb(28, 32, 40)
                                        },
                                    );
                                    p.rect_stroke(
                                        rect.shrink(1.0),
                                        12.0,
                                        Stroke::new(
                                            if chosen { 2.0_f32 } else { 1.0_f32 },
                                            if chosen || response.has_focus() {
                                                TEAL
                                            } else {
                                                Color32::TRANSPARENT
                                            },
                                        ),
                                        StrokeKind::Inside,
                                    );
                                    let mut candidate = layer.clone();
                                    candidate.effect = effect;
                                    let tile = Rect::from_center_size(
                                        rect.center_top() + Vec2::new(0.0, 82.0),
                                        Vec2::splat(148.0_f32.min(card_width - 16.0)),
                                    );
                                    thumbnail(ui, tile, &candidate, preview);
                                    let mut job = egui::text::LayoutJob::simple_singleline(
                                        effect.label().into(),
                                        FontId::proportional(13.0),
                                        Color32::WHITE,
                                    );
                                    job.wrap.max_width = card_width - 12.0;
                                    job.wrap.max_rows = 1;
                                    let text = p.layout_job(job);
                                    p.galley(
                                        Pos2::new(
                                            rect.center().x - text.size().x / 2.0,
                                            rect.bottom() - 25.0,
                                        ),
                                        text,
                                        Color32::WHITE,
                                    );
                                    if review_mode {
                                        review::record(
                                            &ctx,
                                            &format!("gallery-{:?}", effect),
                                            rect,
                                        );
                                    }
                                }
                                if response.on_hover_text(description(effect)).clicked() {
                                    selected = Some(effect);
                                }
                            }
                        });
                    }
                });
        });
    if review_mode {
        review::record(&ctx, "effect-gallery", modal.response.rect);
    }
    ctx.data_mut(|d| d.insert_temp(filter_id, filter));
    if let Some(effect) = selected {
        layer.effect = effect;
    }
    if close || selected.is_some() || modal.should_close() {
        ctx.data_mut(|d| d.insert_temp(key, false));
    }
    ctx.request_repaint_after(Duration::from_millis(33));
}

fn thumbnail(ui: &egui::Ui, rect: Rect, layer: &Layer, preview: Preview) {
    let mut layer = layer.clone();
    layer.keys.clear();
    layer.enabled = true;
    layer.opacity = 1.0;
    layer.blend = Blend::Replace;
    let scene = Scene {
        layers: vec![layer],
        brightness: 1.0,
        ..Default::default()
    };
    scene_thumbnail(ui, rect, &scene, preview);
}
pub(super) fn scene_thumbnail(ui: &egui::Ui, rect: Rect, scene: &Scene, preview: Preview) {
    let pulses = demo_pulses(preview.time);
    let colors = ajazz_studio::keyboard_memory::device_preview(
        scene,
        preview.time,
        &pulses,
        preview.brightness,
        preview.speed,
        preview.transfer,
    )
    .unwrap_or_else(|_| {
        lighting::render(
            &scene.layers,
            &lighting::keyboard_leds(),
            preview.time,
            1.0,
            &pulses,
        )
    });
    keyboard_art::miniature(ui, rect, &colors);
}

pub(super) fn demo_pulses(time: f32) -> [Pulse; 3] {
    [(0.25, 0.4, 0.0), (0.65, 0.6, 0.3), (0.45, 0.2, 0.6)].map(|(x, y, phase)| Pulse {
        x,
        y,
        time: ((time - phase) / 0.9).floor() * 0.9 + phase,
    })
}

fn category(effect: Effect) -> usize {
    match effect {
        Effect::Reactive => 3,
        Effect::Solid
        | Effect::Gradient
        | Effect::Breathing
        | Effect::Aurora
        | Effect::Nebula
        | Effect::Embers
        | Effect::Fireflies => 1,
        _ => 2,
    }
}
fn description(effect: Effect) -> &'static str {
    match effect {
        Effect::Solid => "Один цвет на всех клавишах",
        Effect::Gradient => "Плавный переход между двумя цветами",
        Effect::Wave => "Цветовая волна проходит по клавиатуре",
        Effect::Breathing => "Свет плавно набирает яркость и затухает",
        Effect::Ripple => "Круги расходятся от центра",
        Effect::Sparkle => "Короткие вспышки на случайных клавишах",
        Effect::Reactive => "Световой отклик на нажатие. В миниатюре — пример нажатия",
        Effect::Aurora => "Мягкие переливы полярного сияния",
        Effect::Plasma => "Подвижные цветовые поля",
        Effect::Comet => "Светящиеся кометы с затухающим следом",
        Effect::DigitalRain => "Падающие потоки света",
        Effect::Embers => "Медленное тление углей",
        Effect::Orbit => "Свет вращается вокруг центра",
        Effect::Visor => "Полоса света сканирует клавиатуру",
        Effect::TwinWave => "Две волны движутся навстречу",
        Effect::Checkerboard => "Ритмичный шахматный узор",
        Effect::Fireflies => "Мягкое мерцание светлячков",
        Effect::Raindrops => "Круги от капель на воде",
        Effect::Lightning => "Короткие вспышки молнии",
        Effect::Heartbeat => "Двойные импульсы сердцебиения",
        Effect::Nebula => "Медленное движение космической туманности",
        Effect::Mosaic => "Переливы цветной мозаики",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, layer: &mut Layer, events: Vec<egui::Event>) {
        ctx.data_mut(|d| d.remove::<Vec<(String, Rect)>>(egui::Id::new("offline-layout-rects")));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 740.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    picker(ui, layer, (false, 0), 1.2, 255, 128, true, true)
                });
            },
        );
    }
    fn rect(ctx: &egui::Context, name: &str) -> Rect {
        ctx.data(|d| d.get_temp::<Vec<(String, Rect)>>(egui::Id::new("offline-layout-rects")))
            .unwrap()
            .iter()
            .find(|(n, _)| n == name)
            .unwrap()
            .1
    }
    fn click(ctx: &egui::Context, layer: &mut Layer, pos: Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                layer,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn gallery_browsing_escape_and_selection_preserve_other_layer_settings() {
        let ctx = egui::Context::default();
        let mut layer = Layer {
            effect: Effect::Solid,
            keys: BTreeSet::from([1, 4]),
            opacity: 0.37,
            blend: Blend::Multiply,
            colors: [[120, 30, 60], [20, 100, 180]],
            ..Default::default()
        };
        let original = serde_json::to_value(&layer).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut layer, vec![]);
        }
        let trigger = rect(&ctx, "effect-picker").center();
        click(&ctx, &mut layer, trigger);
        for _ in 0..3 {
            frame(&ctx, &mut layer, vec![]);
        }
        let plasma = rect(&ctx, "gallery-Plasma").center();
        frame(&ctx, &mut layer, vec![egui::Event::PointerMoved(plasma)]);
        assert_eq!(
            serde_json::to_value(&layer).unwrap(),
            original,
            "Browsing must not apply an effect"
        );
        frame(
            &ctx,
            &mut layer,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!ctx.data(|d| d.get_temp::<bool>(open_key((false, 0))).unwrap()));
        assert_eq!(serde_json::to_value(&layer).unwrap(), original);
        frame(&ctx, &mut layer, vec![]);
        click(&ctx, &mut layer, trigger);
        for _ in 0..3 {
            frame(&ctx, &mut layer, vec![]);
        }
        let plasma = rect(&ctx, "gallery-Plasma").center();
        click(&ctx, &mut layer, plasma);
        assert_eq!(layer.effect, Effect::Plasma);
        let mut expected = original;
        expected["effect"] = serde_json::json!("Plasma");
        assert_eq!(
            serde_json::to_value(&layer).unwrap(),
            expected,
            "Changing type must preserve palette, key mask, and parameters"
        );
        assert!(!ctx.data(|d| d.get_temp::<bool>(open_key((false, 0))).unwrap()));
    }
}

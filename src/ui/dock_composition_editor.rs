use super::*;
pub(super) fn open_key() -> egui::Id {
    egui::Id::new("dock-composition-effects-open")
}
pub(super) fn editor(
    ui: &mut egui::Ui,
    composition: &mut ajazz_studio::dock_composition::Composition,
    config: &dock_telemetry::Config,
    sample: Option<&dock_telemetry::Sample>,
) {
    use ajazz_studio::dock_composition::{Backdrop, Metric};
    ui.collapsing("Готовые композиции", |ui| {
        ui.horizontal_wrapped(|ui| {
            for (name, preset) in ajazz_studio::dock_composition::Composition::presets() {
                if ui.button(name).clicked() {
                    *composition = preset;
                }
            }
        });
    });
    let mut move_up = None;
    let mut remove = None;
    let count = composition.count();
    let columns = if count == 4 && ui.available_width() >= 540.0 {
        2
    } else {
        1
    };
    egui::Grid::new("composition-fields")
        .num_columns(columns)
        .spacing(Vec2::new(14.0, 8.0))
        .show(ui, |ui| {
            for i in 0..count {
                ui.horizontal(|ui| {
                    if count == 3 && i == 0 {
                        ui.label("Главный");
                    }
                    egui::ComboBox::from_id_salt(("composition-metric", i))
                        .width(210.0)
                        .selected_text(composition.metrics[i].label())
                        .show_ui(ui, |ui| {
                            for m in Metric::ALL.into_iter().filter(|m| *m != Metric::None) {
                                let duplicate =
                                    composition.metrics.contains(&m) && composition.metrics[i] != m;
                                if ui
                                    .add_enabled(
                                        !duplicate,
                                        egui::Button::new(m.label())
                                            .selected(composition.metrics[i] == m),
                                    )
                                    .clicked()
                                {
                                    composition.metrics[i] = m;
                                    ui.close();
                                }
                            }
                        });
                    ui.menu_button("•••", |ui| {
                        if i > 0 && ui.button("Сделать выше").clicked() {
                            move_up = Some(i);
                            ui.close();
                        }
                        if count > 1 && ui.button("Убрать показатель").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                });
                if i % columns == columns - 1 {
                    ui.end_row();
                }
            }
        });
    if let Some(i) = move_up {
        composition.metrics.swap(i, i - 1);
    }
    if let Some(i) = remove {
        composition.metrics[i] = Metric::None;
        composition.normalize();
    }
    if count < 4
        && ui.small_button("+ Показатель").clicked()
        && let Some(m) = Metric::ALL
            .into_iter()
            .find(|m| *m != Metric::None && !composition.metrics.contains(m))
    {
        composition.metrics[count] = m;
    }
    ui.add_space(6.0);
    if composition
        .metrics
        .iter()
        .any(|m| (1..=4).contains(&(*m as u8)))
    {
        ui.checkbox(&mut composition.bars, "Полосы загрузки");
    }
    let key = open_key();
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(key).unwrap_or(false));
    if ui
        .button(format!("Оформление · {}  ›", composition.backdrop.label()))
        .clicked()
    {
        open = true;
    }
    if !open {
        return;
    }
    let now = chrono::Local::now();
    let stamp = if design::preview_phase(ui.ctx()).is_some() {
        now.timestamp()
    } else {
        now.timestamp_millis() * 9 / 1000
    };
    let modal = egui::Modal::new(key.with("modal")).show(ui.ctx(), |ui| {
        ui.heading("Оформление экрана");
        ui.add_space(12.0);
        egui::Grid::new("composition-effects")
            .num_columns(3)
            .spacing(Vec2::splat(12.0))
            .show(ui, |ui| {
                for (i, effect) in Backdrop::ALL.into_iter().enumerate() {
                    let id = egui::Id::new(("composition-effect-texture", i));
                    let cached = ui
                        .ctx()
                        .data(|d| d.get_temp::<(i64, egui::TextureHandle)>(id));
                    let texture = if let Some((_, t)) = cached.filter(|(at, _)| *at == stamp) {
                        t
                    } else {
                        let mut c = composition.clone();
                        c.backdrop = effect;
                        let img = ajazz_studio::dock_telemetry_preview::render_composition(
                            config,
                            &c,
                            sample,
                            now,
                            design::preview_phase(ui.ctx()),
                        );
                        let t = ui.ctx().load_texture(
                            format!("composition-effect-{i}"),
                            egui::ColorImage::from_rgba_unmultiplied([128, 128], img.as_raw()),
                            egui::TextureOptions::NEAREST,
                        );
                        ui.ctx().data_mut(|d| d.insert_temp(id, (stamp, t.clone())));
                        t
                    };
                    let selected = composition.backdrop == effect;
                    let card = egui::Frame::new()
                        .stroke(Stroke::new(
                            1.5_f32,
                            if selected {
                                TEAL
                            } else {
                                Color32::from_rgb(46, 57, 70)
                            },
                        ))
                        .corner_radius(10.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                let hit = ui
                                    .add(egui::Image::new((texture.id(), Vec2::splat(96.0))))
                                    .interact(Sense::click());
                                let label =
                                    ui.add(egui::Label::new(effect.label()).sense(Sense::click()));
                                hit.clicked() || label.clicked()
                            })
                            .inner
                        });
                    if card.inner {
                        composition.backdrop = effect;
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(12.0);
        if ui.button("Готово").clicked() {
            open = false;
        }
    });
    if modal.should_close() {
        open = false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, open));
    ui.ctx().request_repaint_after(Duration::from_millis(111));
}

use super::*;

impl Studio {
    pub(super) fn mouse_page(&mut self, ui: &mut egui::Ui) {
        design::tabs(ui, |ui| {
            ui.selectable_value(&mut self.mouse_buttons_tab, false, "Параметры");
            ui.selectable_value(&mut self.mouse_buttons_tab, true, "Кнопки");
        });
        ui.add_space(8.0);
        if self.mouse_buttons_tab {
            self.mouse_buttons_page(ui);
            return;
        }
        let Some(snapshot) = self.mouse_snapshot.clone() else {
            surface().show(ui, |ui| {
                ui.strong(if self.busy {
                    "Чтение параметров…"
                } else {
                    "Подключите мышь"
                });
            });
            return;
        };
        let Some(settings) = &mut self.mouse_settings else {
            return;
        };
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.columns(2, |columns| {
                surface().show(&mut columns[0], |ui| {
                    ui.label(RichText::new("Отклик").size(19.0).strong());
                    ui.label("Частота опроса");
                    egui::ComboBox::from_id_salt("mouse_rate")
                        .width(ui.available_width())
                        .selected_text(format!("{} Гц", settings.options.polling_hz().unwrap_or(0)))
                        .show_ui(ui, |ui| {
                            for (code, rate) in [
                                (8, 125),
                                (4, 250),
                                (2, 500),
                                (1, 1000),
                                (132, 2000),
                                (130, 4000),
                                (129, 8000),
                            ] {
                                ui.selectable_value(
                                    &mut settings.options.polling_code,
                                    code,
                                    format!("{rate} Гц"),
                                );
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Задержка клика")
                            .on_hover_text("Защита от случайных повторных кликов (debounce)");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(
                                egui::DragValue::new(&mut settings.options.debounce_ms)
                                    .range(2..=10)
                                    .suffix(" мс"),
                            );
                        });
                    });
                    ui.label("Высота отрыва");
                    egui::ComboBox::from_id_salt("mouse_lod")
                        .width(ui.available_width())
                        .selected_text(
                            ["Низкая", "Средняя", "Высокая"]
                                .get(settings.options.lift_off as usize)
                                .copied()
                                .unwrap_or("Неизвестна"),
                        )
                        .show_ui(ui, |ui| {
                            for (code, label) in [(0, "Низкая"), (1, "Средняя"), (2, "Высокая")]
                            {
                                ui.selectable_value(&mut settings.options.lift_off, code, label);
                            }
                        });
                    ui.add_space(4.0);
                    ui.collapsing("Дополнительно", |ui| {
                        let mut snap = settings.options.angle_snap != 0;
                        if ui.checkbox(&mut snap, "Выпрямление линий").changed() {
                            settings.options.angle_snap = u8::from(snap);
                        }
                        if let Some(sleep) = &mut settings.sleep {
                            ui.horizontal(|ui| {
                                ui.label("Переход в сон");
                                ui.add(
                                    egui::DragValue::new(&mut sleep.radio_seconds)
                                        .range(0..=3600)
                                        .speed(5.0)
                                        .suffix(" с"),
                                );
                            });
                            ui.label(
                                RichText::new("0 — оставаться активной")
                                    .size(13.0)
                                    .color(MUTED),
                            );
                        }
                    });
                });
                surface().show(&mut columns[1], |ui| {
                    ui.label(RichText::new("Чувствительность").size(19.0).strong());
                    ui.label(
                        RichText::new("Выберите активную ступень DPI")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.spacing_mut().item_spacing.y = 8.0;
                    egui::Grid::new("mouse-dpi-stages")
                        .spacing(Vec2::new(16.0, 8.0))
                        .show(ui, |ui| {
                            for i in 0..usize::from(settings.dpi.count) {
                                ui.radio_value(
                                    &mut settings.dpi.current_index,
                                    i as u8,
                                    format!("{}", i + 1),
                                );
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut settings.dpi.x[i])
                                            .range(0..=42000)
                                            .speed(50.0)
                                            .suffix(" DPI"),
                                    )
                                    .changed()
                                {
                                    settings.dpi.y[i] = settings.dpi.x[i];
                                }
                                ui.color_edit_button_srgb(&mut settings.dpi.colors[i]);
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Шаг 50 DPI · 0 отключает ступень")
                            .size(13.0)
                            .color(MUTED),
                    );
                });
            });
        });
        let validation = snapshot.target_with_sleep(settings);
        let changed = validation
            .as_ref()
            .is_ok_and(|target| !target.equivalent(&snapshot));
        if let Err(error) = validation {
            ui.colored_label(AMBER, error.to_string());
        }
        ui.add_space(12.0);
        let save = ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && changed,
                    egui::Button::new(RichText::new("Сохранить изменения").color(Color32::WHITE))
                        .fill(PRIMARY)
                        .min_size(Vec2::new(180.0, 38.0)),
                )
                .clicked()
            {
                self.apply_mouse();
            }
            if changed
                && ui
                    .add_enabled(!self.busy, egui::Button::new("Отменить правки"))
                    .clicked()
            {
                self.mouse_settings = Some(MouseSettings::from(&snapshot));
            }
            ui.label(
                RichText::new(if changed {
                    "Есть изменения"
                } else {
                    "Настройки сохранены"
                })
                .size(13.0)
                .color(MUTED),
            );
        });
        if self.review_mode {
            review::record(ui.ctx(), "mouse-save", save.response.rect);
        }
    }

    pub(super) fn mouse_editor_actions(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if ui
            .add_enabled(
                self.mouse_settings.is_some(),
                egui::Button::new("Экспортировать параметры…"),
            )
            .clicked()
            && let (Some(settings), Some(path)) = (
                self.mouse_settings.as_ref(),
                rfd::FileDialog::new()
                    .set_file_name("aj159-apex-settings.json")
                    .save_file(),
            )
        {
            let result = serde_json::to_vec_pretty(settings)
                .map_err(anyhow::Error::from)
                .and_then(|data| Ok(std::fs::write(path, data)?));
            self.message(
                result
                    .map(|()| "Параметры сохранены".into())
                    .unwrap_or_else(|e| e.to_string()),
            );
            ui.close();
        }
        if ui
            .add_enabled(
                !self.busy && self.mouse_snapshot.is_some(),
                egui::Button::new("Импортировать параметры…"),
            )
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Mouse settings", &["json"])
                .pick_file()
        {
            let result = ajazz_studio::storage::read_json::<MouseSettings>(&path, 16384).and_then(
                |loaded| {
                    self.mouse_snapshot
                        .as_ref()
                        .unwrap()
                        .target_with_sleep(&loaded)?;
                    Ok(loaded)
                },
            );
            match result {
                Ok(loaded) => {
                    self.mouse_settings = Some(loaded);
                    self.message("Параметры открыты для просмотра. Нажмите «Сохранить изменения», чтобы записать их.");
                }
                Err(e) => self.message(format!("Не удалось открыть параметры: {e:#}")),
            }
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                !self.busy && self.mouse_backup.is_some(),
                egui::Button::new("Отменить последнюю запись"),
            )
            .clicked()
            && let Some(path) = self.mouse_backup.clone()
        {
            self.restore_mouse(path);
            ui.close();
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Восстановить из копии…"))
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Mouse backup", &["json"])
                .pick_file()
        {
            self.restore_mouse(path);
            ui.close();
        }
    }
}

//! Keyboard selection and the corresponding inspector share one viewport.
use super::*;

fn travel(ui: &mut egui::Ui, label: &str, value: &mut f32, min: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::DragValue::new(value)
                    .range(min..=3.3)
                    .speed(0.01)
                    .fixed_decimals(2)
                    .suffix(" мм"),
            );
        });
    });
    ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(140.0);
    ui.add(
        egui::Slider::new(value, min..=3.3)
            .step_by(0.01)
            .show_value(false),
    );
}

impl Studio {
    pub(super) fn keyboard_navigation(&mut self, ui: &mut egui::Ui) {
        design::tabs(ui, |ui| {
            ui.selectable_value(&mut self.keyboard_tab, KeyboardTab::Hall, "Отклик");
            ui.selectable_value(&mut self.keyboard_tab, KeyboardTab::Keymap, "Назначения");
            ui.selectable_value(&mut self.keyboard_tab, KeyboardTab::Encoder, "Энкодер");
            let (extra, label) = match self.keyboard_tab {
                KeyboardTab::General => (true, "Общие настройки"),
                KeyboardTab::Macros => (true, "Макросы"),
                KeyboardTab::Calibration => (true, "Калибровка"),
                KeyboardTab::Test => (true, "Проверка клавиш"),
                _ => (false, "Дополнительно"),
            };
            ui.menu_button(
                RichText::new(label).color(if extra { TEAL } else { MUTED }),
                |ui| {
                    for (tab, label) in [
                        (KeyboardTab::General, "Общие настройки"),
                        (KeyboardTab::Macros, "Макросы"),
                        (KeyboardTab::Calibration, "Калибровка"),
                        (KeyboardTab::Test, "Проверка клавиш"),
                    ] {
                        if ui
                            .selectable_value(&mut self.keyboard_tab, tab, label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                },
            );
        });
    }
    pub(super) fn hall_page(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        if width >= 780.0 {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(width * 0.59 - 12.0, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| self.hall_selection_pane(ui),
                );
                ui.allocate_ui_with_layout(
                    Vec2::new(width * 0.41 - 12.0, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| self.hall_inspector(ui),
                );
            });
        } else {
            self.hall_selection_pane(ui);
            ui.add_space(16.0);
            self.hall_inspector(ui);
        }
    }
    fn hall_selection_pane(&mut self, ui: &mut egui::Ui) {
        self.keyboard_canvas(ui);
        self.groups(ui);
        ui.add_space(8.0);
        ui.label(
            RichText::new("Выберите клавиши на схеме. Настройки справа применяются к выделению.")
                .size(13.0)
                .color(MUTED),
        );
    }
    fn hall_inspector(&mut self, ui: &mut egui::Ui) {
        if self.hall_selection.as_ref() != Some(&self.selected) {
            if let Some(snapshot) = &self.snapshot
                && let Some(id) = self.selected.first()
                && let Some(raw) = snapshot.blocks.get("rapid_trigger")
                && let Some(record) = raw.get(usize::from(*id) * 8..usize::from(*id) * 8 + 8)
                && let Ok(mut k) = HallKey::decode(record, snapshot.info.rt_precision)
            {
                if k.actuation == 0.0 {
                    k.actuation = 1.2;
                }
                if k.press == 0.0 {
                    k.press = 0.1;
                }
                if k.release == 0.0 {
                    k.release = 0.1;
                }
                self.hall = k;
            }
            self.hall_selection = Some(self.selected.clone());
        }
        let enabled = !self.busy && self.snapshot.is_some() && !self.selected.is_empty();
        surface().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Отклик клавиш").size(19.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("•••", |ui| {
                        if ui
                            .add_enabled(
                                !self.busy && self.hall_backup.is_some(),
                                egui::Button::new("Отменить последнюю запись"),
                            )
                            .clicked()
                            && let Some(path) = self.hall_backup.clone()
                        {
                            self.restore_hall(path);
                            ui.close();
                        }
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Восстановить из копии…"))
                            .clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("Keyboard backup", &["json"])
                                .pick_file()
                        {
                            self.restore_hall(path);
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Копии настроек");
                });
            });
            if self.selected.is_empty() {
                ui.label(
                    RichText::new("Выберите клавиши на схеме")
                        .size(13.0)
                        .color(MUTED),
                );
            }
            ui.add_space(8.0);
            ui.add_enabled_ui(enabled, |ui| {
                travel(ui, "Срабатывание", &mut self.hall.actuation, 0.1);
                ui.add_space(8.0);
                ui.checkbox(&mut self.hall.rapid, "Rapid Trigger")
                    .on_hover_text("Повторное срабатывание без полного отпускания клавиши");
                if self.hall.rapid {
                    travel(ui, "Повторное нажатие", &mut self.hall.press, 0.01);
                    travel(ui, "Отпускание", &mut self.hall.release, 0.01);
                    ui.collapsing("Поведение Rapid Trigger", |ui| {
                        ui.checkbox(&mut self.hall.whole_travel, "По всему ходу");
                        ui.checkbox(&mut self.hall.continuous, "Непрерывный");
                    });
                }
                ui.collapsing("Тип переключателя", |ui| {
                    egui::ComboBox::from_id_salt("hall_axis")
                        .width(ui.available_width())
                        .selected_text(
                            *["Тип по умолчанию", "Штатный свитч", "Jade", "King"]
                                .get(usize::from(self.hall.axis))
                                .unwrap_or(&"Неизвестный тип"),
                        )
                        .show_ui(ui, |ui| {
                            for (i, label) in ["Тип по умолчанию", "Штатный свитч", "Jade", "King"]
                                .iter()
                                .enumerate()
                            {
                                ui.selectable_value(&mut self.hall.axis, i as u8, *label);
                            }
                        });
                });
            });
            ui.add_space(8.0);
            let save = ui.add_enabled(
                enabled,
                egui::Button::new(
                    RichText::new(format!("Сохранить для {} клавиш", self.selected.len()))
                        .color(Color32::WHITE),
                )
                .fill(PRIMARY)
                .min_size(Vec2::new(ui.available_width(), 38.0)),
            );
            if self.review_mode {
                review::record(ui.ctx(), "hall-save", save.rect);
            }
            if save.clicked() {
                self.apply_hall();
            }
            if self.snapshot.is_none() {
                ui.label(
                    RichText::new("Настройки появятся после подключения клавиатуры.")
                        .size(13.0)
                        .color(MUTED),
                );
            }
            if let Some(snapshot) = &self.snapshot {
                ui.collapsing("Текущие значения", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("hall_read_values")
                        .max_height(160.0)
                        .show(ui, |ui| {
                            for key in keyboard_rows()
                                .into_iter()
                                .flatten()
                                .filter(|k| self.selected.contains(&k.id))
                            {
                                if let Some(raw) =
                                    snapshot.blocks.get("rapid_trigger").and_then(|b| {
                                        b.get(usize::from(key.id) * 8..usize::from(key.id) * 8 + 8)
                                    })
                                    && let Ok(k) = HallKey::decode(raw, snapshot.info.rt_precision)
                                {
                                    let travel = if k.actuation == 0.0 { 1.2 } else { k.actuation };
                                    ui.label(format!(
                                        "{} · {travel:.2} мм · {}",
                                        key.label,
                                        if k.rapid {
                                            "RT включён"
                                        } else {
                                            "RT выключен"
                                        }
                                    ));
                                }
                            }
                        });
                    ui.label(
                        RichText::new(
                            "При выборе группы редактор показывает настройки первой клавиши.",
                        )
                        .size(13.0)
                        .color(MUTED),
                    );
                });
            }
        });
    }
}

use super::*;
use ajazz_studio::keyboard_memory::{self as memory, Action, Operation};

impl Studio {
    pub(super) fn memory_operation(&mut self, operation: Operation) {
        if self.busy || self.playback_cancel.is_some() {
            return;
        }
        self.memory_open_after_read =
            matches!(operation, Operation::Read) && self.memory_editor_slot.is_none();
        self.memory_deleting = if let Operation::Delete(slot) = &operation {
            Some(*slot)
        } else {
            None
        };
        self.memory_publishing =
            if let Operation::Publish(slot, _) | Operation::Upload(slot, _) = &operation {
                Some(*slot)
            } else {
                None
            };
        self.memory_notice = match &operation {
            Operation::Read => "Режимы и назначения обновлены",
            Operation::Publish(..) => "Режим сохранён на клавиатуре и включён",
            Operation::Upload(..) => "Изменения сохранены на клавиатуре",
            Operation::Activate(_) => "Выбор сохранён на клавиатуре",
            Operation::Encoder(..) | Operation::Configure(_) => "Назначения энкодера сохранены",
            Operation::Delete(_) => "Режим удалён. Остальные режимы сохранены",
            _ => "Настройки сохранены на клавиатуре",
        }
        .into();
        self.memory_generation = self.memory_generation.wrapping_add(1);
        self.busy = true;
        self.lighting_pending = None;
        self.memory_error = None;
        self.memory_clock_failed = false;
        self.message(if matches!(operation, Operation::Read) {
            "Загружаю настройки клавиатуры…"
        } else {
            "Сохраняю на клавиатуре…"
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = ajazz_studio::storage::backup_directory()
                .and_then(|dir| memory::run(operation, &dir))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::KeyboardMemory(result));
        });
    }
    pub(super) fn keyboard_memory_panel(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Keyboard {
            self.encoder_settings(ui);
            return;
        }
        let enabled = self.memory_state.is_some()
            && (!self.busy)
            && self.playback_cancel.is_none()
            && self.playback_channel == PlaybackChannel::Usb;
        let was_custom = self.memory_preview;
        design::tabs(ui, |ui| {
            ui.selectable_value(&mut self.memory_preview, true, "Мои режимы");
            ui.selectable_value(&mut self.memory_preview, false, "Встроенные эффекты");
            if ui.button("Коллекция").clicked() {
                lighting_collection::open(ui.ctx());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("•••", |ui| {
                    if ui.button("Импортировать эффект…").clicked() {
                        self.open_scene();
                        ui.close();
                    }
                    if ui.button("Экспортировать эффект…").clicked() {
                        self.save_scene();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Обновить с клавиатуры"))
                        .clicked()
                    {
                        self.memory_operation(Operation::Read);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new("Запомнить яркость и скорость энкодера"),
                        )
                        .clicked()
                    {
                        self.memory_operation(Operation::Save);
                        ui.close();
                    }
                    if ui.button("Настроить энкодер").clicked() {
                        self.navigate(Page::Keyboard);
                        self.keyboard_tab = KeyboardTab::Encoder;
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            (!self.busy) && self.memory_drafts.contains_key(&8),
                            egui::Button::new("Открыть локальную сцену"),
                        )
                        .clicked()
                    {
                        if let Some(slot) = self.memory_editor_slot {
                            self.memory_drafts.insert(slot, self.scene.clone());
                        }
                        if let Some(scene) = self.memory_drafts.get(&8).cloned() {
                            self.scene = scene;
                            self.memory_editor_slot = None;
                            self.layer = 0;
                        }
                        ui.close();
                    }
                    let has_slot = self
                        .memory_state
                        .as_ref()
                        .is_some_and(|s| s.scenes[self.memory_slot].is_some());
                    ui.separator();
                    if ui
                        .add_enabled(
                            enabled && has_slot,
                            egui::Button::new("Удалить выбранный режим…"),
                        )
                        .clicked()
                    {
                        self.memory_delete = Some(self.memory_slot as u8);
                        ui.close();
                    }
                });
            });
        });
        self.memory_connection_status(ui);
        if !self.memory_preview {
            if was_custom
                && self
                    .memory_state
                    .as_ref()
                    .is_some_and(|s| s.config.selected.is_some())
                && enabled
            {
                self.memory_operation(Operation::Activate(None));
            }
            ui.add_space(8.0);
            return;
        }
        let mut chosen = None;
        egui::ScrollArea::horizontal()
            .id_salt("memory-ribbon")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut shown_empty = false;
                    for slot in 0..8 {
                        let saved = self
                            .memory_state
                            .as_ref()
                            .and_then(|s| s.scenes[slot].as_ref())
                            .and_then(|p| memory::edit_scene(p).ok());
                        let scene = if self.memory_editor_slot == Some(slot as u8) {
                            Some(self.scene.clone())
                        } else {
                            self.memory_drafts.get(&(slot as u8)).cloned().or(saved)
                        };
                        if scene.is_none() {
                            if shown_empty {
                                continue;
                            }
                            shown_empty = true;
                        }
                        let name = scene
                            .as_ref()
                            .map(|s| {
                                if s.name == "Новый эффект"
                                    || s.name
                                        .strip_prefix("Режим ")
                                        .is_some_and(|n| n.parse::<u8>().is_ok())
                                {
                                    s.layers
                                        .first()
                                        .map_or("Новый эффект", |l| l.effect.label())
                                        .into()
                                } else {
                                    s.name.clone()
                                }
                            })
                            .unwrap_or_else(|| {
                                if self.memory_state.is_none()
                                    && self.playback_channel == PlaybackChannel::Usb
                                    && self.busy
                                {
                                    "Загрузка…".into()
                                } else {
                                    "Новый эффект".into()
                                }
                            });
                        let active = self
                            .memory_state
                            .as_ref()
                            .is_some_and(|s| s.config.selected == Some(slot as u8));
                        let selected = self.memory_editor_slot == Some(slot as u8);
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(
                                148.0,
                                if ui.ctx().content_rect().height() < 800.0 {
                                    140.0
                                } else {
                                    154.0
                                },
                            ),
                            Sense::click(),
                        );
                        if self.review_mode {
                            review::record(ui.ctx(), "effect-card", rect);
                        }
                        let painter = ui.painter().clone();
                        painter.rect_filled(
                            rect,
                            14.0,
                            if response.hovered() {
                                Color32::from_rgb(38, 43, 52)
                            } else {
                                PANEL
                            },
                        );
                        painter.rect_stroke(
                            rect.shrink(1.5),
                            14.0,
                            egui::Stroke::new(
                                if selected { 2.5_f32 } else { 1.0_f32 },
                                if selected { TEAL } else { Color32::TRANSPARENT },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        let mut title = egui::text::LayoutJob::simple_singleline(
                            name,
                            FontId::proportional(14.0),
                            Color32::WHITE,
                        );
                        title.wrap.max_width = if active { 112.0 } else { 128.0 };
                        title.wrap.max_rows = 1;
                        title.wrap.break_anywhere = true;
                        let galley = painter.layout_job(title);
                        painter.galley(rect.min + Vec2::new(10.0, 10.0), galley, Color32::WHITE);
                        if active {
                            painter.circle_filled(
                                Pos2::new(rect.right() - 12.0, rect.top() + 20.0),
                                3.5,
                                TEAL,
                            );
                        }
                        let tile = Rect::from_center_size(
                            rect.center() + Vec2::new(0.0, 12.0),
                            Vec2::splat(if ui.ctx().content_rect().height() < 800.0 {
                                88.0
                            } else {
                                100.0
                            }),
                        );
                        self.memory_thumbnail(ui, tile, scene.as_ref(), active);
                        if response
                            .on_hover_text(if active {
                                "Сейчас на клавиатуре. Нажмите, чтобы настроить."
                            } else {
                                "Открыть эффект. Другие черновики сохранятся."
                            })
                            .clicked()
                            && (!self.busy || self.memory_publishing.is_some())
                            && self.memory_state.is_some()
                        {
                            if scene.is_some() {
                                chosen = Some(slot);
                            } else {
                                lighting_collection::open(ui.ctx());
                            }
                        }
                    }
                });
            });
        ui.ctx().request_repaint_after(Duration::from_millis(33));
        if let Some(slot) = chosen {
            self.choose_memory_slot(slot);
            self.memory_activation = Some(slot as u8);
            if self.memory_draft_changed() {
                self.memory_changes
                    .insert(slot as u8, (Instant::now(), self.scene.clone()));
            }
        }
        let pending = !self.memory_changes.is_empty()
            || self.memory_publishing.is_some()
            || self.memory_activation.is_some();
        ui.horizontal_wrapped(|ui| {
            if self.memory_error.is_some()
                || self.memory_editor_slot.is_some() && self.memory_draft_changed() && !pending
            {
                ui.small(RichText::new("Изменения ещё не записаны на клавиатуру").color(AMBER));
                if ui
                    .add_enabled(enabled, egui::Button::new("Повторить сохранение"))
                    .clicked()
                {
                    self.memory_changes.insert(
                        self.memory_slot as u8,
                        (Instant::now() - Duration::from_secs(2), self.scene.clone()),
                    );
                    self.memory_activation = Some(self.memory_slot as u8);
                }
            } else if self.memory_editor_slot.is_none() {
                let free = self.vacant_memory_slot();
                if ui
                    .add_enabled(
                        enabled && free.is_some(),
                        egui::Button::new("Добавить на клавиатуру"),
                    )
                    .clicked()
                {
                    self.add_collection_scene(self.scene.clone());
                }
                if self.memory_state.is_some() && free.is_none() {
                    ui.small("Память заполнена. Освободите место, чтобы добавить новый режим.");
                }
            } else {
                ui.small(
                    RichText::new(
                        if self.playback_channel != PlaybackChannel::Usb
                            || self.memory_state.is_none()
                        {
                            "Черновик · для записи подключите USB"
                        } else if pending {
                            "Сохраняется…"
                        } else {
                            "Сохранено на клавиатуре"
                        },
                    )
                    .color(MUTED),
                );
            }
        });
        if let Some(slot) = self.memory_delete {
            egui::Frame::new()
                .fill(PANEL)
                .corner_radius(10.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(format!(
                        "Удалить режим {}? Перед удалением будет сохранена копия.",
                        slot + 1
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Удалить"))
                            .clicked()
                        {
                            self.memory_operation(Operation::Delete(slot));
                        }
                        if ui.button("Отмена").clicked() {
                            self.memory_delete = None;
                        }
                    });
                });
        }
        ui.add_space(12.0);
    }
    fn memory_thumbnail(&self, ui: &egui::Ui, tile: Rect, scene: Option<&Scene>, active: bool) {
        let p = ui.painter();
        p.rect_filled(tile, 14.0, Color32::from_rgb(8, 12, 19));
        let Some(scene) = scene else {
            p.text(
                tile.center(),
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(28.0),
                MUTED,
            );
            return;
        };
        let time = if active {
            self.memory_clock
                .as_ref()
                .map(|(r, at)| r.frame_after(at.elapsed().as_millis() as u64) as f32 / 50.0)
                .unwrap_or(self.time)
        } else {
            ui.input(|i| i.time) as f32
        };
        let time = design::preview_time(ui.ctx(), time);
        let pulses = effect_gallery::demo_pulses(time);
        if let Ok(colors) = memory::device_preview(
            scene,
            time,
            &pulses,
            self.memory_config.brightness,
            self.memory_config.speed,
            self.memory_clock
                .as_ref()
                .is_some_and(|(r, _)| r.color_transfer == 1),
        ) {
            keyboard_art::miniature(ui, tile, &colors);
        }
    }
    fn memory_connection_status(&mut self, ui: &mut egui::Ui) {
        if self.playback_channel != PlaybackChannel::Usb {
            ui.small("Для записи режимов и энкодера подключите USB.");
        } else if let Some(error) = self.memory_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(AMBER, "Не удалось получить подтверждение клавиатуры.");
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Повторить чтение"))
                    .clicked()
                {
                    self.memory_operation(Operation::Read);
                }
            });
            egui::CollapsingHeader::new("Подробности подключения").show(ui, |ui| {
                ui.label(error);
            });
        } else if self.memory_state.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Загружаю режимы с клавиатуры…");
            });
        }
    }
    fn battery_indicator_settings(&mut self, ui: &mut egui::Ui) {
        let supported = self
            .memory_state
            .as_ref()
            .and_then(|s| s.runtime.as_ref())
            .is_some_and(|r| r.battery_supported);
        surface().show(ui,|ui| {
            ui.spacing_mut().item_spacing.y=8.0;
            ui.label(RichText::new("Заряд по удержанию").size(19.0).strong());
            ui.label(RichText::new("Временный индикатор заряда.").size(13.0).color(MUTED));
            if !supported {ui.colored_label(AMBER,"Нужно обновление клавиатуры");}
            ui.add_space(8.0);
            ui.add_enabled_ui(supported && (!self.busy||self.memory_clock_pending),|ui| {
                let c=&mut self.memory_config.battery;
                ui.checkbox(&mut c.enabled,"Показывать заряд").on_hover_text("Удерживайте энкодер для показа заряда. Короткое нажатие сохраняет своё действие.");
                if c.enabled {
                    ui.add_space(10.0);ui.label("Область индикатора");
                    ui.horizontal_wrapped(|ui| {ui.selectable_value(&mut c.whole_keyboard,false,"Верхний ряд").on_hover_text("Esc, F1–F12 и Delete");ui.selectable_value(&mut c.whole_keyboard,true,"Вся клавиатура");});
                    ui.add_space(10.0);ui.label("Пустая часть");
                    ui.horizontal_wrapped(|ui| {ui.selectable_value(&mut c.background,false,"Текущий эффект");ui.selectable_value(&mut c.background,true,"Свой цвет");});
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {ui.label("Цвет");ui.color_edit_button_srgb(&mut c.filled);if c.background {ui.add_space(12.0);ui.label("Фон");ui.color_edit_button_srgb(&mut c.empty);}});
                    ui.add_space(10.0);
                    ui.collapsing("Время и яркость",|ui| {
                        ui.add(egui::Slider::new(&mut c.hold_seconds,1..=5).text("Удерживать, с"));
                        ui.add(egui::Slider::new(&mut c.display_seconds,1..=10).text("После отпускания, с"));
                        ui.add(egui::Slider::new(&mut c.brightness,0..=255).text("Яркость"));
                    });
                    ui.label(RichText::new(format!("Удержание {} с · после отпускания {} с",c.hold_seconds,c.display_seconds)).size(13.0).color(MUTED));
                }
            });
        });
    }
    fn battery_indicator_preview(&mut self, ui: &mut egui::Ui) {
        let preview = surface().inner_margin(16.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Предпросмотр индикатора");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.battery_preview_percent)
                            .range(0..=100)
                            .speed(1)
                            .suffix("%"),
                    );
                });
            });
            let scene = self
                .memory_state
                .as_ref()
                .and_then(|s| {
                    s.config
                        .selected
                        .and_then(|slot| s.scenes[slot as usize].as_ref())
                })
                .and_then(|p| memory::edit_scene(p).ok())
                .unwrap_or_else(|| self.scene.clone());
            self.keyboard_canvas_for(ui, &scene, self.time, false);
            ui.response()
                .on_hover_text("Пример. На клавиатуре отображается фактический заряд.");
        });
        if self.review_mode {
            review::record(ui.ctx(), "encoder-preview", preview.response.rect);
        }
    }
    fn encoder_settings(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;
        self.memory_connection_status(ui);
        if self.memory_state.is_none() {
            if self.review_mode {
                review::record(ui.ctx(), "encoder-awaiting-usb", ui.min_rect());
            }
            return;
        }
        let enabled = self.memory_state.is_some()
            && (!self.busy)
            && self.playback_cancel.is_none()
            && self.playback_channel == PlaybackChannel::Usb;
        let before = self.memory_config.encoder;
        let battery_before = self.memory_config.battery.clone();
        ui.columns(2, |columns| {
            self.encoder_actions(&mut columns[0], enabled);
            if self.memory_config.battery.enabled {
                self.battery_indicator_preview(&mut columns[0]);
            }
            self.battery_indicator_settings(&mut columns[1]);
        });
        if before != self.memory_config.encoder || battery_before != self.memory_config.battery {
            self.memory_encoder_pending = Some(Instant::now());
        }
        let dirty = self.memory_state.as_ref().is_some_and(|s| {
            s.config.encoder != self.memory_config.encoder
                || s.config.battery != self.memory_config.battery
        });
        ui.horizontal(|ui| {
            if self.memory_encoder_pending.is_some() || self.busy {
                ui.spinner();
                ui.label("Сохраняю…");
            } else if dirty {
                ui.colored_label(AMBER, "Назначения ещё не сохранены.");
                if ui.button("Повторить сохранение").clicked() {
                    self.memory_encoder_pending = Some(Instant::now() - Duration::from_secs(1));
                }
            } else if self.memory_state.is_some() {
                ui.label(
                    RichText::new("Сохранено на клавиатуре")
                        .size(13.0)
                        .color(MUTED),
                );
            }
        });
        ui.add_space(12.0);
    }
    fn encoder_actions(&mut self, ui: &mut egui::Ui, enabled: bool) {
        surface().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new("Действия энкодера").size(19.0).strong());
            ui.add_space(8.0);
            let field_width = (ui.available_width() - 155.0).clamp(160.0, 260.0);
            ui.add_enabled_ui(enabled, |ui| {
                egui::Grid::new("encoder-settings")
                    .spacing(Vec2::new(24.0, 12.0))
                    .show(ui, |ui| {
                        for (i, label) in ["Поворот влево", "Поворот вправо", "Нажатие"]
                            .into_iter()
                            .enumerate()
                        {
                            ui.label(label);
                            egui::ComboBox::from_id_salt(("encoder-action", i))
                                .width(field_width)
                                .selected_text(self.memory_config.encoder[i].label())
                                .show_ui(ui, |ui| {
                                    for action in Action::CHOICES {
                                        ui.selectable_value(
                                            &mut self.memory_config.encoder[i],
                                            action,
                                            action.label(),
                                        );
                                    }
                                });
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                ui.label(RichText::new("Быстрая настройка").color(MUTED).size(12.0));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Музыка").clicked() {
                        self.memory_config.encoder = [
                            Action::Media(0xea),
                            Action::Media(0xe9),
                            Action::Media(0xcd),
                        ];
                    }
                    if ui.button("Подсветка").clicked() {
                        self.memory_config.encoder = [
                            Action::Brightness(-1),
                            Action::Brightness(1),
                            Action::Mode(1),
                        ];
                    }
                    if ui.button("По умолчанию").clicked() {
                        self.memory_config.encoder = [Action::Native; 3];
                    }
                });
            });
        });
    }
}

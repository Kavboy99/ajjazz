//! One update action per device, with read-only preparation before confirmation.
use super::*;
use ajazz_studio::device_updates::{self, Target};
impl Studio {
    fn update_available(&self) -> bool {
        !self.busy
            && !self.update_running
            && self.update_pending.is_none()
            && self.update_prepared.is_none()
            && self.memory_changes.is_empty()
            && self.memory_activation.is_none()
            && self.memory_encoder_pending.is_none()
            && !self.dock_save_pending
            && self.dock_change.is_none()
            && self.calibration_commands.is_none()
            && self.playback_cancel.is_none()
    }
    fn begin_update(&mut self, t: Target) {
        if !self.update_available() {
            return;
        }
        self.save_session();
        self.navigate(Page::Research);
        self.busy = true;
        self.update_pending = Some(t);
        self.update_notice = None;
        self.update_open = false;
        if let Some(c) = &self.screen_commands {
            let _ = c.send(dock_telemetry::Command::Stop);
        }
    }
    pub(super) fn advance_update(&mut self) {
        if self.review_mode || self.update_pending.is_none() {
            return;
        }
        self.busy = true;
        if self.screen_commands.is_some()
            || self.memory_clock_pending
            || self.dock_poll_pending
            || self.pages_poll_inflight
        {
            return;
        }
        let t = self.update_pending.take().unwrap();
        self.update_running = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::UpdatePrepared(
                device_updates::prepare(t).map_err(|e| format!("{e:#}")),
            ));
        });
    }
    pub(super) fn updates_page(&mut self, ui: &mut egui::Ui) {
        let enabled = self.update_available();
        let mut selected = None;
        ui.label(RichText::new("Устройства").size(19.0));
        ui.add_space(10.0);
        for t in Target::ALL {
            let card = surface().inner_margin(16.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.strong(t.title());
                        ui.label(RichText::new(t.detail()).size(13.0).color(MUTED));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let current = match t {
                            Target::Dock => self.screen_design >= 4,
                            Target::Keyboard => self
                                .memory_state
                                .as_ref()
                                .is_some_and(|s| s.build_id == "fd469e3144ab533c"),
                            Target::Mouse => self
                                .mouse_firmware_info
                                .as_ref()
                                .is_some_and(|i| i.mouse_version == Some(0x302)),
                        };
                        if current {
                            ui.menu_button("•••", |ui| {
                                if ui
                                    .add_enabled(enabled, egui::Button::new("Переустановить…"))
                                    .clicked()
                                {
                                    selected = Some(t);
                                    ui.close();
                                }
                            });
                            ui.label(RichText::new("Обновлено").color(TEAL).size(13.0));
                        } else if ui
                            .add_enabled(enabled, egui::Button::new("Обновить…"))
                            .clicked()
                        {
                            selected = Some(t);
                        }
                    });
                });
            });
            super::review::record(ui.ctx(), "update-card", card.response.rect);
            ui.add_space(8.0);
        }
        if let Some(t) = selected {
            self.begin_update(t);
        }
        if self.update_running || self.update_pending.is_some() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(if self.update_open {
                    "Обновление AJAZZ · не отключайте устройство"
                } else {
                    "Проверяем устройство и сохраняем настройки…"
                });
            });
            if self.update_open {
                ui.small(self.update_notice.as_deref().unwrap_or("Следуйте инструкциям в окне AJAZZ. После его закрытия Studio проверит ответ устройства."));
            }
        }
        if !self.update_running
            && let Some(message) = &self.update_notice
        {
            ui.add_space(8.0);
            ui.label(message);
            if message.contains("калибровку") && ui.button("Калибровка клавиатуры").clicked()
            {
                self.navigate(Page::Keyboard);
                self.keyboard_tab = KeyboardTab::Calibration;
            }
        }
        ui.add_space(16.0);
        ui.label(RichText::new("Показатели компьютера").size(19.0));
        ui.add_space(10.0);
        surface().inner_margin(16.0).show(ui,|ui|{
            ui.horizontal(|ui|{
                ui.vertical(|ui|{ui.strong("FPS и датчики");ui.label(RichText::new("Сборщик входит в Studio. Отдельный мониторинг устанавливать не нужно.").size(13.0).color(MUTED));});
                ui.with_layout(Layout::right_to_left(Align::Center),|ui|{
                    let connected=ajazz_studio::metrics_agent::snapshot(&self.graphics_options).is_some();
                    if connected&&device_updates::sensor_driver_installed(){ui.colored_label(TEAL,"Подключены");}
                    else if ui.add_enabled(enabled&&!ajazz_studio::metrics_agent::pending(),egui::Button::new(if ajazz_studio::metrics_agent::pending(){"Подключение…"}else{"Включить…"})).clicked() && !self.review_mode {
                        self.busy=true;let tx=self.tx.clone();std::thread::spawn(move||{let r=device_updates::enable_sensors().map(|()|"Подтвердите запрос Windows для сбора показателей.".into()).map_err(|e|format!("{e:#}"));let _=tx.send(Event::Done(r));});
                    }
                });
            });
            if !device_updates::sensor_driver_installed(){ui.small("Для CPU будет установлен подписанный драйвер PawnIO. Windows запросит подтверждение.");}
        });
    }
    pub(super) fn update_confirmation(&mut self, ctx: &egui::Context) {
        let Some(p) = self.update_prepared.as_ref() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let modal =
            egui::Modal::new(egui::Id::new("firmware-install-confirmation")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.heading(format!("Обновить {}?", p.target.title().to_lowercase()));
                ui.add_space(8.0);
                ui.label(&p.version);
                ui.add_space(8.0);
                ui.label("Устройство проверено. Резервная копия доступных настроек сохранена.");
                if let Some(notice) = &p.notice {
                    ui.label(notice);
                }
                ui.add_space(8.0);
                ui.label("Откроется встроенный в пакет обновлятор AJAZZ.");
                ui.label(p.target.instruction());
                ui.label("Дождитесь завершения. Не отключайте питание.");
                ui.label(
                    RichText::new(
                        "При сбое записи может потребоваться восстановление программатором.",
                    )
                    .size(13.0)
                    .color(MUTED),
                );
                ui.collapsing("Сведения и резервная копия", |ui| {
                    ui.label(p.backup.display().to_string());
                    if let Some(h) = &p.image_sha256 {
                        ui.monospace(h);
                    }
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    cancel = ui.button("Отмена").clicked();
                    confirm = ui
                        .add(egui::Button::new("Открыть обновление").fill(PRIMARY))
                        .clicked();
                });
            });
        cancel |= modal.should_close();
        if cancel {
            self.update_prepared = None;
            self.busy = false;
        }
        if confirm && !self.review_mode {
            let prepared = self.update_prepared.take().unwrap();
            self.update_notice = Some(format!(
                "{} После закрытия обновлятора Studio проверит ответ устройства.",
                prepared.target.instruction()
            ));
            self.update_running = true;
            self.update_open = true;
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = device_updates::launch(&prepared).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::UpdateFinished(result));
            });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_cancels_prepared_update_without_launching_engine() {
        let ctx = egui::Context::default();
        let mut app = Studio::build(&ctx, None, false);
        app.busy = true;
        app.update_prepared = Some(device_updates::Prepared {
            target: Target::Dock,
            version: "UI2".into(),
            directory: std::path::PathBuf::new(),
            executable: "never-run.exe".into(),
            files: Default::default(),
            backup: std::path::PathBuf::from("not-opened.json"),
            notice: None,
            image_sha256: None,
            memory_fingerprint: None,
        });
        for i in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 740.0))),
                    events: if i == 2 {
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                },
                |ctx| app.update_confirmation(ctx),
            );
        }
        assert!(app.update_prepared.is_none());
        assert!(!app.update_running);
        assert!(!app.busy);
    }
}

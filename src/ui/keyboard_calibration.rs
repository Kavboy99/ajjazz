use super::*;
use ajazz_studio::keyboard_calibration::{self as calibration, Command};
impl Studio {
    fn start_calibration(&mut self) {
        if self.busy || self.playback_cancel.is_some() {
            return;
        }
        self.busy = true;
        self.calibration_started = false;
        self.calibration_saving = false;
        self.calibration_saved = false;
        self.calibration_error = None;
        self.calibration_progress = Default::default();
        let (tx, rx) = mpsc::channel();
        self.calibration_commands = Some(tx);
        let events = self.tx.clone();
        self.message("Подготавливаю калибровку и копию настроек…");
        std::thread::spawn(move || {
            let result = calibration::run(rx, |update| {
                let _ = events.send(Event::CalibrationUpdate(update));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = events.send(Event::CalibrationFinished(result));
        });
    }
    pub(super) fn calibration_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Калибровка магнитных клавиш");
        let running = self.calibration_commands.is_some();
        let ready = self.calibration_started && self.calibration_progress.may_commit();
        ui.label("1. Подготовьте клавиатуру → 2. Пройдите все клавиши → 3. Сохраните результат");
        if let Some(error) = &self.calibration_error {
            ui.colored_label(Color32::from_rgb(255, 180, 105), error);
        }
        if self.calibration_saved {
            ui.colored_label(
                TEAL,
                "Готово. Калибровка сохранена в клавиатуре, остальные настройки проверены.",
            );
        } else if self.calibration_saving {
            ui.spinner();
            ui.label("Сохраняю калибровку и проверяю настройки. Не отключайте USB…");
        } else if ready {
            ui.colored_label(
                TEAL,
                "Все клавиши пройдены. Отпустите их и нажмите «Сохранить калибровку».",
            );
        } else if self.calibration_started {
            ui.label("Можно начинать. По очереди полностью нажимайте и отпускайте клавиши, включая Fn. Подтверждённые клавиши станут зелёными.");
        } else if running {
            ui.spinner();
            ui.label("Готовлю клавиатуру. Пока не нажимайте клавиши…");
        } else {
            ui.label("Подключите USB и уберите предметы с клавиш. Нажмите «Начать» и дождитесь приглашения. При отмене без сохранения потребуется переподключить USB.");
        }
        ui.horizontal_wrapped(|ui| {
            if !running
                && ui
                    .add_enabled(
                        !self.busy
                            && self.playback_cancel.is_none()
                            && self.playback_channel == PlaybackChannel::Usb,
                        egui::Button::new("Начать калибровку"),
                    )
                    .clicked()
            {
                self.start_calibration();
            }
            if running {
                if ui
                    .add_enabled(
                        ready && !self.calibration_saving,
                        egui::Button::new("Сохранить калибровку"),
                    )
                    .clicked()
                    && let Some(tx) = &self.calibration_commands
                    && tx.send(Command::Finish).is_ok()
                {
                    self.calibration_saving = true;
                }
                if ui
                    .add_enabled(
                        !self.calibration_saving,
                        egui::Button::new("Прервать без сохранения"),
                    )
                    .clicked()
                    && let Some(tx) = &self.calibration_commands
                {
                    let _ = tx.send(Command::Cancel);
                }
            }
            ui.label("Проводное подключение · AJKS 1.36");
        });
        let count = self.calibration_progress.completed.len();
        ui.add(egui::ProgressBar::new(count as f32 / 82.0).text(format!("{count} из 82 клавиш")));
        let rows = keyboard_rows();
        if self.calibration_started && !ready && !self.calibration_saving {
            let missing: Vec<_> = rows
                .iter()
                .flatten()
                .filter(|k| !self.calibration_progress.completed.contains(&k.id))
                .take(10)
                .map(|k| k.label.as_str())
                .collect();
            ui.label(format!(
                "Ещё не пройдены: {}{}",
                missing.join(", "),
                if 82 - count > 10 { "…" } else { "" }
            ));
        }
        let width = ui.available_width().min(1000.0);
        let unit = (width - 24.0) / 17.4;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, unit * 6.4), Sense::hover());
        ui.painter()
            .rect_filled(rect, 14.0, Color32::from_rgb(10, 15, 23));
        for (row, keys) in rows.iter().enumerate() {
            let mut x = rect.left() + 12.0;
            for key in keys {
                let w = unit * key.width;
                let r = Rect::from_min_size(
                    Pos2::new(x, rect.top() + 12.0 + row as f32 * unit * 1.02),
                    Vec2::new(w - 4.0, unit - 4.0),
                );
                let done = self.calibration_progress.completed.contains(&key.id);
                ui.painter().rect_filled(
                    r,
                    5.0,
                    if done {
                        Color32::from_rgb(26, 105, 86)
                    } else {
                        PANEL
                    },
                );
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    &key.label,
                    FontId::proportional((unit * 0.22).max(10.0)),
                    if done { TEAL } else { MUTED },
                );
                x += w;
            }
        }
        if let Some(sample) = &self.calibration_progress.last {
            let label = rows
                .iter()
                .flatten()
                .find(|key| key.id == sample.id)
                .map(|key| key.label.as_str())
                .unwrap_or("—");
            ui.label(format!("Последняя подтверждённая клавиша: {label}"));
        }
    }
}

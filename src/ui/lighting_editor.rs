use super::*;

impl Studio {
    pub(super) fn lighting_scene_editor(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        if width >= 850.0 {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(width * 0.59 - 12.0, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| self.lighting_preview_and_layers(ui),
                );
                ui.allocate_ui_with_layout(
                    Vec2::new(width * 0.41 - 12.0, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| self.lighting_layer_inspector(ui),
                );
            });
        } else {
            self.lighting_preview_and_layers(ui);
            ui.add_space(16.0);
            self.lighting_layer_inspector(ui);
        }
    }
    fn lighting_preview_and_layers(&mut self, ui: &mut egui::Ui) {
        if self.dock_surface {
            self.dock_ring(ui);
        } else {
            self.keyboard_canvas(ui);
            ui.collapsing("Выбор клавиш", |ui| self.groups(ui));
        }
        ui.horizontal(|ui| {
            if ui
                .small_button(if self.playing {
                    "Пауза"
                } else {
                    "Воспроизвести"
                })
                .on_hover_text("Пауза предпросмотра · не меняет подсветку клавиатуры")
                .clicked()
            {
                self.playing = !self.playing;
            }
            ui.add(
                egui::Slider::new(&mut self.scene.brightness, 0.0..=1.0)
                    .text("Яркость")
                    .show_value(false),
            );
        });
        if self.memory_preview && !self.dock_surface {
            if self.memory_clock_failed {
                ui.colored_label(AMBER, "Синхронизация потеряна. Нажмите «Обновить».");
            } else if self
                .memory_clock
                .as_ref()
                .is_some_and(|(runtime, _)| runtime.paused)
            {
                ui.small("Анимация на клавиатуре приостановлена");
            } else if self.memory_clock.is_none() && self.memory_state.is_some() {
                ui.small(
                    "Для синхронизации темпа и цвета требуется обновление прошивки клавиатуры.",
                );
            }
        }
        ui.add_space(12.0);
        let layer_count = self.layers().len();
        egui::CollapsingHeader::new(format!("Слои · {layer_count}"))
            .id_salt("lighting-layers-disclosure")
            .default_open(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .corner_radius(12.0)
                    .inner_margin(14.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.strong("Слои");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let max_layers = if self.memory_preview && !self.dock_surface {
                                    8
                                } else {
                                    32
                                };
                                if ui
                                    .add_enabled(
                                        self.layers().len() < max_layers,
                                        egui::Button::new("+ Слой"),
                                    )
                                    .clicked()
                                {
                                    self.layers().push(Layer::default());
                                    self.layer = self.layers().len() - 1;
                                }
                            });
                        });
                        let selected = self.layer;
                        let mut pick = None;
                        egui::ScrollArea::vertical()
                            .id_salt("lighting-layers")
                            .max_height(160.0)
                            .show(ui, |ui| {
                                for (index, layer) in self.layers().iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut layer.enabled, "");
                                        if ui
                                            .selectable_label(
                                                index == selected,
                                                format!(
                                                    "{} · {}",
                                                    layer.name,
                                                    layer.effect.label()
                                                ),
                                            )
                                            .clicked()
                                        {
                                            pick = Some(index);
                                        }
                                    });
                                }
                            });
                        if let Some(index) = pick {
                            self.layer = index;
                        }
                        ui.small("Слои в списке расположены снизу вверх.");
                    });
            });
        egui::CollapsingHeader::new("Название и экспорт").show(ui, |ui| {
            ui.label("Название режима");
            ui.text_edit_singleline(&mut self.scene.name);
            ui.add(
                egui::Slider::new(&mut self.scene.duration, 1.0..=30.0).text("Длительность GIF, с"),
            );
            ui.add(egui::Slider::new(&mut self.scene.fps, 1..=60).text("Кадров/с GIF"));
            if ui.button("Экспортировать GIF…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("GIF", &["gif"])
                    .set_file_name("lighting.gif")
                    .save_file()
            {
                let scene = self.scene.clone();
                let tx = self.tx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result = screen::export_lighting_gif(&scene, &path)
                        .map(|()| format!("GIF сохранён: {}", path.display()))
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Event::Done(result));
                });
            }
        });
    }
    fn lighting_layer_inspector(&mut self, ui: &mut egui::Ui) {
        let selection = self.selected.clone();
        let dock = self.dock_surface;
        let index = self.layer;
        let covered = lighting::covering_layer(self.layers(), index).map(|l| l.name.clone());
        let mut duplicate = false;
        let mut remove = false;
        let mut up = false;
        let mut down = false;
        let max_layers = if self.memory_preview && !dock { 8 } else { 32 };
        let count = self.layers().len();
        let time = ui.input(|i| i.time) as f32;
        let brightness = self.memory_config.brightness;
        let speed = self.memory_config.speed;
        let transfer = self
            .memory_clock
            .as_ref()
            .is_some_and(|(r, _)| r.color_transfer == 1);
        let review_mode = self.review_mode;
        egui::Frame::new().fill(PANEL).corner_radius(12.0).inner_margin(18.0).show(ui,|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Оформление").size(19.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("•••", |ui| {
                        if ui.add_enabled(count<max_layers,egui::Button::new("Дублировать")).clicked() { duplicate=true;ui.close(); }
                        if ui.add_enabled(index+1<count,egui::Button::new("Поверх следующего слоя")).clicked() { up=true;ui.close(); }
                        if ui.add_enabled(index>0,egui::Button::new("Под предыдущий слой")).clicked() { down=true;ui.close(); }
                        ui.separator();
                        if ui.button("Удалить слой").clicked() { remove=true;ui.close(); }
                    });
                });
            });
            if let Some(layer)=self.layers().get_mut(index) {
                if let Some(name)=covered { ui.colored_label(AMBER,format!("Этот слой закрыт слоем «{name}». Измените порядок или прозрачность.")); }
                effect_gallery::picker(ui, layer, (dock,index), time, brightness, speed, transfer, review_mode);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Палитра");ui.color_edit_button_srgb(&mut layer.colors[0]);ui.color_edit_button_srgb(&mut layer.colors[1]);
                });
                ui.spacing_mut().slider_width=(ui.available_width()-16.0).max(140.0);
                ui.label("Скорость");
                ui.add(egui::Slider::new(&mut layer.speed,0.0..=3.0).show_value(false)).on_hover_text(format!("{:.2}",layer.speed));
                egui::CollapsingHeader::new("Дополнительные параметры").show(ui,|ui| {
                    ui.label("Название слоя");ui.text_edit_singleline(&mut layer.name);
                    ui.add(egui::Slider::new(&mut layer.scale,0.0625..=5.0).text("Размер рисунка"));
                    ui.add(egui::Slider::new(&mut layer.angle,0.0..=360.0).text("Направление"));
                    ui.add(egui::Slider::new(&mut layer.opacity,0.0..=1.0).text("Непрозрачность"));
                    ui.add(egui::Slider::new(&mut layer.phase,0.0..=1.0).text("Сдвиг"));
                    egui::ComboBox::from_id_salt("blend").selected_text(layer.blend.label()).show_ui(ui,|ui| { for blend in Blend::ALL { ui.selectable_value(&mut layer.blend,blend,blend.label()); } });
                });
                if !dock && (!selection.is_empty() || !layer.keys.is_empty()) {
                    ui.separator();
                    ui.label(RichText::new(if layer.keys.is_empty() { "Вся клавиатура".into() } else { format!("Только {} клавиш",layer.keys.len()) }).size(13.0).color(MUTED));
                    ui.horizontal_wrapped(|ui| {
                        if !selection.is_empty() && ui.button(format!("Применить к выделенным · {}",selection.len())).clicked() { layer.keys=selection; }
                        if !layer.keys.is_empty() && ui.button("На всю клавиатуру").clicked() { layer.keys.clear(); }
                    });

                }
            } else { ui.label("Добавьте слой, чтобы создать эффект."); }
        });
        if duplicate && let Some(layer) = self.layers().get(index).cloned() {
            self.layers().push(layer);
            self.layer = self.layers().len() - 1;
        }
        if remove && index < count {
            self.layers().remove(index);
            self.layer = index.saturating_sub(1);
        }
        if up {
            self.layers().swap(index, index + 1);
            self.layer += 1;
        }
        if down {
            self.layers().swap(index, index - 1);
            self.layer -= 1;
        }
    }
}

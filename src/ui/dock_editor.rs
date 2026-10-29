//! Screen ribbon, local selection and the corresponding page inspector.
use super::*;
use dock_pages::Kind;

struct ScreenDrag;

fn panel() -> egui::Frame {
    surface()
}
fn title(page: &dock_pages::Page) -> String {
    match page.kind {
        Kind::Summary => "Сводка".into(),
        Kind::Cpu => "CPU".into(),
        Kind::Memory => "Память".into(),
        Kind::Clock => "Часы".into(),
        Kind::Media => dock_pages::asset_label(page.asset),
        Kind::Analog => "Циферблат".into(),
        Kind::Calendar => "Календарь".into(),
        Kind::Focus => "Фокус".into(),
        Kind::Stopwatch => "Секундомер".into(),
        Kind::Gaming => "В игре".into(),
        Kind::Thermals => "Температуры".into(),
        Kind::Gpu => "Видеокарта".into(),
        Kind::Pulse => "Пульс системы".into(),
        Kind::Custom => ajazz_studio::dock_composition::Composition::presets()
            .into_iter()
            .find(|(_, c)| c.metrics == page.composition.metrics)
            .map(|(name, _)| name.to_owned())
            .unwrap_or_else(|| {
                page.composition
                    .metrics
                    .iter()
                    .filter(|m| **m != ajazz_studio::dock_composition::Metric::None)
                    .map(|m| m.short())
                    .collect::<Vec<_>>()
                    .join(" · ")
            }),
    }
}
fn position(ui: &mut egui::Ui, xy: &mut [u8; 2]) {
    ui.horizontal(|ui| {
        ui.label("Положение");
        ui.add(egui::DragValue::new(&mut xy[0]).range(0..=127).prefix("X "));
        ui.add(egui::DragValue::new(&mut xy[1]).range(0..=127).prefix("Y "));
    });
}

impl Studio {
    pub(super) fn dock_draft_needs_upgrade(&self) -> bool {
        (self.screen_design < 3 && self.dock_pages.pages.iter().any(|p| p.kind == Kind::Custom))
            || (self.screen_design < 2 && self.dock_pages.pages.iter().any(|p| p.kind as u8 > 8))
            || (self
                .page_status
                .as_ref()
                .is_some_and(|s| s.capabilities & 128 == 0)
                && (self.dock_telemetry.style != 0
                    || self.dock_telemetry.focus_minutes != 25
                    || self.dock_pages.pages.iter().any(|p| p.kind as u8 > 4)))
    }
    fn dock_timer_command(&mut self, command: Option<u8>) {
        let start = matches!(command, Some(0 | 1));
        let mut c = self.dock_telemetry.clone();
        c.enabled = true;
        let mut p = self.dock_pages.clone();
        p.enabled = true;
        if let Some(sender) = &self.screen_commands {
            if start {
                let _ = sender.send(dock_telemetry::Command::Config(c, false));
                let _ = sender.send(dock_telemetry::Command::Pages(p, false));
            }
            let _ = sender.send(dock_telemetry::Command::Timer(command));
        } else {
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let r = dock_telemetry::timer(command, if start { Some((&c, &p)) } else { None })
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::TimerResult(r));
            });
        }
    }
    pub(super) fn dock_editor_actions(&mut self, ui: &mut egui::Ui) {
        let enabled = (!self.busy || self.screen_commands.is_some())
            && self
                .page_status
                .as_ref()
                .is_none_or(|s| s.erasing.is_none());
        ui.menu_button("•••", |ui| {
            if ui.button("Ручной редактор макета").clicked() {
                self.dock_tab = DockTab::Telemetry;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new("Применить до отключения питания"),
                )
                .clicked()
            {
                self.pages_apply(false);
                ui.close();
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Загрузить оформление с дока"))
                .on_hover_text("Заменить текущий черновик настройками устройства")
                .clicked()
            {
                self.pages_read(true);
                ui.close();
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Обновить подключение"))
                .clicked()
            {
                self.pages_read(false);
                ui.close();
            }
        });
        if self.dock_save_error.is_some() {
            if ui
                .add_enabled(
                    enabled && !self.dock_save_pending,
                    egui::Button::new("Повторить сохранение"),
                )
                .clicked()
            {
                self.dock_change = Some(Instant::now() - Duration::from_secs(2));
            }
        } else {
            ui.small(
                RichText::new(if self.dock_change.is_some() || self.dock_save_pending {
                    "Сохраняется…"
                } else {
                    "Сохраняется автоматически"
                })
                .color(MUTED),
            );
        }
    }

    fn dock_show_page(&mut self, index: usize) {
        if self.dock_draft_needs_upgrade() {
            self.message("Для этого оформления требуется обновление OLED.");
            return;
        }
        let mut config = self.dock_telemetry.clone();
        config.enabled = true;
        let mut pages = self.dock_pages.clone();
        pages.enabled = true;
        pages.selected = index as u8;
        if let Err(error) = pages.validate().and_then(|_| config.validate()) {
            self.message(error.to_string());
            return;
        }
        self.dock_pages.selected = index as u8;
        if let Some(c) = &self.screen_commands {
            let _ = c.send(dock_telemetry::Command::Editor(
                config,
                pages,
                false,
                Some(index as u8),
            ));
        } else {
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let r = dock_telemetry::show_editor(&config, &pages, index as u8)
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::PagesResult(r, false));
            });
        }
    }

    fn dock_ribbon(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let thumbnail = if ctx.content_rect().height() < 650.0 {
            56.0
        } else if ctx.content_rect().height() < 950.0 {
            80.0
        } else {
            96.0
        };
        let count = self.dock_pages.pages.len();
        self.dock_editor_index = self.dock_editor_index.min(count.saturating_sub(1));
        self.dock_card_textures.truncate(count);
        let mut render_pages = self.dock_pages.clone();
        // An unfinished draft (e.g. all cards disabled) still needs a useful preview.
        for page in &mut render_pages.pages {
            page.enabled = true;
        }
        let mut key = self
            .dock_telemetry
            .encode()
            .map(|v| v.to_vec())
            .unwrap_or_default();
        key.extend(render_pages.encode().unwrap_or([0; 48]));
        if let Some(s) = &self.screen_sample {
            key.extend(s.encode().unwrap_or([0; 16]));
        }
        key.extend(chrono::Local::now().timestamp().to_le_bytes());
        if design::preview_phase(ctx).is_none()
            && render_pages.pages.iter().any(|p| p.kind == Kind::Custom)
        {
            key.extend((chrono::Local::now().timestamp_millis() * 9 / 1000).to_le_bytes());
            ctx.request_repaint_after(Duration::from_millis(111));
        }
        if let Some(t) = &self.dock_timer {
            key.extend(serde_json::to_vec(t).unwrap_or_default());
        }
        if key != self.dock_preview_key {
            for (index, page) in render_pages.pages.iter().enumerate() {
                let mut page_preview = render_pages.clone();
                page_preview.selected = index as u8;
                let image = if page.kind == Kind::Custom {
                    Ok(ajazz_studio::dock_telemetry_preview::render_composition(
                        &self.dock_telemetry,
                        &page.composition,
                        self.screen_sample.as_ref(),
                        chrono::Local::now(),
                        design::preview_phase(ctx),
                    ))
                } else {
                    ajazz_studio::dock_telemetry_preview::render_page_with_timer(
                        &self.dock_telemetry,
                        &page_preview,
                        page.kind,
                        self.screen_sample.as_ref(),
                        chrono::Local::now(),
                        self.dock_timer.as_ref(),
                    )
                };
                if let Ok(image) = image {
                    let pixels =
                        egui::ColorImage::from_rgba_unmultiplied([128, 128], image.as_raw());
                    if let Some(texture) = self.dock_card_textures.get_mut(index) {
                        texture.set(pixels, egui::TextureOptions::NEAREST);
                    } else {
                        self.dock_card_textures.push(ctx.load_texture(
                            format!("dock-card-{index}"),
                            pixels,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
            }
            self.dock_preview_key = key;
        }
        let mut swap = None;
        let mut chosen = None;
        let mut release_target = None;
        // Remember the press itself, including fast gestures whose press/release
        // events arrive in one frame. egui's live drag payload supplies the ghost.
        let (press, release, cancel) = ctx.input(|i| {
            let mut press = None;
            let mut release = None;
            for event in &i.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } = event
                {
                    if *pressed {
                        press = Some(*pos);
                    } else {
                        release = Some(*pos);
                    }
                }
            }
            (press, release, i.key_pressed(egui::Key::Escape))
        });
        if press.is_some() || cancel {
            self.dock_drag_start = None;
        }
        ui.horizontal(|ui| {
            let active = self.dock_pages.pages.iter().filter(|p| p.enabled).count();
            ui.colored_label(MUTED, format!("{active} в показе"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("+ Добавить экран", |ui| {
                    for kind in [
                        Kind::Custom,
                        Kind::Clock,
                        Kind::Analog,
                        Kind::Calendar,
                        Kind::Focus,
                        Kind::Stopwatch,
                        Kind::Media,
                    ] {
                        if ui
                            .add_enabled(count < 8, egui::Button::new(kind.label()))
                            .clicked()
                        {
                            self.dock_pages.pages.push(dock_pages::Page {
                                kind,
                                ..Default::default()
                            });
                            self.dock_editor_index = count;
                            ui.close();
                        }
                    }
                    ui.collapsing("Прежние макеты", |ui| {
                        for kind in [
                            Kind::Summary,
                            Kind::Cpu,
                            Kind::Memory,
                            Kind::Gaming,
                            Kind::Thermals,
                            Kind::Gpu,
                            Kind::Pulse,
                        ] {
                            if ui
                                .add_enabled(count < 8, egui::Button::new(kind.label()))
                                .clicked()
                            {
                                self.dock_pages.pages.push(dock_pages::Page {
                                    kind,
                                    ..Default::default()
                                });
                                self.dock_editor_index = count;
                                ui.close();
                            }
                        }
                    });
                });
            });
        });
        egui::ScrollArea::horizontal()
            .id_salt("dock-screen-ribbon")
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for index in 0..count {
                        let selected = index == self.dock_editor_index;
                        let page = self.dock_pages.pages[index].clone();
                        let card = egui::Frame::new()
                            .fill(if selected {
                                Color32::from_rgb(26, 34, 48)
                            } else {
                                PANEL
                            })
                            .stroke(Stroke::new(
                                if selected { 2.0_f32 } else { 1.0_f32 },
                                if selected {
                                    TEAL
                                } else {
                                    Color32::from_rgb(46, 57, 70)
                                },
                            ))
                            .corner_radius(12.0)
                            .inner_margin(12.0)
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(thumbnail);
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new(title(&page)).strong(),
                                                )
                                                .truncate()
                                                .sense(Sense::click()),
                                            )
                                            .clicked()
                                        {
                                            self.dock_editor_index = index;
                                        }
                                    });
                                    let source = ui.dnd_drag_source(
                                        egui::Id::new(("dock-drag", index)),
                                        ScreenDrag,
                                        |ui| {
                                            if page.kind == Kind::Media {
                                                self.dock_media_placeholder(
                                                    ui, page.asset, thumbnail,
                                                )
                                            } else if let Some(texture) =
                                                self.dock_card_textures.get(index)
                                            {
                                                ui.add(
                                                    egui::Image::new((
                                                        texture.id(),
                                                        Vec2::splat(thumbnail),
                                                    ))
                                                    .sense(Sense::click()),
                                                )
                                            } else {
                                                ui.allocate_response(
                                                    Vec2::splat(thumbnail),
                                                    Sense::click(),
                                                )
                                            }
                                        },
                                    );
                                    if source.inner.clicked() {
                                        self.dock_editor_index = index;
                                        if page.enabled {
                                            chosen = Some(index);
                                        }
                                    }
                                    if let Some(pos) = press
                                        && source.response.rect.contains(pos)
                                    {
                                        self.dock_drag_start = Some((index, pos));
                                        self.dock_editor_index = index;
                                    }
                                    source.response.context_menu(|ui| {
                                        if ui
                                            .add_enabled(
                                                index > 0,
                                                egui::Button::new("Переместить влево"),
                                            )
                                            .clicked()
                                        {
                                            swap = Some((index, index - 1));
                                            ui.close();
                                        }
                                        if ui
                                            .add_enabled(
                                                index + 1 < count,
                                                egui::Button::new("Переместить вправо"),
                                            )
                                            .clicked()
                                        {
                                            swap = Some((index, index + 1));
                                            ui.close();
                                        }
                                    });
                                    if !page.enabled {
                                        ui.small(RichText::new("Скрыт").color(MUTED));
                                    }
                                });
                            });
                        if release.is_some_and(|pos| card.response.rect.contains(pos)) {
                            release_target = Some(index);
                        }
                    }
                });
            });
        // Resolve only after every source and target has been visited; a fast
        // right-to-left drag may press and release within the same input frame.
        if let Some(to) = release_target
            && let Some(pos) = release
            && let Some((from, origin)) = self.dock_drag_start
            && from != to
            && from < count
            && origin.distance(pos) > 8.0
        {
            swap = Some((from, to));
        }
        if release.is_some() || cancel {
            self.dock_drag_start = None;
        }
        if let Some((a, b)) = swap {
            let selected = usize::from(self.dock_pages.selected);
            let page = self.dock_pages.pages.remove(a);
            self.dock_pages.pages.insert(b, page);
            self.dock_editor_index = b;
            self.dock_pages.selected = if selected == a {
                b
            } else if a < b && selected > a && selected <= b {
                selected - 1
            } else if a > b && selected >= b && selected < a {
                selected + 1
            } else {
                selected
            } as u8;
            self.dock_change = Some(Instant::now());
        }
        if let Some(index) = chosen {
            self.dock_show_page(index);
            self.dock_change = Some(Instant::now());
        }
    }

    fn dock_media_placeholder(&self, ui: &mut egui::Ui, asset: u8, side: f32) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
        let paint = ui.painter();
        paint.rect_filled(rect, 8.0, Color32::from_rgb(9, 15, 23));
        let present = self
            .page_status
            .as_ref()
            .is_some_and(|s| s.has_asset(asset));
        paint.text(
            rect.center() - Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            dock_pages::asset_label(asset),
            FontId::proportional((side / 8.0).clamp(15.0, 28.0)),
            if present { TEAL } else { MUTED },
        );
        paint.text(
            rect.center() + Vec2::new(0.0, 20.0),
            egui::Align2::CENTER_CENTER,
            if present {
                "В памяти дока"
            } else {
                "Пустой слот"
            },
            FontId::proportional(12.0),
            MUTED,
        );
        response
    }

    fn dock_page_inspector(&mut self, ui: &mut egui::Ui, enabled: bool) -> bool {
        let index = self.dock_editor_index;
        let count = self.dock_pages.pages.len();
        let show = false;
        let mut remove = false;
        let mut timer_command = None;
        let other_enabled = self
            .dock_pages
            .pages
            .iter()
            .enumerate()
            .any(|(i, p)| i != index && p.enabled);
        let page = &mut self.dock_pages.pages[index];

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if page.kind == Kind::Custom {
                    "Показатели"
                } else {
                    "Настройки экрана"
                })
                .size(19.0)
                .strong(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("•••", |ui| {
                    if page.kind == Kind::Custom {
                        ui.menu_button("Изменить тип экрана", |ui| {
                            for kind in Kind::ALL {
                                if ui.button(kind.label()).clicked() {
                                    page.kind = kind;
                                    page.asset = 0;
                                    page.composition = Default::default();
                                    ui.close();
                                }
                            }
                        });
                    }
                    remove = ui
                        .add_enabled(count > 1, egui::Button::new("Убрать экран из списка"))
                        .on_hover_text("Фото и GIF остаются в памяти дока")
                        .clicked();
                    if remove {
                        ui.close();
                    }
                });
            });
        });
        ui.add_space(4.0);
        if page.kind != Kind::Custom {
            ui.horizontal(|ui| {
                ui.label("Содержимое");
                egui::ComboBox::from_id_salt("dock-editor-kind")
                    .width(190.0)
                    .selected_text(page.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in Kind::ALL {
                            if ui
                                .selectable_value(&mut page.kind, kind, kind.label())
                                .changed()
                                && kind != Kind::Media
                            {
                                page.asset = 0;
                                page.composition = Default::default();
                                if kind == Kind::Custom {
                                    page.seconds = page.seconds.min(255);
                                }
                            }
                        }
                    });
            });
        }
        if count > 1 || page.kind != Kind::Custom {
            ui.horizontal(|ui| {
                ui.add_enabled(
                    other_enabled || !page.enabled,
                    egui::Checkbox::new(&mut page.enabled, "Показывать на доке"),
                )
                .on_disabled_hover_text("Хотя бы один экран должен оставаться в показе");
                if self.dock_pages.autoplay {
                    ui.add(
                        egui::DragValue::new(&mut page.seconds)
                            .range(1..=if page.kind == Kind::Custom { 255 } else { 3600 })
                            .suffix(" с"),
                    )
                    .on_hover_text("Длительность показа");
                }
            });
        }
        if page.kind != Kind::Custom {
            ui.separator();
        }
        match page.kind {
            Kind::Custom => {
                super::dock_composition_editor::editor(
                    ui,
                    &mut page.composition,
                    &self.dock_telemetry,
                    self.screen_sample.as_ref(),
                );
            }
            Kind::Media => {
                egui::ComboBox::from_id_salt("dock-editor-asset")
                    .selected_text(dock_pages::asset_label(page.asset))
                    .show_ui(ui, |ui| {
                        for asset in 0..6 {
                            let present = self
                                .page_status
                                .as_ref()
                                .is_some_and(|s| s.has_asset(asset));
                            ui.selectable_value(
                                &mut page.asset,
                                asset,
                                format!(
                                    "{}{}",
                                    dock_pages::asset_label(asset),
                                    if present { "" } else { " · пусто" }
                                ),
                            );
                        }
                    });
                ui.colored_label(MUTED, "Файлы загружаются во вкладке «Изображения и GIF».");
            }
            Kind::Clock => {
                if self.dock_telemetry.style == 0 {
                    ui.add(
                        egui::Slider::new(&mut self.dock_pages.clock_scale, 1..=3)
                            .text("Размер часов"),
                    );
                } else {
                    let mut large = self.dock_pages.clock_scale > 1;
                    let prior = large;
                    ui.horizontal(|ui| {
                        ui.label("Цифры");
                        ui.selectable_value(&mut large, false, "Обычные");
                        ui.selectable_value(&mut large, true, "Крупные");
                    });
                    if large != prior {
                        self.dock_pages.clock_scale = if large { 3 } else { 1 };
                    }
                }
                if self.dock_telemetry.style == 0 {
                    ui.collapsing("Ручное положение", |ui| {
                        position(ui, &mut self.dock_pages.clock_position)
                    });
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.dock_pages.show_date, "Дата");
                    ui.checkbox(&mut self.dock_pages.show_seconds, "Секунды");
                });
            }
            Kind::Cpu | Kind::Memory => {
                if self.dock_telemetry.style == 0 {
                    ui.add(
                        egui::Slider::new(&mut self.dock_pages.detail_scale, 1..=3)
                            .text("Размер показателей"),
                    );
                } else {
                    let mut large = self.dock_pages.detail_scale > 1;
                    let prior = large;
                    ui.horizontal(|ui| {
                        ui.label("Цифры");
                        ui.selectable_value(&mut large, false, "Обычные");
                        ui.selectable_value(&mut large, true, "Крупные");
                    });
                    if large != prior {
                        self.dock_pages.detail_scale = if large { 3 } else { 1 };
                    }
                }
                if self.dock_telemetry.style == 0 {
                    ui.collapsing("Ручное положение", |ui| {
                        position(
                            ui,
                            if page.kind == Kind::Cpu {
                                &mut self.dock_pages.cpu_position
                            } else {
                                &mut self.dock_pages.memory_position
                            },
                        )
                    });
                }
            }
            Kind::Summary => {
                egui::ComboBox::from_id_salt("dock-summary-layout")
                    .selected_text(
                        [
                            "Ручной макет",
                            "С часами",
                            "Акцент на CPU",
                            "Система и графика",
                        ][usize::from(self.dock_telemetry.style)],
                    )
                    .show_ui(ui, |ui| {
                        for (i, label) in [
                            "Ручной макет",
                            "С часами",
                            "Акцент на CPU",
                            "Система и графика",
                        ]
                        .iter()
                        .enumerate()
                        {
                            ui.selectable_value(&mut self.dock_telemetry.style, i as u8, *label);
                        }
                    });
                ui.label("Как показывать загрузку");
                let mut display = if self.dock_telemetry.bars {
                    if self.dock_telemetry.percentages {
                        2
                    } else {
                        0
                    }
                } else {
                    1
                };
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut display, 0, "Полосы");
                    ui.selectable_value(&mut display, 1, "Проценты");
                    ui.selectable_value(&mut display, 2, "Вместе");
                });
                self.dock_telemetry.bars = display != 1;
                self.dock_telemetry.percentages = display != 0;
                ui.checkbox(&mut self.dock_telemetry.memory_size, "Объём памяти");
                if self.dock_telemetry.style == 3 {
                    ui.collapsing("Источник FPS", |ui| {
                        ui.add_enabled_ui(true, |ui| {
                            ui.checkbox(&mut self.graphics_options.fps_enabled, "Считать FPS игры");
                            ui.label("Игра (необязательно)");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.graphics_options.process_name)
                                    .hint_text("Автоматически · или game.exe"),
                            );
                        });
                    });
                    ui.label(RichText::new("Без данных — прочерк. GPU и VRAM доступны с NVIDIA.").size(13.0).color(MUTED))
                        .on_hover_text("FPS измеряет встроенный сборщик Windows. Для доступа используется отдельный фоновый процесс с разрешением Windows.");
                } else {
                    ui.checkbox(&mut self.dock_telemetry.clock, "Часы");
                }
                if self.dock_telemetry.style == 0 {
                    ui.small("Расположение — во вкладке «Оформление показателей».");
                }
            }
            Kind::Gaming => {
                ui.label("Крупный FPS, средний интервал между кадрами и температуры CPU/GPU.");
                ui.small("Игра определяется автоматически. При переключении на рабочий стол захват продолжается.");
                ui.collapsing("Источник FPS", |ui| {
                    ui.checkbox(&mut self.graphics_options.fps_enabled, "Считать FPS игры");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.graphics_options.process_name)
                            .hint_text("Автоматически · или game.exe"),
                    );
                });
            }
            Kind::Thermals => {
                ui.label("Температуры CPU, GPU, накопителей и системной платы.");
                ui.small("Накопители: самая высокая доступная температура. Недоступный датчик обозначен прочерком.");
            }
            Kind::Gpu => {
                ui.label(
                    "Загрузка видеокарты, заполнение VRAM, температура и потребление в ваттах.",
                );
            }
            Kind::Pulse => {
                ui.label("Загрузка CPU и GPU за последние 32 обновления.");
                ui.small("График строится по реальным измерениям. После потери связи история начинается заново.");
            }
            Kind::Analog => {
                ui.checkbox(&mut self.dock_pages.show_seconds, "Секундная стрелка");
            }
            Kind::Calendar => {
                ui.small("Неделя начинается с понедельника.");
            }
            Kind::Focus | Kind::Stopwatch => {
                if page.kind == Kind::Focus {
                    ui.add(
                        egui::Slider::new(&mut self.dock_telemetry.focus_minutes, 1..=180)
                            .text("Длительность, мин"),
                    );
                }
                ui.label("Отсчёт продолжается в доке после остановки CPU/RAM. При отключении питания сбрасывается.");
                let supported = self
                    .page_status
                    .as_ref()
                    .is_some_and(|s| s.capabilities & 128 != 0);
                if !supported {
                    ui.colored_label(AMBER, "Для таймеров требуется обновление OLED.");
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(enabled && supported, egui::Button::new("Запустить"))
                        .clicked()
                    {
                        timer_command = Some(if page.kind == Kind::Focus { 0 } else { 1 });
                    }
                    let mode = if page.kind == Kind::Focus { 1 } else { 2 };
                    if let Some(t) = self.dock_timer.as_ref().filter(|t| t.mode == mode) {
                        if ui
                            .add_enabled(
                                enabled && supported,
                                egui::Button::new(if t.running {
                                    "Пауза"
                                } else {
                                    "Продолжить"
                                }),
                            )
                            .clicked()
                        {
                            timer_command = Some(if t.running { 2 } else { 3 });
                        }
                        if ui
                            .add_enabled(enabled && supported, egui::Button::new("Сбросить"))
                            .clicked()
                        {
                            timer_command = Some(4);
                        }
                        if ui
                            .add_enabled(enabled && supported, egui::Button::new("Завершить"))
                            .clicked()
                        {
                            timer_command = Some(5);
                        }
                    }
                });
            }
        }
        ui.separator();
        ui.collapsing("Цвет и яркость", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (name, bg, fg, accent) in [
                    ("Графит", [5, 7, 11], [245, 247, 252], [112, 174, 255]),
                    ("Бумага", [242, 240, 232], [25, 30, 34], [40, 95, 215]),
                    ("Полночь", [8, 10, 23], [233, 230, 255], [165, 138, 255]),
                    ("Тёплый", [25, 20, 17], [250, 239, 216], [249, 166, 72]),
                ] {
                    if ui.small_button(name).clicked() {
                        self.dock_telemetry.background = bg;
                        self.dock_telemetry.foreground = fg;
                        self.dock_telemetry.accent = accent;
                        self.dock_telemetry.border = false;
                    }
                }
            });
            ui.add(
                egui::Slider::new(&mut self.dock_telemetry.brightness, 1..=100).text("Яркость, %"),
            );
            ui.collapsing("Свой стиль и детали", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Фон");
                    ui.color_edit_button_srgb(&mut self.dock_telemetry.background);
                    ui.label("Текст");
                    ui.color_edit_button_srgb(&mut self.dock_telemetry.foreground);
                    ui.label("Акцент");
                    ui.color_edit_button_srgb(&mut self.dock_telemetry.accent);
                });
                ui.checkbox(&mut self.dock_telemetry.border, "Рамка");
                if self.dock_telemetry.style == 0
                    || matches!(page.kind, Kind::Focus | Kind::Stopwatch)
                {
                    ui.checkbox(&mut self.dock_telemetry.show_title, "Заголовок");
                }
            });
        });
        if let Some(command) = timer_command {
            self.dock_timer_command(Some(command));
        }
        if remove {
            self.dock_pages.pages.remove(index);
            let selected = usize::from(self.dock_pages.selected);
            self.dock_pages.selected = if selected > index {
                selected - 1
            } else {
                selected.min(count - 2)
            } as u8;
            self.dock_editor_index = index.min(count - 2);
        }
        show && !remove
    }

    pub(super) fn dock_pages_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut access_banner = false;
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 7.0);
        let running = self.screen_commands.is_some();
        let erasing = self
            .page_status
            .as_ref()
            .is_some_and(|s| s.erasing.is_some());
        let enabled = (!self.busy || running) && !erasing;
        let timer_visible = self
            .page_status
            .as_ref()
            .is_some_and(|s| s.capabilities & 128 != 0)
            && self
                .dock_pages
                .pages
                .get(self.dock_editor_index)
                .is_some_and(|p| matches!(p.kind, Kind::Focus | Kind::Stopwatch));
        if !self.review_mode
            && !running
            && !self.busy
            && !self.dock_poll_pending
            && self.page_status.is_some()
            && self.pages_polled.elapsed()
                > Duration::from_secs(if erasing || timer_visible { 1 } else { 3 })
        {
            self.pages_polled = Instant::now();
            self.dock_poll_pending = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result =
                    dock_telemetry::background_status(timer_visible).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::DockBackground(result));
            });
        }
        #[cfg(windows)]
        if !running && self.screen_sample_time.elapsed() >= Duration::from_secs(1) {
            if let Some(counters) = self.screen_counters.as_mut() {
                self.screen_sample = counters.sample(0).ok();
            }
            self.screen_sample_time = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        ui.vertical(|ui| {
            if !self.screen_online
                || self
                    .page_status
                    .as_ref()
                    .is_some_and(|s| s.save_pending || s.erasing.is_some())
            {
                ui.horizontal_wrapped(|ui| {
                    let message = if self.screen_online
                        && dock_telemetry::needs_samples(&self.dock_telemetry, &self.dock_pages)
                    {
                        "● Показатели обновляются автоматически"
                    } else if self.screen_online {
                        "Экраны работают без приложения"
                    } else if self
                        .endpoints
                        .iter()
                        .any(|e| e.vid == 0x3151 && e.pid == 0x5007)
                    {
                        "Подключение к доку…"
                    } else {
                        "Подключите док по USB"
                    };
                    if !self.screen_online {
                        ui.label(
                            RichText::new(message)
                                .size(13.0)
                                .color(if self.screen_online { TEAL } else { MUTED }),
                        )
                        .on_hover_text("Показатели продолжают обновляться в трее.");
                    }
                    if let Some(s) = &self.page_status {
                        if s.save_pending {
                            ui.label(RichText::new("Сохранение…").size(13.0).color(MUTED));
                        }
                        if let Some(asset) = s.erasing {
                            ui.colored_label(
                                AMBER,
                                format!(
                                    "Удаление {}: {}%",
                                    dock_pages::asset_label(asset),
                                    s.progress
                                ),
                            );
                        }
                    }
                });
            }
            let metric_page = self.dock_pages.pages.iter().any(|p| {
                p.enabled
                    && matches!(
                        p.kind,
                        Kind::Gaming | Kind::Thermals | Kind::Gpu | Kind::Pulse | Kind::Custom
                    )
            });
            let cpu_missing = self
                .screen_sample
                .as_ref()
                .is_some_and(|s| s.graphics.temperatures.cpu.is_none());
            let thermal_page = self.dock_pages.pages.iter().any(|p| {
                p.enabled
                    && (matches!(p.kind, Kind::Gaming | Kind::Thermals)
                        || (p.kind == Kind::Custom
                            && p.composition
                                .metrics
                                .contains(&ajazz_studio::dock_composition::Metric::CpuTemp)))
            });
            let sensor_access = thermal_page
                && cpu_missing
                && ajazz_studio::metrics_agent::snapshot(&self.graphics_options).is_none();
            if sensor_access
                || (self.fps_requires_admin
                    && (metric_page
                        || (self.graphics_options.fps_enabled && self.dock_telemetry.style == 3)))
            {
                access_banner = true;
                surface().inner_margin(10.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Для FPS и датчиков нужно разрешение Windows");
                        if ui
                            .add_enabled(
                                !ajazz_studio::metrics_agent::pending(),
                                egui::Button::new(if ajazz_studio::metrics_agent::pending() {
                                    "Подключение…"
                                } else {
                                    "Разрешить…"
                                }),
                            )
                            .on_hover_text("Запрос Windows включает отдельный сборщик показателей.")
                            .clicked()
                        {
                            self.enable_fps();
                        }
                    });
                });
            }
        });
        ui.add_space(8.0);
        if self.dock_draft_needs_upgrade() {
            ui.colored_label(
                AMBER,
                "Новые экраны и шаблоны будут доступны на доке после обновления OLED.",
            );
        }
        self.dock_ribbon(ui, ctx);
        ui.add_space(12.0);
        let index = self.dock_editor_index;
        let page = self.dock_pages.pages[index].clone();
        let mut show = false;
        let width = ui.available_width();
        // Keep content height stable while the outer page scrolls. Using the
        // current cursor's screen Y here made the pane grow as it scrolled.
        let warning = if access_banner { 96.0 } else { 0.0 };
        let media_footer = if page.kind == Kind::Media { 24.0 } else { 0.0 };
        let pane_height =
            (ctx.content_rect().height() - 390.0 - warning - media_footer).clamp(180.0, 490.0);
        let preview_side = (width * 0.30).min(300.0).min(pane_height - 90.0).max(128.0);
        let editor = |ui: &mut egui::Ui, this: &mut Self| {
            panel().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("dock-inspector")
                    .max_height(pane_height - 36.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        show = this.dock_page_inspector(ui, enabled);
                    });
            });
        };
        ui.horizontal_top(|ui| {
            let preview = panel().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(preview_side);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(5, 9, 14))
                        .corner_radius(16.0)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            if page.kind == Kind::Media {
                                self.dock_media_placeholder(ui, page.asset, preview_side - 20.0);
                            } else if let Some(texture) = self.dock_card_textures.get(index) {
                                ui.image((texture.id(), Vec2::splat(preview_side - 20.0)));
                            }
                        });
                    if page.kind == Kind::Media {
                        ui.add_space(6.0);
                        ui.small("Ресурс в памяти дока");
                    }
                });
            });
            if self.review_mode {
                review::record(ui.ctx(), "dock-preview", preview.response.rect);
            }
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width().max(280.0), pane_height),
                Layout::top_down(Align::Min),
                |ui| {
                    let mut editor = editor;
                    editor(ui, self);
                },
            );
        });
        if show {
            self.dock_show_page(self.dock_editor_index);
        }
        ui.add_space(8.0);
        ui.collapsing("Переключение",|ui| {
            ui.checkbox(&mut self.dock_pages.autoplay, "Автоматически");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(
                    &mut self.dock_pages.skip_offline,
                    "Пропускать CPU/RAM после остановки передачи",
                )
                .on_hover_text("Страницы остаются в списке и памяти дока. При выключенной опции они доступны с прочерками вместо показателей.");
            });
            ui.add_space(5.0);
            if let Err(error) = self
                .dock_pages
                .validate()
                .and_then(|_| self.dock_telemetry.validate())
            {
                ui.colored_label(AMBER, error.to_string());
            }
        });
        let enabled_count = self.dock_pages.pages.iter().filter(|p| p.enabled).count();
        if enabled_count < 2 {
            ui.small(
                RichText::new(
                    "В показе один экран. Включите ещё один, чтобы переключать их задней кнопкой.",
                )
                .color(AMBER),
            );
        } else {
            ui.small(RichText::new("Задняя кнопка переключает включённые экраны, даже после выхода из Studio. Данные ПК доступны, пока Studio работает в трее.").color(MUTED));
        }
        if let Some(error) = &self.dock_save_error {
            ui.colored_label(AMBER, error);
        }
        ui.add_space(8.0);
        self.dock_library(ui, ctx, enabled);
    }

    fn dock_library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, enabled: bool) {
        ui.collapsing("Медиатека дока · фото и GIF", |ui| {
            ui.small(
                "Убрать экран из показа можно выше. «Стереть файл» удаляет сам ресурс из памяти.",
            );
            for asset in 0..6 {
                let present = self
                    .page_status
                    .as_ref()
                    .is_some_and(|s| s.has_asset(asset));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(dock_pages::asset_label(asset)).strong());
                    ui.colored_label(
                        if present { TEAL } else { MUTED },
                        if present {
                            "В памяти"
                        } else if self.page_status.is_some() {
                            "Пусто"
                        } else {
                            "Не прочитано"
                        },
                    );
                    if asset == 5
                        && present
                        && let Some(s) = &self.page_status
                    {
                        ui.small(format!("{} кадров", s.gif_frames));
                    }
                    if ui
                        .add_enabled(
                            present && self.dock_pages.pages.len() < 8,
                            egui::Button::new("Добавить экран"),
                        )
                        .clicked()
                    {
                        self.dock_pages.pages.push(dock_pages::Page {
                            kind: Kind::Media,
                            asset,
                            ..Default::default()
                        });
                        self.dock_editor_index = self.dock_pages.pages.len() - 1;
                    }
                    if ui
                        .add_enabled(present && enabled, egui::Button::new("Стереть файл…"))
                        .clicked()
                    {
                        self.delete_asset_confirmation = Some(asset);
                    }
                });
            }
        });
        ui.collapsing("Штатный интерфейс дока", |ui| {
            ui.small("Возврат к экранам AJAZZ завершает передачу CPU/RAM.");
            if ui
                .add_enabled(enabled, egui::Button::new("Вернуться к штатным экранам"))
                .clicked()
            {
                if let Some(c) = &self.screen_commands {
                    let _ = c.send(dock_telemetry::Command::Native);
                } else {
                    self.busy = true;
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let r = dock_telemetry::native().map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Event::PagesResult(r, false));
                    });
                }
            }
        });
        if let Some(asset) = self.delete_asset_confirmation {
            egui::Window::new("Стереть файл из дока?").collapsible(false).resizable(false).show(ctx,|ui| {
                ui.set_max_width(400.0);
                ui.label(format!("{} будет удалён из памяти и из списка экранов. Для возврата понадобится исходный файл.",dock_pages::asset_label(asset)));
                ui.horizontal(|ui| {
                    if ui.add_enabled(enabled,egui::Button::new("Стереть файл")).clicked(){
                        self.delete_asset_confirmation=None;
                        if let Some(c)=&self.screen_commands{let _=c.send(dock_telemetry::Command::Delete(asset));}
                        else{self.busy=true;let tx=self.tx.clone();std::thread::spawn(move||{let r=dock_telemetry::delete_asset(asset).map_err(|e|format!("{e:#}"));let _=tx.send(Event::PagesResult(r,false));});}
                    }
                    if ui.button("Отмена").clicked(){self.delete_asset_confirmation=None;}
                });
            });
        }
    }
}

//! Shared presentation and local accessibility preferences. No device writes.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub scale: u16,
    pub reduce_motion: bool,
    pub reduce_transparency: bool,
    pub high_contrast: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            scale: 100,
            reduce_motion: system_reduce_motion(),
            reduce_transparency: false,
            high_contrast: false,
        }
    }
}
fn system_reduce_motion() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled = 1i32;
        // Read the user's Windows preference; never change system settings.
        if unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            )
        } != 0
        {
            return enabled == 0;
        }
    }
    false
}
fn motion_key() -> egui::Id {
    egui::Id::new("studio-reduce-motion")
}
pub(super) fn preview_phase(ctx: &egui::Context) -> Option<u8> {
    ctx.data(|d| d.get_temp::<bool>(motion_key()).unwrap_or(false))
        .then_some(12)
}
pub(super) fn preview_time(ctx: &egui::Context, time: f32) -> f32 {
    if ctx.data(|d| d.get_temp::<bool>(motion_key()).unwrap_or(false)) {
        2.4
    } else {
        time
    }
}
pub(super) fn install(ctx: &egui::Context, prefs: &Preferences) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_zoom_factor(f32::from(prefs.scale.clamp(100, 130)) / 100.0);
    ctx.data_mut(|d| d.insert_temp(motion_key(), prefs.reduce_motion));
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("studio-opaque-chrome"),
            prefs.reduce_transparency,
        )
    });
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(17, 19, 24);
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(12, 14, 18);
    v.faint_bg_color = PANEL;
    v.selection.bg_fill = Color32::from_rgb(36, 65, 100);
    v.selection.stroke = Stroke::new(1.5_f32, TEAL);
    v.widgets.inactive.bg_fill = Color32::from_rgb(43, 47, 56);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(43, 47, 56);
    v.widgets.inactive.fg_stroke.color = Color32::from_rgb(242, 244, 248);
    v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(239, 241, 247);
    v.widgets.hovered.bg_fill = Color32::from_rgb(57, 65, 79);
    v.widgets.hovered.fg_stroke.color = Color32::WHITE;
    v.widgets.active.fg_stroke.color = Color32::WHITE;
    for widget in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        widget.corner_radius = egui::CornerRadius::same(5);
        widget.bg_stroke = if prefs.high_contrast {
            Stroke::new(1.0_f32, MUTED)
        } else {
            Stroke::NONE
        };
    }
    if prefs.high_contrast {
        v.override_text_color = Some(Color32::WHITE);
    }
    v.handle_shape = egui::style::HandleShape::Circle;
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(12.0, 10.0);
        s.spacing.interact_size = Vec2::new(40.0, 36.0);
        s.spacing.button_padding = Vec2::new(12.0, 8.0);
        s.animation_time = if prefs.reduce_motion { 0.0 } else { 0.14 };
        for (kind, size) in [
            (egui::TextStyle::Small, 13.0),
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Heading, 27.0),
        ] {
            s.text_styles.insert(kind, FontId::proportional(size));
        }
    });
}
/// Translucency is confined to navigation; content stays opaque and legible.
pub(super) fn chrome(ctx: &egui::Context) -> egui::Frame {
    let opaque = ctx.data(|d| {
        d.get_temp::<bool>(egui::Id::new("studio-opaque-chrome"))
            .unwrap_or(false)
    });
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            32,
            37,
            46,
            if opaque { 255 } else { 238 },
        ))
        .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(24)))
        .corner_radius(12.0)
        .inner_margin(4.0)
}
pub(super) fn tabs(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    chrome(ui.ctx()).show(ui, |ui| {
        ui.horizontal_wrapped(content);
    });
}
fn nav_icon(p: &egui::Painter, center: Pos2, page: Page, ink: Color32) {
    let r = Rect::from_center_size(center, Vec2::new(20.0, 16.0));
    let s = Stroke::new(1.5_f32, ink);
    match page {
        Page::Lighting => {
            for dx in [-6.0, 0.0, 6.0] {
                p.line_segment(
                    [center + Vec2::new(dx, -7.0), center + Vec2::new(dx, 7.0)],
                    s,
                );
            }
            for (dx, dy) in [(-6.0, -3.0), (0.0, 3.0), (6.0, -1.0)] {
                p.circle_filled(center + Vec2::new(dx, dy), 2.7, ink);
            }
        }
        Page::Keyboard => {
            p.rect_stroke(r, 3.0, s, StrokeKind::Inside);
            for x in [-6.0, -2.0, 2.0, 6.0] {
                p.circle_filled(center + Vec2::new(x, -2.0), 1.0, ink);
            }
            p.line_segment(
                [center + Vec2::new(-5.0, 3.0), center + Vec2::new(5.0, 3.0)],
                s,
            );
        }
        Page::Mouse => {
            p.rect_stroke(
                Rect::from_center_size(center, Vec2::new(13.0, 20.0)),
                6.0,
                s,
                StrokeKind::Inside,
            );
            p.line_segment(
                [center + Vec2::new(0.0, -6.0), center + Vec2::new(0.0, -2.0)],
                s,
            );
        }
        Page::Dock => {
            p.rect_stroke(r, 4.0, s, StrokeKind::Inside);
            p.line_segment(
                [
                    center + Vec2::new(-5.0, 3.0),
                    center + Vec2::new(-2.0, -1.0),
                ],
                s,
            );
            p.line_segment(
                [center + Vec2::new(-2.0, -1.0), center + Vec2::new(2.0, 2.0)],
                s,
            );
            p.line_segment(
                [center + Vec2::new(2.0, 2.0), center + Vec2::new(6.0, -4.0)],
                s,
            );
        }
        _ => {
            p.circle_stroke(center, 7.0, s);
            p.circle_stroke(center, 2.5, s);
            for d in [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y] {
                p.line_segment([center + d * 7.0, center + d * 10.0], s);
            }
        }
    }
}
impl Studio {
    pub(super) fn navigate(&mut self, page: Page) {
        if page != self.page {
            self.navigation.push(self.page);
            self.navigation
                .drain(..self.navigation.len().saturating_sub(16));
            self.page = page;
        }
    }
    fn nav_row(&mut self, ui: &mut egui::Ui, page: Page, label: &str) {
        let selected = self.page == page
            || page == Page::Preferences && matches!(self.page, Page::Devices | Page::Research);
        let r = ui.add_sized(
            [ui.available_width(), 44.0],
            egui::Button::new(format!("     {label}"))
                .fill(if selected {
                    Color32::from_rgb(40, 65, 96)
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(if selected && self.preferences.high_contrast {
                    Stroke::new(1.5_f32, TEAL)
                } else {
                    Stroke::NONE
                })
                .corner_radius(11.0),
        );
        nav_icon(
            ui.painter(),
            Pos2::new(r.rect.left() + 21.0, r.rect.center().y),
            page,
            if selected { TEAL } else { MUTED },
        );
        if r.has_focus() || r.hovered() {
            ui.painter().rect_stroke(
                r.rect.shrink(1.0),
                11.0,
                Stroke::new(
                    1.0_f32,
                    if r.has_focus() {
                        TEAL
                    } else {
                        Color32::from_white_alpha(70)
                    },
                ),
                StrokeKind::Inside,
            );
        }
        if r.clicked() {
            self.navigate(page);
        }
    }
    pub(super) fn modern_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .exact_width(if ctx.content_rect().width() < 1000.0 {
                182.0
            } else {
                202.0
            })
            .resizable(false)
            .frame(chrome(ctx).inner_margin(12.0).corner_radius(0.0))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("AJAZZ").size(23.0).strong());
                ui.add_space(24.0);
                ui.small(RichText::new("Клавиатура").color(MUTED));
                self.nav_row(ui, Page::Lighting, "Подсветка");
                self.nav_row(ui, Page::Keyboard, "Клавиши");
                ui.add_space(12.0);
                self.nav_row(ui, Page::Mouse, "Мышь");
                self.nav_row(ui, Page::Dock, "Экран дока");
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.menu_button("Studio", |ui| {
                        ui.small(format!("Версия {}", env!("CARGO_PKG_VERSION")));
                        if self.tray.is_some() && ui.button("Свернуть в трей").clicked()
                        {
                            self.hide_to_tray(ctx);
                            ui.close();
                        }
                        if ui.button("Завершить Studio").clicked() {
                            self.exit_requested = true;
                            ui.close();
                        }
                    });
                    ui.add_space(8.0);
                    self.nav_row(ui, Page::Preferences, "Настройки");
                    ui.add_space(10.0);
                    let kb = PlaybackChannel::detect(&self.endpoints);
                    let dock = self
                        .endpoints
                        .iter()
                        .any(|e| e.vid == 0x3151 && e.pid == 0x5007);
                    ui.small(
                        RichText::new(if dock {
                            "Док · USB"
                        } else {
                            "Док не подключён"
                        })
                        .color(MUTED),
                    );
                    let battery = self
                        .memory_clock
                        .as_ref()
                        .and_then(|(r, _)| r.battery_percent)
                        .or_else(|| self.keyboard_info.as_ref().map(|i| i.battery));
                    let connection = match kb {
                        Some(PlaybackChannel::Usb) => "Клавиатура · USB",
                        Some(PlaybackChannel::Radio) => "Клавиатура · 2.4 GHz",
                        None => "Клавиатура не подключена",
                    };
                    ui.small(
                        RichText::new(if let Some(battery) = battery.filter(|_| kb.is_some()) {
                            format!("Клавиатура · {battery}%")
                        } else {
                            connection.to_owned()
                        })
                        .color(MUTED),
                    )
                    .on_hover_text(connection);
                });
            });
    }
    pub(super) fn preferences_page(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(760.0);
        let before = self.preferences.clone();
        let card_width = ui.available_width();
        surface().show(ui,|ui| {
            ui.set_width((card_width-36.0).max(240.0));
            ui.strong("Внешний вид");ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("Размер интерфейса");
                for scale in [100,115,130] {ui.selectable_value(&mut self.preferences.scale,scale,format!("{scale}%"));}
            });
            ui.checkbox(&mut self.preferences.high_contrast,"Повысить контраст");
            ui.checkbox(&mut self.preferences.reduce_transparency,"Уменьшить прозрачность");
            ui.checkbox(&mut self.preferences.reduce_motion,"Уменьшить движение")
                .on_hover_text("Отключает автоматические анимации предпросмотра. Режимы устройств сохраняются.");
        });
        ui.add_space(12.0);
        surface().show(ui, |ui| {
            ui.set_width((card_width - 36.0).max(240.0));
            ui.strong("Устройства");
            ui.add_space(6.0);
            if ui
                .add_sized(
                    [ui.available_width(), 40.0],
                    egui::Button::new("Обновления   ›"),
                )
                .clicked()
            {
                self.navigate(Page::Research);
            }
            if ui
                .add_sized(
                    [ui.available_width(), 40.0],
                    egui::Button::new("Подключения   ›"),
                )
                .clicked()
            {
                self.navigate(Page::Devices);
            }
        });
        ui.add_space(12.0);
        ui.collapsing("Журнал", |ui| {
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for line in self.logs.iter().rev().take(40) {
                        ui.small(line);
                    }
                });
        });
        if before != self.preferences {
            if self.preferences.reduce_motion {
                self.playing = false;
            }
            install(ui.ctx(), &self.preferences);
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("studio-opaque-chrome"),
                    self.preferences.reduce_transparency,
                )
            });
        }
    }
    pub(super) fn feedback(&mut self, ctx: &egui::Context) {
        let error = self.status.starts_with("Ошибка")
            || self.status.contains("остановлена:")
            || self.status.contains("Не удалось")
            || self.status.starts_with("Чтение остановлено");
        if self.status.is_empty() && self.playback_cancel.is_none() {
            return;
        }
        if !error
            && !self.busy
            && self.playback_cancel.is_none()
            && (self.status.is_empty() || self.status_changed.elapsed() > Duration::from_secs(4))
        {
            return;
        }
        let mut dismiss = false;
        let toast =
            egui::Area::new(egui::Id::new("studio-feedback"))
                .anchor(egui::Align2::RIGHT_BOTTOM, [-20.0, -20.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    surface().inner_margin(14.0).show(ui, |ui| {
                        let width = (ctx.content_rect().width() - 250.0).clamp(240.0, 430.0);
                        ui.set_width(width);
                        ui.horizontal_top(|ui| {
                            if self.busy && !self.memory_automatic {
                                ui.spinner();
                            }
                            ui.allocate_ui_with_layout(
                                Vec2::new(width - if self.busy { 116.0 } else { 88.0 }, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&self.status)
                                                .size(13.0)
                                                .color(if error { AMBER } else { Color32::WHITE }),
                                        )
                                        .wrap(),
                                    );
                                },
                            );
                            dismiss = ui.small_button("Закрыть").clicked();
                        });
                        if let Some(cancel) = &self.playback_cancel
                            && ui.button("Остановить анимацию").clicked()
                        {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    });
                });
        if self.review_mode {
            review::record(ctx, "feedback-toast", toast.response.rect);
        }
        if dismiss {
            self.status.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_sessions_get_accessible_defaults_and_preferences_round_trip() {
        let legacy = serde_json::json!({"schema":1,"scene":Scene::default(),"screen":ScreenDesign::default(),"lighting_backup":null,"hall_backup":null,"mouse_backup":null});
        let saved: Session = serde_json::from_value(legacy).unwrap();
        assert_eq!(saved.last_page, Page::Lighting);
        let p = saved.preferences;
        assert_eq!(p.scale, 100);
        let p = Preferences {
            scale: 130,
            reduce_motion: true,
            reduce_transparency: true,
            high_contrast: true,
        };
        assert_eq!(
            serde_json::from_slice::<Preferences>(&serde_json::to_vec(&p).unwrap()).unwrap(),
            p
        );
        let ctx = egui::Context::default();
        install(&ctx, &p);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(ctx.zoom_factor(), 1.3);
        assert_eq!(ctx.style().animation_time, 0.0);
        assert_eq!(preview_time(&ctx, 92.0), 2.4);
    }
    #[test]
    fn navigation_returns_to_the_origin_without_losing_drafts() {
        let ctx = egui::Context::default();
        let mut s = Studio::build(&ctx, None, false);
        let scene = s.scene.clone();
        s.navigate(Page::Preferences);
        s.navigate(Page::Research);
        assert_eq!(s.navigation.pop(), Some(Page::Preferences));
        assert_eq!(
            serde_json::to_vec(&scene).unwrap(),
            serde_json::to_vec(&s.scene).unwrap()
        );
    }
}

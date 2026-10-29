//! A scene gallery adds to a vacant slot; browsing never changes the user's bank.
use super::*;
use ajazz_studio::lighting_collection as collection;

pub(super) fn key() -> egui::Id {
    egui::Id::new("lighting-collection-open")
}
pub(super) fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(key(), true));
}

impl Studio {
    pub(super) fn vacant_memory_slot(&self) -> Option<usize> {
        let state = self.memory_state.as_ref()?;
        (0..8).find(|&slot| {
            state.scenes[slot].is_none()
                && !self.memory_drafts.contains_key(&(slot as u8))
                && !self.memory_changes.contains_key(&(slot as u8))
                && self.memory_editor_slot != Some(slot as u8)
                && self.memory_publishing != Some(slot as u8)
        })
    }
    pub(super) fn add_collection_scene(&mut self, scene: Scene) {
        if let Some(slot) = self.vacant_memory_slot() {
            self.choose_memory_slot(slot);
            self.scene = scene;
            self.memory_changes
                .insert(slot as u8, (Instant::now(), self.scene.clone()));
            self.memory_activation = Some(slot as u8);
        } else {
            self.memory_drafts
                .insert(self.memory_editor_slot.unwrap_or(8), self.scene.clone());
            self.scene = scene;
            self.memory_editor_slot = None;
            self.message(if self.memory_state.is_some() {
                "Открыт черновик. Сохранённые режимы не заменены: память клавиатуры заполнена."
            } else {
                "Открыт черновик. Подключите клавиатуру, чтобы добавить режим в память."
            });
        }
        self.memory_preview = true;
        self.memory_delete = None;
        self.layer = self
            .scene
            .layers
            .iter()
            .position(|l| l.effect != Effect::Solid)
            .unwrap_or(0);
        self.time = 0.0;
        self.pulses.clear();
        self.selected.clear();
        self.playing = true;
    }
    pub(super) fn forget_deleted_memory_slot(&mut self, slot: u8) {
        self.memory_drafts.remove(&slot);
        self.memory_changes.remove(&slot);
        if self.memory_activation == Some(slot) {
            self.memory_activation = None;
        }
        if self.memory_editor_slot == Some(slot) {
            self.memory_editor_slot = None;
            self.scene = Scene {
                name: "Новый эффект".into(),
                layers: vec![Layer::default()],
                ..Default::default()
            };
            self.layer = 0;
        }
    }
    pub(super) fn collection_gallery(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !ctx.data(|d| d.get_temp::<bool>(key()).unwrap_or(false)) {
            return;
        }
        let mut choice = None;
        let mut blank = false;
        let mut close = false;
        let filter_id = key().with("filter");
        let mut filter = ctx.data(|d| d.get_temp::<usize>(filter_id).unwrap_or(0));
        let modal_id = key().with("modal");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).default_size(Vec2::new(
                850.0,
                (ctx.content_rect().height() - 50.0).max(400.0),
            )))
            .frame(surface().inner_margin(24.0))
            .show(&ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 96.0).clamp(400.0, 800.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Коллекция").size(23.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = ui.button("Готово").clicked();
                        blank = ui.button("С нуля").clicked();
                    });
                });
                ui.label(
                    RichText::new(if self.memory_state.is_none() {
                        "Выберите режим для предпросмотра. Клавиатуру можно подключить позже."
                    } else if self.vacant_memory_slot().is_none() {
                        "Память заполнена. Выбор откроет черновик, сохранив ваши режимы."
                    } else {
                        "Выбор добавит режим на клавиатуру."
                    })
                    .size(13.0)
                    .color(MUTED),
                );
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in [
                        "Все",
                        "Природа",
                        "Космос",
                        "Атмосфера",
                        "Движение",
                        "Отклик",
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut filter, i, *name);
                    }
                });
                let family =
                    ["", "nature", "space", "ambient", "motion", "reactive"][filter.min(5)];
                let looks: Vec<_> = collection::all()
                    .iter()
                    .filter(|p| filter == 0 || p.family == family)
                    .collect();
                let columns = if ui.available_width() >= 700.0 { 4 } else { 3 };
                let w = (ui.available_width() - 12.0 * (columns - 1) as f32 - 8.0) / columns as f32;
                let preview = effect_gallery::Preview {
                    time: design::preview_time(ui.ctx(), ui.input(|i| i.time) as f32),
                    brightness: 255,
                    speed: 128,
                    transfer: true,
                };
                egui::ScrollArea::vertical()
                    .id_salt("collection-scroll")
                    .max_height((ctx.content_rect().height() - 240.0).clamp(220.0, 620.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for row in looks.chunks(columns) {
                            ui.horizontal(|ui| {
                                for look in row {
                                    let (r, response) =
                                        ui.allocate_exact_size(Vec2::new(w, 180.0), Sense::click());
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            ui.is_enabled(),
                                            format!(
                                                "Добавить: {}. {}",
                                                look.name, look.description
                                            ),
                                        )
                                    });
                                    if ui.is_rect_visible(r) {
                                        let p = ui.painter();
                                        p.rect_filled(
                                            r,
                                            12.0,
                                            if response.hovered() {
                                                Color32::from_rgb(38, 43, 54)
                                            } else {
                                                Color32::from_rgb(28, 32, 40)
                                            },
                                        );
                                        if response.hovered() || response.has_focus() {
                                            p.rect_stroke(
                                                r.shrink(1.0),
                                                12.0,
                                                Stroke::new(1.5_f32, TEAL),
                                                StrokeKind::Inside,
                                            );
                                        }
                                        let tile = Rect::from_center_size(
                                            r.center_top() + Vec2::new(0.0, 82.0),
                                            Vec2::splat(148.0_f32.min(w - 16.0)),
                                        );
                                        effect_gallery::scene_thumbnail(
                                            ui,
                                            tile,
                                            &look.scene,
                                            preview,
                                        );
                                        let mut job = egui::text::LayoutJob::simple_singleline(
                                            look.name.clone(),
                                            FontId::proportional(14.0),
                                            Color32::WHITE,
                                        );
                                        job.wrap.max_width = w - 12.0;
                                        job.wrap.max_rows = 1;
                                        let text = p.layout_job(job);
                                        p.galley(
                                            Pos2::new(
                                                r.center().x - text.size().x / 2.0,
                                                r.bottom() - 25.0,
                                            ),
                                            text,
                                            Color32::WHITE,
                                        );
                                        if self.review_mode {
                                            review::record(
                                                &ctx,
                                                &format!("collection-{}", look.id),
                                                r,
                                            );
                                        }
                                    }
                                    if response.on_hover_text(&look.description).clicked() {
                                        choice = Some(look.scene.clone());
                                    }
                                }
                            });
                        }
                    });
            });
        if self.review_mode {
            review::record(&ctx, "collection-gallery", response.response.rect);
        }
        ctx.data_mut(|d| d.insert_temp(filter_id, filter));
        if blank {
            choice = Some(Scene {
                name: "Новый эффект".into(),
                layers: vec![Layer::default()],
                ..Default::default()
            });
        }
        let chosen = choice.is_some();
        if let Some(scene) = choice {
            self.add_collection_scene(scene);
        }
        if chosen || close || response.should_close() {
            ctx.data_mut(|d| d.insert_temp(key(), false));
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_hover_and_escape_do_not_write_then_one_click_adds_only_a_free_slot() {
        let ctx = egui::Context::default();
        let mut app = app();
        let before = serde_json::to_value(&app.scene).unwrap();
        let bank = serde_json::to_value(&app.memory_state).unwrap();
        let mut time = 0.0;
        let frame = |app: &mut Studio, time: &mut f64, events| {
            *time += 0.25;
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("offline-layout-rects"),
                    Vec::<(String, Rect)>::new(),
                )
            });
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 740.0))),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.collection_gallery(ui));
                },
            );
        };
        open(&ctx);
        for _ in 0..4 {
            frame(&mut app, &mut time, vec![]);
        }
        let first = |ctx: &egui::Context| {
            ctx.data(|d| d.get_temp::<Vec<(String, Rect)>>(egui::Id::new("offline-layout-rects")))
                .unwrap()
                .iter()
                .find(|(name, _)| name == "collection-living-forest")
                .unwrap()
                .1
                .center()
        };
        let pos = first(&ctx);
        frame(&mut app, &mut time, vec![egui::Event::PointerMoved(pos)]);
        assert_eq!(serde_json::to_value(&app.scene).unwrap(), before);
        assert!(app.memory_changes.is_empty());
        frame(
            &mut app,
            &mut time,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!ctx.data(|d| d.get_temp::<bool>(key()).unwrap()));
        assert_eq!(serde_json::to_value(&app.scene).unwrap(), before);
        open(&ctx);
        for _ in 0..4 {
            frame(&mut app, &mut time, vec![]);
        }
        let pos = first(&ctx);
        for pressed in [true, false] {
            frame(
                &mut app,
                &mut time,
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
        assert_eq!(app.memory_editor_slot, Some(1));
        assert_eq!(app.memory_changes.len(), 1);
        assert_eq!(app.scene.name, collection::all()[0].name);
        assert_eq!(serde_json::to_value(&app.memory_state).unwrap(), bank);
        assert!(!ctx.data(|d| d.get_temp::<bool>(key()).unwrap()));
    }
    fn app() -> Studio {
        let mut app = Studio::build(&egui::Context::default(), None, false);
        let program = ajazz_studio::keyboard_memory::compile(&Scene::default()).unwrap();
        app.memory_state = Some(ajazz_studio::keyboard_memory::MemoryState {
            config: Default::default(),
            scenes: vec![
                Some(program.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            firmware: "1.36".into(),
            build_id: "test".into(),
            runtime: None,
        });
        app.memory_editor_slot = Some(0);
        app.memory_slot = 0;
        app
    }
    #[test]
    fn collection_reserves_new_slots_preserves_drafts_and_never_replaces_a_full_bank() {
        let mut app = app();
        let original = app.memory_state.clone();
        app.memory_drafts.insert(1, Scene::default());
        app.add_collection_scene(collection::all()[0].scene.clone());
        assert_eq!(app.memory_editor_slot, Some(2));
        assert_eq!(app.memory_activation, Some(2));
        assert_eq!(app.memory_changes.len(), 1);
        assert!(app.memory_drafts.contains_key(&1));
        assert_eq!(
            serde_json::to_value(&app.memory_state).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        let program = ajazz_studio::keyboard_memory::compile(&Scene::default()).unwrap();
        app.memory_state.as_mut().unwrap().scenes = vec![Some(program); 8];
        let queued = app.memory_changes.clone();
        let activation = app.memory_activation;
        app.add_collection_scene(collection::all()[1].scene.clone());
        assert_eq!(app.memory_editor_slot, None);
        assert_eq!(app.memory_activation, activation);
        assert_eq!(app.memory_changes.len(), queued.len());
        assert!(app.memory_drafts.contains_key(&2));
        let mut offline = Studio::build(&egui::Context::default(), None, false);
        offline.add_collection_scene(collection::all()[0].scene.clone());
        assert!(offline.memory_changes.is_empty() && offline.memory_activation.is_none());
        // A confirmed deletion also frees its local draft/reservation; it cannot
        // be resurrected by an older autosave or block adding the next look.
        app.memory_state.as_mut().unwrap().scenes[2] = None;
        app.memory_editor_slot = Some(2);
        app.forget_deleted_memory_slot(2);
        assert_eq!(app.vacant_memory_slot(), Some(2));
        assert!(!app.memory_changes.contains_key(&2));
        assert!(!app.memory_drafts.contains_key(&2));
        assert_eq!(app.memory_activation, None);
    }
}

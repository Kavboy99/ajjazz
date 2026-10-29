//! Debounced device changes; drawing and background reads never schedule writes.
use super::*;
use ajazz_studio::keyboard_memory::{self as memory, Operation};

impl Studio {
    fn memory_automatic_operation(&mut self, operation: Operation) {
        self.memory_operation(operation);
        self.memory_automatic = true;
        self.status.clear();
    }
    pub(super) fn hide_to_tray(&mut self, ctx: &egui::Context) {
        if self.tray.is_some() {
            self.hide_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.request_repaint();
        }
    }
    pub(super) fn poll_connections(&mut self) {
        if self.hidden
            || self.exit_requested
            || self.hide_requested
            || self.connection_pending
            || self.busy
            || self.playback_cancel.is_some()
            || self.connection_polled.elapsed() < Duration::from_secs(3)
        {
            return;
        }
        self.connection_polled = Instant::now();
        self.connection_pending = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Connections(
                hardware::scan().map_err(|e| format!("{e:#}")),
            ));
        });
    }
    pub(super) fn flush_automatic_changes(&mut self, ctx: &egui::Context) {
        if self.busy
            || self.playback_cancel.is_some()
            || self.memory_encoder_pending.is_some()
            || ctx.input(|i| i.pointer.any_down())
        {
            return;
        }
        match next_memory_operation(
            &mut self.memory_changes,
            &mut self.memory_activation,
            self.memory_state.as_ref(),
            self.playback_channel == PlaybackChannel::Usb,
            Instant::now(),
            self.exit_requested || self.hide_requested,
        ) {
            Ok(Some(operation)) => {
                self.memory_automatic_operation(operation);
                return;
            }
            Err(error) => {
                self.memory_error = Some(format!("Изменения сохранены в приложении: {error}"));
                return;
            }
            Ok(None) => {}
        }
        if let Some(at) = self.dock_change
            && self.screen_online
            && !self.dock_save_pending
            && self.page_status.is_some()
            && self
                .page_status
                .as_ref()
                .is_none_or(|s| s.erasing.is_none())
            && (self.exit_requested
                || self.hide_requested
                || at.elapsed() >= Duration::from_millis(900))
        {
            self.dock_change = None;
            if self.dock_draft_needs_upgrade() {
                self.dock_save_error = None;
                return;
            }
            let mut config = self.dock_telemetry.clone();
            config.enabled = true;
            let mut pages = self.dock_pages.clone();
            pages.enabled = true;
            if let Err(error) = config.validate().and_then(|_| pages.validate()) {
                self.dock_save_error = Some(error.to_string());
                return;
            }
            self.dock_save_pending = true;
            self.dock_save_error = None;
            if let Some(commands) = &self.screen_commands {
                let _ = commands.send(dock_telemetry::Command::Editor(config, pages, true, None));
            } else {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Event::DockSaved(
                        dock_telemetry::apply_editor(&config, &pages, true)
                            .map_err(|e| format!("{e:#}")),
                    ));
                });
            }
        }
    }
}

/// Drain one settled edit. A selection waits for its own program, and older
/// edits use Upload so they cannot steal the user's latest active mode.
fn next_memory_operation(
    edits: &mut std::collections::BTreeMap<u8, (Instant, Scene)>,
    activate: &mut Option<u8>,
    state: Option<&memory::MemoryState>,
    wired: bool,
    now: Instant,
    force: bool,
) -> anyhow::Result<Option<Operation>> {
    let Some(state) = state.filter(|_| wired) else {
        return Ok(None);
    };
    if let Some((&slot, _)) = edits.iter().find(|(_, (at, _))| {
        force || now.saturating_duration_since(*at) >= Duration::from_millis(900)
    }) {
        let (_, scene) = edits.remove(&slot).unwrap();
        let program = memory::compile(&scene)?;
        if !state
            .scenes
            .get(usize::from(slot))
            .and_then(|p| p.as_ref())
            .is_some_and(|p| memory::same_program(p, &program))
        {
            return Ok(Some(if *activate == Some(slot) {
                *activate = None;
                Operation::Publish(slot, scene)
            } else {
                Operation::Upload(slot, scene)
            }));
        }
    }
    if activate.is_some_and(|s| !edits.contains_key(&s))
        && let Some(slot) = activate.take()
        && state.config.selected != Some(slot)
    {
        return Ok(Some(Operation::Activate(Some(slot))));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> memory::MemoryState {
        memory::MemoryState {
            config: memory::Config::default(),
            scenes: vec![None; 8],
            firmware: String::new(),
            build_id: String::new(),
            runtime: None,
        }
    }
    #[test]
    fn empty_card_is_programmed_before_activation_and_only_latest_edit_is_written() {
        let now = Instant::now();
        let mut edits = std::collections::BTreeMap::new();
        let mut selected = Some(3);
        let mut scene = Scene::default();
        edits.insert(3, (now, scene.clone()));
        scene.name = "latest".into();
        edits.insert(3, (now + Duration::from_millis(700), scene));
        assert!(
            next_memory_operation(
                &mut edits,
                &mut selected,
                Some(&state()),
                true,
                now + Duration::from_millis(1000),
                false
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(selected, Some(3));
        let op = next_memory_operation(
            &mut edits,
            &mut selected,
            Some(&state()),
            true,
            now + Duration::from_millis(1600),
            false,
        )
        .unwrap();
        assert!(matches!(op,Some(Operation::Publish(3,s)) if s.name=="latest"));
        assert!(edits.is_empty());
        assert_eq!(selected, None);
    }
    #[test]
    fn switching_cards_does_not_activate_an_older_pending_edit() {
        let now = Instant::now();
        let mut edits = std::collections::BTreeMap::new();
        let mut selected = Some(5);
        edits.insert(1, (now, Scene::default()));
        edits.insert(5, (now, Scene::default()));
        assert!(matches!(
            next_memory_operation(&mut edits, &mut selected, Some(&state()), true, now, true)
                .unwrap(),
            Some(Operation::Upload(1, _))
        ));
        assert_eq!(selected, Some(5));
        assert!(matches!(
            next_memory_operation(&mut edits, &mut selected, Some(&state()), true, now, true)
                .unwrap(),
            Some(Operation::Publish(5, _))
        ));
        assert!(
            next_memory_operation(&mut edits, &mut selected, Some(&state()), true, now, true)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn unplug_and_identical_edits_do_not_create_flash_writes() {
        let now = Instant::now();
        let mut edits = std::collections::BTreeMap::new();
        let mut selected = Some(1);
        edits.insert(1, (now, Scene::default()));
        assert!(
            next_memory_operation(&mut edits, &mut selected, Some(&state()), false, now, true)
                .unwrap()
                .is_none()
        );
        assert!(
            next_memory_operation(&mut edits, &mut selected, None, true, now, true)
                .unwrap()
                .is_none()
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(selected, Some(1));
        let mut state = state();
        state.scenes[1] = Some(memory::compile(&Scene::default()).unwrap());
        state.config.selected = Some(1);
        assert!(
            next_memory_operation(&mut edits, &mut selected, Some(&state), true, now, true)
                .unwrap()
                .is_none()
        );
        assert!(edits.is_empty());
        assert_eq!(selected, None);
    }
}

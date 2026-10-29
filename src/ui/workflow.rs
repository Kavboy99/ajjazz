use super::*;
use ajazz_studio::keyboard_memory::Operation;

impl Studio {
    pub(super) fn accept_memory_clock(&mut self, runtime: ajazz_studio::keyboard_memory::Runtime) {
        if let Some(state) = &mut self.memory_state {
            state.config.selected = runtime.selected;
            state.config.brightness = runtime.brightness;
            state.config.speed = runtime.speed;
            state.config.paused = runtime.paused;
            state.runtime = Some(runtime.clone());
        }
        self.memory_config.selected = runtime.selected;
        self.memory_config.brightness = runtime.brightness;
        self.memory_config.speed = runtime.speed;
        self.memory_config.paused = runtime.paused;
        self.memory_clock = Some((runtime, Instant::now()));
        self.memory_clock_polled = Instant::now();
        self.memory_clock_failed = false;
    }
    pub(super) fn poll_memory_clock(&mut self, ctx: &egui::Context) {
        if self.busy
            || self.memory_clock_pending
            || self.playback_cancel.is_some()
            || self.memory_encoder_pending.is_some()
            || self.memory_clock_failed
            || self.memory_clock.is_none()
            || self.playback_channel != PlaybackChannel::Usb
            || !matches!(self.page, Page::Lighting | Page::Keyboard)
            || self.page == Page::Keyboard && self.keyboard_tab != KeyboardTab::Encoder
            || self.memory_clock_polled.elapsed() < Duration::from_secs(1)
        {
            return;
        }
        // Schedule after drawing the UI, never in the frame of a user action.
        if ctx.input(|i| {
            i.pointer.any_down()
                || i.pointer.any_pressed()
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
        }) {
            return;
        }
        self.memory_clock_pending = true;
        self.memory_clock_polled = Instant::now();
        let tx = self.tx.clone();
        let generation = self.memory_generation;
        std::thread::spawn(move || {
            let result =
                ajazz_studio::keyboard_memory::runtime_background().map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::KeyboardRuntime(generation, result));
        });
    }
    /// Attempt each missing data source once. A failed read waits for an explicit
    /// refresh; repainting must never create a retry loop against sleeping hardware.
    pub(super) fn load_visible_data(&mut self) {
        if self.busy || self.playback_cancel.is_some() || self.memory_encoder_pending.is_some() {
            return;
        }
        let usb = self.playback_channel == PlaybackChannel::Usb;
        let target = match self.page {
            Page::Lighting
                if !self.dock_surface
                    && self.memory_preview
                    && usb
                    && self.memory_state.is_none() =>
            {
                "memory"
            }
            Page::Lighting
                if !self.dock_surface && !self.memory_preview && self.snapshot.is_none() =>
            {
                "keyboard"
            }
            Page::Keyboard
                if self.keyboard_tab == KeyboardTab::Encoder
                    && usb
                    && self.memory_state.is_none() =>
            {
                "memory"
            }
            Page::Keyboard
                if self.keyboard_tab == KeyboardTab::Macros && self.macro_snapshot.is_none() =>
            {
                "macros"
            }
            Page::Keyboard
                if !matches!(
                    self.keyboard_tab,
                    KeyboardTab::Encoder | KeyboardTab::Calibration | KeyboardTab::Test
                ) && self.snapshot.is_none() =>
            {
                "keyboard"
            }
            Page::Mouse if self.mouse_buttons_tab && self.buttons_snapshot.is_none() => "buttons",
            Page::Mouse if !self.mouse_buttons_tab && self.mouse_snapshot.is_none() => "mouse",
            Page::Dock if self.page_status.is_none() => "dock",
            _ => return,
        };
        if matches!(target, "memory" | "keyboard" | "macros")
            && PlaybackChannel::detect(&self.endpoints).is_none()
        {
            return;
        }
        if !self.auto_reads.insert((target, usb)) {
            return;
        }
        match target {
            "memory" => self.memory_operation(Operation::Read),
            "keyboard" => self.read_snapshot(),
            "macros" => self.read_macros(true),
            "buttons" => self.read_buttons(),
            "mouse" => self.read_mouse(),
            "dock" => self.pages_read(false),
            _ => {}
        }
    }
    pub(super) fn refresh_visible_data(&mut self) {
        if self.busy || self.playback_cancel.is_some() {
            return;
        }
        self.endpoints = hardware::scan().unwrap_or_else(|_| self.endpoints.clone());
        match self.page {
            Page::Lighting if self.memory_preview && !self.dock_surface => {
                self.memory_operation(Operation::Read)
            }
            Page::Keyboard if self.keyboard_tab == KeyboardTab::Encoder => {
                self.memory_operation(Operation::Read)
            }
            Page::Keyboard if self.keyboard_tab == KeyboardTab::Macros => self.read_macros(true),
            Page::Lighting | Page::Keyboard => self.read_snapshot(),
            Page::Mouse if self.mouse_buttons_tab => self.read_buttons(),
            Page::Mouse => self.read_mouse(),
            Page::Dock => self.pages_read(false),
            _ => {}
        }
    }
    pub(super) fn choose_memory_slot(&mut self, slot: usize) {
        if slot >= 8 || self.memory_editor_slot == Some(slot as u8) {
            return;
        }
        let previous = self.memory_editor_slot.unwrap_or(8);
        self.memory_drafts.insert(previous, self.scene.clone());
        let draft = self.memory_drafts.get(&(slot as u8)).cloned().or_else(|| {
            self.memory_state.as_ref()?.scenes[slot]
                .as_ref()
                .and_then(|p| ajazz_studio::keyboard_memory::edit_scene(p).ok())
        });
        self.scene = draft.unwrap_or_else(|| Scene {
            name: "Новый эффект".into(),
            ..Default::default()
        });
        self.memory_slot = slot;
        self.memory_editor_slot = Some(slot as u8);
        self.memory_delete = None;
        self.layer = 0;
        self.time = 0.0;
        self.pulses.clear();
        self.playing = true;
    }
    pub(super) fn memory_draft_changed(&self) -> bool {
        let Some(state) = &self.memory_state else {
            return false;
        };
        ajazz_studio::keyboard_memory::compile(&self.scene).is_ok_and(|p| {
            !state.scenes[self.memory_slot]
                .as_ref()
                .is_some_and(|saved| ajazz_studio::keyboard_memory::same_program(saved, &p))
        })
    }
    pub(super) fn flush_encoder_change(&mut self, ctx: &egui::Context) {
        let Some(changed) = self.memory_encoder_pending else {
            return;
        };
        if !self.busy
            && self.playback_cancel.is_none()
            && !ctx.input(|i| i.pointer.any_down())
            && (changed.elapsed() >= Duration::from_millis(650)
                || self.exit_requested
                || self.hide_requested)
        {
            if self.exit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            self.memory_encoder_pending = None;
            self.memory_operation(Operation::Encoder(
                self.memory_config.encoder,
                self.memory_config.battery.clone(),
            ));
        } else {
            if self.exit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

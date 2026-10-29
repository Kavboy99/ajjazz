use ajazz_dock_core::*;

fn scene(effect: Effect, brightness: u8) -> Scene {
    let palette = Palette::new(
        [
            Color(255, 0, 0),
            Color(0, 255, 255),
            Color::BLACK,
            Color::BLACK,
        ],
        2,
    )
    .unwrap();
    let layer = Layer::new(effect, palette, 2000, 1).unwrap();
    Scene::new([layer; MAX_LAYERS], 1, brightness).unwrap()
}

#[test]
fn rgb565_channels_and_brightness_keep_their_meaning() {
    assert_eq!(Color(255, 0, 0).rgb565(), 0xf800);
    assert_eq!(Color(0, 255, 0).rgb565(), 0x07e0);
    assert_eq!(Color(0, 0, 255).rgb565(), 0x001f);
    assert_eq!(Color(255, 255, 255).rgb565(), 0xffff);
    assert_eq!(Color(120, 64, 255).scale(255), Color(120, 64, 255));
    assert_eq!(Color(120, 64, 255).scale(0), Color::BLACK);
}

#[test]
fn wave_moves_without_usb_and_repeats_after_its_period() {
    let scene = scene(Effect::Wave, 255);
    let mut first = [Color::BLACK; 12];
    let mut middle = first;
    let mut end = first;
    scene.render(0, &mut first).unwrap();
    scene.render(1000, &mut middle).unwrap();
    scene.render(2000, &mut end).unwrap();
    assert_ne!(first, middle);
    assert_eq!(first, end);
    assert_ne!(first[0], first[6]);
}

#[test]
fn palette_wave_is_continuous_across_period_wrap() {
    let scene = scene(Effect::Wave, 255);
    let mut previous = [Color::BLACK; 32];
    scene.render(1983, &mut previous).unwrap();
    for ms in (2000..=2204).step_by(17) {
        let mut current = previous;
        scene.render(ms, &mut current).unwrap();
        for (a, b) in previous.iter().zip(current) {
            assert!(a.0.abs_diff(b.0) <= 10);
            assert!(a.1.abs_diff(b.1) <= 10);
            assert!(a.2.abs_diff(b.2) <= 10);
        }
        previous = current;
    }
}

#[test]
fn masks_opacity_and_global_brightness_apply_in_order() {
    let red = Layer::new(Effect::Solid, Palette::solid(Color(255, 0, 0)), 1000, 1).unwrap();
    let mut blue = Layer::new(Effect::Solid, Palette::solid(Color(0, 0, 255)), 1000, 1).unwrap();
    blue.mask = 1 << 3;
    blue.opacity = 128;
    let scene = Scene::new([red, blue, red, red], 2, 128).unwrap();
    let mut out = [Color::BLACK; 8];
    scene.render(0, &mut out).unwrap();
    assert_eq!(out[0], Color(128, 0, 0));
    assert_eq!(out[3], Color(64, 0, 64));
}

#[test]
fn invalid_led_buffer_is_rejected_before_any_write() {
    let scene = scene(Effect::Wave, 255);
    let mut out = [Color(1, 2, 3); MAX_LEDS + 1];
    assert_eq!(scene.render(0, &mut out), Err(ConfigError::LedCount));
    assert_eq!(out, [Color(1, 2, 3); MAX_LEDS + 1]);
    assert_eq!(scene.render(0, &mut []), Err(ConfigError::LedCount));
}

#[test]
fn millisecond_wrap_does_not_restart_effects() {
    let mut clock = Timeline::new(u32::MAX - 4);
    assert_eq!(clock.advance(10), 15);
    assert_eq!(clock.advance(12), 17);
}

#[test]
fn pacer_delivers_sixty_frames_per_second_and_drops_stale_frames() {
    let mut pacer = FramePacer::new(60).unwrap();
    let count = (0..1000).filter(|ms| pacer.due(*ms)).count();
    assert_eq!(count, 60);
    assert!(pacer.due(5_000));
    assert!(!pacer.due(5_000));
    assert!(!pacer.due(5_001));
    assert!(pacer.due(5_017));
}

#[test]
fn rtc_midnight_and_page_boundaries_are_exact() {
    assert_eq!(
        TimeOfDay::new(23, 59, 59).unwrap().after(1).hms(),
        (0, 0, 0)
    );
    assert_eq!(
        TimeOfDay::new(23, 59, 59).unwrap().after(86_401).hms(),
        (0, 0, 0)
    );
    let red = Page::new(1000, Content::Solid(Color(255, 0, 0))).unwrap();
    let blue = Page::new(2000, Content::Solid(Color(0, 0, 255))).unwrap();
    let pages = Pages::new([red, blue, red, red, red, red, red, red], 2).unwrap();
    assert!(matches!(
        pages.current(999).content,
        Content::Solid(Color(255, 0, 0))
    ));
    assert!(matches!(
        pages.current(1000).content,
        Content::Solid(Color(0, 0, 255))
    ));
    assert!(matches!(
        pages.current(3000).content,
        Content::Solid(Color(255, 0, 0))
    ));
}

struct NoAssets;
impl Assets for NoAssets {
    type Error = u8;
    fn row(
        &mut self,
        _id: u16,
        _frame: u16,
        _width: u16,
        _height: u16,
        _y: u16,
        _output: &mut [u16],
    ) -> Result<(), u8> {
        Err(7)
    }
}

#[test]
fn stored_animation_uses_elapsed_time_and_skips_missed_frames() {
    let clip = Clip::new(4, 10, 33).unwrap();
    assert_eq!(clip.frame(0), 0);
    assert_eq!(clip.frame(32), 0);
    assert_eq!(clip.frame(33), 1);
    assert_eq!(clip.frame(297), 9);
    assert_eq!(clip.frame(330), 0);
    assert_eq!(clip.frame(33_099), 3);
    assert!(matches!(Clip::new(0, 0, 33), Err(ConfigError::Animation)));
    assert!(matches!(Clip::new(0, 1, 0), Err(ConfigError::Animation)));
}

#[test]
fn autonomous_presets_are_distinct_and_animate_at_maximum_led_capacity() {
    use ajazz_dock_core::presets::Preset;
    let mut starts = [[Color::BLACK; MAX_LEDS]; 4];
    for (i, preset) in [
        Preset::Aurora,
        Preset::Embers,
        Preset::Comet,
        Preset::NeonPulse,
    ]
    .into_iter()
    .enumerate()
    {
        let scene = preset.scene().unwrap();
        scene.render(0, &mut starts[i]).unwrap();
        let mut later = starts[i];
        scene.render(1234, &mut later).unwrap();
        assert_ne!(starts[i], later);
    }
    for i in 0..4 {
        for j in (i + 1)..4 {
            assert_ne!(starts[i], starts[j]);
        }
    }
}

#[test]
fn clock_scanline_clears_background_and_asset_failure_is_visible() {
    let style = ClockStyle::new(Color(0, 255, 255), Color::BLACK, 3).unwrap();
    let time = TimeOfDay::new(12, 34, 0).unwrap();
    let mut row = [0xffff; 128];
    scanline(
        Content::Clock(style),
        time,
        128,
        128,
        127,
        &mut row,
        &mut NoAssets,
    )
    .unwrap();
    assert_eq!(row, [0; 128]);
    scanline(
        Content::Clock(style),
        time,
        128,
        128,
        54,
        &mut row,
        &mut NoAssets,
    )
    .unwrap();
    assert!(row.contains(&Color(0, 255, 255).rgb565()));
    assert_eq!(
        scanline(
            Content::Image(0),
            time,
            128,
            128,
            0,
            &mut row,
            &mut NoAssets
        ),
        Err(RenderError::Asset(7))
    );
    assert_eq!(
        scanline(
            Content::Clock(style),
            time,
            8,
            8,
            0,
            &mut row[..8],
            &mut NoAssets
        ),
        Err(RenderError::Config(ConfigError::Geometry))
    );
}

#[test]
fn untrusted_configuration_cannot_create_zero_period_or_empty_schedule() {
    assert!(matches!(
        Palette::new([Color::BLACK; 4], 0),
        Err(ConfigError::Palette)
    ));
    assert!(matches!(
        Layer::new(Effect::Wave, Palette::solid(Color::BLACK), 0, 1),
        Err(ConfigError::Period)
    ));
    assert!(matches!(
        Page::new(0, Content::Solid(Color::BLACK)),
        Err(ConfigError::Duration)
    ));
    assert!(matches!(
        ClockStyle::new(Color::BLACK, Color::BLACK, 0),
        Err(ConfigError::Geometry)
    ));
    assert!(matches!(TimeOfDay::new(24, 0, 0), Err(ConfigError::Clock)));
}

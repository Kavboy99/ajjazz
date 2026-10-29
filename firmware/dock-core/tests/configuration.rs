use ajazz_dock_core::{config::*, *};

fn configuration() -> Configuration {
    let palette = Palette::new(
        [
            Color(10, 20, 30),
            Color(40, 50, 60),
            Color::BLACK,
            Color::BLACK,
        ],
        2,
    )
    .unwrap();
    let mut layer = Layer::new(Effect::Wave, palette, 2250, 3).unwrap();
    layer.mask = 0x8401;
    layer.opacity = 172;
    layer.reverse = true;
    let scene = Scene::new([layer; MAX_LAYERS], 4, 200).unwrap();
    let mut clock = ClockStyle::new(Color(255, 80, 20), Color(2, 4, 8), 2).unwrap();
    clock.blink_colon = true;
    let pages = [
        Page::new(1200, Content::Clock(clock)).unwrap(),
        Page::new(2000, Content::Solid(Color(7, 9, 11))).unwrap(),
        Page::new(3000, Content::Image(123)).unwrap(),
        Page::new(4000, Content::Animation(Clip::new(42, 48, 33).unwrap())).unwrap(),
        Page::new(1200, Content::Clock(clock)).unwrap(),
        Page::new(2000, Content::Solid(Color(7, 9, 11))).unwrap(),
        Page::new(3000, Content::Image(123)).unwrap(),
        Page::new(4000, Content::Animation(Clip::new(42, 48, 33).unwrap())).unwrap(),
    ];
    Configuration::new(scene, Pages::new(pages, 8).unwrap(), 60).unwrap()
}

fn encoded() -> Vec<u8> {
    let config = configuration();
    let mut out = vec![0; config.encoded_len()];
    config.encode(&mut out).unwrap();
    out
}

// Independent conventional IEEE CRC implementation for malformed fixtures.
fn seal(data: &mut [u8]) {
    let mut crc = 0xffff_ffff_u32;
    for byte in data[..8].iter().chain(data[12..].iter()) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    data[8..12].copy_from_slice(&(!crc).to_le_bytes());
}

#[test]
fn complete_configuration_roundtrip_keeps_rendering_and_canonical_bytes() {
    let input = encoded();
    assert_eq!(input.len(), MAX_CONFIG_BYTES);
    let decoded = Configuration::decode(&input).unwrap();
    assert_eq!(decoded.fps(), 60);
    let mut reencoded = vec![0; input.len()];
    decoded.encode(&mut reencoded).unwrap();
    assert_eq!(input, reencoded);
    let original = configuration();
    for time in [0, 33, 1200, 3200, 6200, 10199, 10200, u32::MAX as u64] {
        let mut a = [Color::BLACK; 16];
        let mut b = a;
        original.scene.render(time, &mut a).unwrap();
        decoded.scene.render(time, &mut b).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            format!("{:?}", original.pages.at(time).page),
            format!("{:?}", decoded.pages.at(time).page)
        );
        assert_eq!(
            original.pages.at(time).elapsed_ms,
            decoded.pages.at(time).elapsed_ms
        );
    }
}

#[test]
fn all_truncations_extensions_and_single_bit_changes_are_rejected() {
    let input = encoded();
    for end in 0..input.len() {
        assert!(
            Configuration::decode(&input[..end]).is_err(),
            "prefix {end}"
        );
    }
    let mut extended = input.clone();
    extended.push(0);
    assert!(Configuration::decode(&extended).is_err());
    for offset in 0..input.len() {
        for bit in 0..8 {
            let mut damaged = input.clone();
            damaged[offset] ^= 1 << bit;
            assert!(
                Configuration::decode(&damaged).is_err(),
                "byte {offset}, bit {bit}"
            );
        }
    }
}

#[test]
fn invalid_semantics_with_valid_checksum_never_create_configuration() {
    // fps, layer/page count, effect, palette length, period, repetitions, boolean,
    // page kind, clock scale and boolean, asset reserved bytes, clip frames/time.
    for (offset, value) in [
        (12, 0),
        (12, 61),
        (14, 0),
        (14, 5),
        (15, 0),
        (15, 9),
        (16, 255),
        (17, 0),
        (17, 5),
        (24, 1),
        (34, 0),
        (34, 17),
        (35, 2),
        (136, 255),
        (143, 0),
        (144, 2),
        (153, 1),
        (165, 1),
        (179, 255),
        (181, 255),
        (182, 255),
    ] {
        let mut input = encoded();
        input[offset] = value;
        seal(&mut input);
        assert!(
            Configuration::decode(&input).is_err(),
            "accepted offset {offset}={value}"
        );
    }
    for period in [0_u32, 99, 60_001, u32::MAX] {
        let mut input = encoded();
        input[30..34].copy_from_slice(&period.to_le_bytes());
        seal(&mut input);
        assert!(Configuration::decode(&input).is_err());
    }
}

#[test]
fn short_encode_leaves_buffer_untouched_and_does_not_expose_inactive_palette() {
    let config = configuration();
    let mut short = vec![0x7f; config.encoded_len() - 1];
    assert_eq!(config.encode(&mut short), Err(DecodeError::Length));
    assert!(short.iter().all(|b| *b == 0x7f));
    let layer = Layer::new(Effect::Solid, Palette::solid(Color(1, 2, 3)), 1000, 1).unwrap();
    let config =
        Configuration::new(Scene::new([layer; 4], 1, 255).unwrap(), config.pages, 30).unwrap();
    let mut out = [0xaa; MAX_CONFIG_BYTES + 8];
    let len = config.encode(&mut out).unwrap();
    assert_eq!(&out[21..30], &[0; 9]);
    assert!(out[len..].iter().all(|b| *b == 0xaa));
    assert!(Configuration::decode(&out[..len]).is_ok());
}

#[test]
fn random_bounded_inputs_do_not_panic() {
    let mut seed = 0xa513_76e9_u32;
    for len in 0..=MAX_CONFIG_BYTES + 1 {
        let mut input = vec![0; len];
        for byte in &mut input {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        let _ = Configuration::decode(&input);
    }
    // Also pass the envelope checks, exercising the actual record parser.
    for _ in 0..2048 {
        let mut input = encoded();
        for byte in &mut input[12..] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        input[14] = 4;
        input[15] = 8;
        seal(&mut input);
        let _ = Configuration::decode(&input);
    }
}

//! Authored multi-layer programs for the installed AJKS renderer; no new opcodes.
use crate::{keyboard_memory, model::Scene};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub id: String,
    pub name: String,
    pub family: String,
    pub description: String,
    pub scene: Scene,
}
pub fn all() -> &'static [Look] {
    static LOOKS: OnceLock<Vec<Look>> = OnceLock::new();
    LOOKS.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/lighting-collection.json"))
            .expect("Embedded lighting collection")
    })
}
/// Offline motion studies from the exact integer renderer used by the keyboard.
pub fn audit(input: &Path, output: &Path) -> Result<()> {
    let looks: Vec<Look> = crate::storage::read_json(input, 2_000_000)?;
    std::fs::create_dir_all(output)?;
    let leds = crate::lighting::keyboard_leds();
    let mut report = Vec::new();
    for look in looks {
        ensure!(
            !look.id.is_empty()
                && look.id.len() <= 48
                && look
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
            "Invalid study ID"
        );
        let page = keyboard_memory::compile(&look.scene)?;
        let mut frames = Vec::new();
        for frame in 0..180 {
            let time = frame as f32 / 15.0;
            let pulses =
                [(0.3, 0.4, 0.0), (0.7, 0.6, 0.6), (0.5, 0.2, 1.2)].map(|(x, y, phase)| {
                    crate::lighting::Pulse {
                        x,
                        y,
                        time: ((time - phase) / 1.8).floor() * 1.8 + phase,
                    }
                });
            let colors =
                keyboard_memory::device_preview(&look.scene, time, &pulses, 255, 128, true)?;
            frames.push(colors.into_iter().flatten().collect::<Vec<u8>>());
        }
        let mut vectors = Vec::new();
        for frame in [1u32, 73, 247, 585] {
            let mut pressed = [None; 128];
            pressed[34] = Some((frame - 1) as u16);
            let rgb = keyboard_memory::render_program(&page, frame, &pressed, Some(34), 255, 128)?;
            vectors.push(serde_json::json!({"frame":frame,"rgb":rgb}));
        }
        let study = serde_json::json!({"look":look,"fps":15,"frames":frames,"program":page,"vectors":vectors,"key_ids":leds.iter().map(|l|l.id).collect::<Vec<_>>(),"hardware_access":false});
        std::fs::write(
            output.join(format!("{}.json", look.id)),
            serde_json::to_vec(&study)?,
        )?;
        report.push(look.id);
    }
    std::fs::write(
        output.join("index.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_collection_is_bounded_distinct_and_roundtrips_on_existing_protocol() {
        let mut ids = std::collections::BTreeSet::new();
        let mut programs = std::collections::BTreeSet::new();
        let mut motions = std::collections::BTreeSet::new();
        assert!(all().len() >= 16);
        for look in all() {
            assert!(ids.insert(&look.id));
            assert_eq!(look.name, look.scene.name);
            assert!(
                look.name.len() <= 32,
                "Name must survive the keyboard's UTF-8 field"
            );
            assert!((2..=5).contains(&look.scene.layers.len()));
            let page = keyboard_memory::compile(&look.scene).unwrap();
            let decoded = keyboard_memory::edit_scene(&page).unwrap();
            assert!(
                keyboard_memory::same_program(&page, &keyboard_memory::compile(&decoded).unwrap()),
                "{} direction/parameters changed during readback",
                look.id
            );
            assert!(
                programs.insert(page[64..448].to_vec()),
                "Duplicate motion program"
            );
            let mut motion = page[64..448].to_vec();
            for layer in motion.as_chunks_mut::<48>().0 {
                layer[4..10].fill(0);
            }
            assert!(
                motions.insert(motion),
                "The collection must not contain palette-only duplicates"
            );
        }
    }
}

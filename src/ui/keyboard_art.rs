//! Shared drawing geometry; firmware coordinates and matrix IDs stay unchanged.
use super::*;

pub(super) struct Geometry {
    pub keys: Vec<Rect>,
    pub unit: f32,
    body: Rect,
    knob: Pos2,
}
pub(super) fn geometry(bounds: Rect) -> Geometry {
    let unit = (bounds.width() / 17.5).min(bounds.height() / 7.2);
    let body = Rect::from_center_size(bounds.center(), Vec2::new(unit * 17.5, unit * 7.2));
    let origin = body.min + Vec2::splat(unit * 0.35);
    let mut keys = Vec::with_capacity(82);
    for (row, items) in keyboard_rows().iter().enumerate() {
        let y = if row == 0 {
            0.0
        } else {
            1.4 + (row - 1) as f32 * 1.06
        };
        let mut x = 0.0;
        for (column, key) in items.iter().enumerate() {
            if row == 0 && matches!(column, 1 | 5 | 9 | 13) {
                x += 0.25;
            }
            if (1..=4).contains(&row) && column == items.len() - 1
                || row == 5 && key.physical == "ArrowLeft"
            {
                x += 0.25;
            }
            let width = if key.physical == "CapsLock" {
                1.75
            } else {
                key.width
            };
            keys.push(Rect::from_min_size(
                origin + Vec2::new(x * unit, y * unit),
                Vec2::new((width - 0.12) * unit, 0.88 * unit),
            ));
            x += width;
        }
    }
    Geometry {
        keys,
        unit,
        body,
        knob: origin + Vec2::new(16.0 * unit, 0.44 * unit),
    }
}
pub(super) fn case(p: &egui::Painter, g: &Geometry) {
    p.rect_filled(g.body, g.unit * 0.32, Color32::from_rgb(36, 41, 50));
    p.rect_stroke(
        g.body,
        g.unit * 0.32,
        Stroke::new(
            (g.unit * 0.045).clamp(0.5, 1.0),
            Color32::from_rgb(73, 81, 95),
        ),
        StrokeKind::Inside,
    );
    p.circle_filled(g.knob, g.unit * 0.44, Color32::from_rgb(65, 72, 84));
    p.circle_stroke(
        g.knob,
        g.unit * 0.34,
        Stroke::new((g.unit * 0.05).max(0.5), Color32::from_rgb(112, 123, 139)),
    );
    p.line_segment(
        [
            g.knob - Vec2::new(0.0, g.unit * 0.25),
            g.knob - Vec2::new(0.0, g.unit * 0.37),
        ],
        Stroke::new((g.unit * 0.065).max(0.7), Color32::from_rgb(210, 220, 235)),
    );
}
pub(super) fn miniature(ui: &egui::Ui, tile: Rect, colors: &[[u8; 3]]) {
    let p = ui.painter().with_clip_rect(tile.intersect(ui.clip_rect()));
    p.rect_filled(tile, 12.0, Color32::from_rgb(8, 12, 19));
    let g = geometry(tile.shrink(tile.width() * 0.055));
    case(&p, &g);
    for (rect, color) in g.keys.iter().zip(colors) {
        let color = Color32::from_rgb(color[0].max(22), color[1].max(26), color[2].max(34));
        p.rect_filled(*rect, g.unit * 0.15, color);
        p.rect_stroke(
            *rect,
            g.unit * 0.15,
            Stroke::new((g.unit * 0.1).clamp(0.35, 0.8), color.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_key_hit_targets_and_encoder_fit_without_overlaps_at_both_sizes() {
        for size in [Vec2::splat(88.0), Vec2::new(510.0, 209.1)] {
            let g = geometry(Rect::from_min_size(Pos2::ZERO, size));
            assert_eq!(g.keys.len(), 82);
            for (index, key) in g.keys.iter().enumerate() {
                assert!(g.body.contains_rect(*key), "Key {index} outside case");
                for other in g.keys.iter().skip(index + 1) {
                    assert!(!key.intersects(*other), "Overlapping key hit targets");
                }
                assert!(
                    !key.intersects(Rect::from_center_size(g.knob, Vec2::splat(g.unit * 0.88))),
                    "Key overlaps encoder"
                );
            }
            let layout: Vec<_> = keyboard_rows().into_iter().flatten().collect();
            let space = layout.iter().position(|k| k.physical == "Space").unwrap();
            assert!(
                g.keys[space].width() > g.unit * 6.0,
                "Spacebar must be recognizable"
            );
        }
    }
}

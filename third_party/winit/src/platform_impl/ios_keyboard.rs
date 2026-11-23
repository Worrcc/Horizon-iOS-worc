//! Horizon: keyboard avoidance in a single coordinate space (UIKit points on iOS).

/// Upward offset needed to put `field` above `keyboard`; rectangles are [x, y, width, height].
/// A hidden keyboard (empty or outside the field) needs no offset. For a floating keyboard,
/// reserve everything below its top, rather than just its height, to keep content above it.
pub(super) fn keyboard_offset(field: [f64; 4], keyboard: [f64; 4]) -> f64 {
    let [x, y, width, height] = field;
    let [kx, ky, kw, kh] = keyboard;
    if width <= 0.0
        || height <= 0.0
        || kw <= 0.0
        || kh <= 0.0
        || x + width <= kx
        || kx + kw <= x
        || y + height <= ky
        || ky + kh <= y
    {
        return 0.0;
    }
    (y + height - ky).max(0.0)
}

/// Portion of a viewport hidden below the keyboard top, bounded by its own height.
pub(super) fn visible_bottom_inset(view: [f64; 4], keyboard: [f64; 4]) -> f64 {
    keyboard_offset(view, keyboard).min(view[3].max(0.0))
}

#[cfg(test)]
mod tests {
    use super::{keyboard_offset, visible_bottom_inset};

    #[test]
    fn covered_field_moves_just_above_keyboard() {
        let field = [10.0, 350.0, 300.0, 30.0];
        let keyboard = [0.0, 300.0, 800.0, 300.0];
        let offset = keyboard_offset(field, keyboard);
        assert_eq!(offset, 80.0);
        assert_eq!(field[1] + field[3] - offset, keyboard[1]);
        assert_eq!(keyboard_offset([10.0, 280.0, 300.0, 30.0], keyboard), 10.0);
    }

    #[test]
    fn visible_field_and_hidden_or_disjoint_keyboard_do_not_move() {
        let field = [10.0, 250.0, 300.0, 30.0];
        for keyboard in [
            [0.0, 280.0, 800.0, 300.0], // Touching edges are not overlap.
            [0.0, 600.0, 800.0, 300.0], // Dismissed below the window.
            [0.0, 0.0, 0.0, 0.0],
            [310.0, 200.0, 200.0, 200.0], // Floating beside the field.
            [0.0, 0.0, 800.0, 250.0],
        ] {
            assert_eq!(keyboard_offset(field, keyboard), 0.0);
        }
    }

    #[test]
    fn viewport_inset_tracks_local_origin_and_restores_after_dismissal() {
        // A view at screen y=40 sees a screen keyboard at y=400 at local y=360.
        assert_eq!(
            visible_bottom_inset([0.0, 0.0, 800.0, 600.0], [-20.0, 360.0, 900.0, 240.0]),
            240.0
        );
        let view = [0.0, 0.0, 800.0, 600.0];
        for keyboard in [
            [0.0, 600.0, 800.0, 300.0],
            [800.0, 200.0, 100.0, 100.0],
            [0.0, 0.0, 0.0, 0.0],
        ] {
            assert_eq!(visible_bottom_inset(view, keyboard), 0.0);
        }
        assert_eq!(
            visible_bottom_inset(view, [0.0, -100.0, 900.0, 800.0]),
            600.0
        );
    }

    #[test]
    fn viewport_reserves_space_above_docked_and_floating_keyboards() {
        let viewport = [20.0, 40.0, 800.0, 600.0];
        assert_eq!(
            visible_bottom_inset(viewport, [0.0, 400.0, 900.0, 240.0]),
            240.0
        );
        assert_eq!(
            visible_bottom_inset(viewport, [100.0, 400.0, 300.0, 100.0]),
            240.0
        );
        assert_eq!(
            visible_bottom_inset(viewport, [0.0, 0.0, 900.0, 700.0]),
            600.0
        );
    }
}

//! iOS keyboard layout in physical pixels, independent of UIKit and the renderer.

/// Preserve the normal GUI without a keyboard. With one, fit the GUI above its top.
/// Keep at least 240 GUI pixels for editors with fixed Y positions (signs, commands),
/// allowing a fractional scale only when even scale 1 would not fit.
/// The GPU still maps these vertices onto the full surface using the returned scale.
pub(super) fn layout(window: [u32; 2], inset: u32, normal: (f32, f32, f32)) -> (f32, f32, f32) {
    if inset == 0 {
        return normal;
    }
    let visible = window[1].saturating_sub(inset).max(1) as f32;
    let fit = visible / 240.0;
    let scale = normal.0.min(fit.floor().max(1.0)).min(fit);
    (
        scale,
        (window[0] as f32 / scale).ceil(),
        (visible / scale).floor(),
    )
}

#[cfg(test)]
mod tests {
    use super::layout;

    #[test]
    fn hidden_keyboard_preserves_existing_gui_exactly() {
        let normal = (3.0, 844.0, 390.0);
        assert_eq!(layout([2532, 1170], 0, normal), normal);
    }

    #[test]
    fn phone_chat_sign_anvil_and_command_fields_fit_above_keyboard() {
        // Landscape/portrait at different Retina scales, including a very short strip
        // and a floating keyboard (the inset includes the space below its top).
        for (window, inset, normal) in [
            ([2532, 1170], 660, (4.0, 633.0, 293.0)),
            ([1170, 2532], 900, (3.0, 390.0, 844.0)),
            ([1136, 640], 480, (2.0, 568.0, 320.0)),
            ([2048, 1536], 1300, (6.0, 342.0, 256.0)),
            ([2532, 1170], 1001, (1.0, 2532.0, 1170.0)),
        ] {
            let (scale, _, height) = layout(window, inset, normal);
            let visible = (window[1] - inset) as f32;
            // Bottom of chat, last hanging-sign line, anvil rename box, command box.
            for bottom in [height, 125.0 + 20.0, (height - 166.0) / 2.0 + 36.0, 70.0] {
                assert!(bottom * scale <= visible + 0.001);
            }
            assert!(height >= 240.0);
            assert!(scale <= normal.0);
        }
    }

    #[test]
    fn rounding_never_places_chat_below_the_visible_edge() {
        for visible in 1..1170 {
            let (scale, _, height) = layout([2532, 1170], 1170 - visible, (4.0, 633.0, 293.0));
            assert!(height * scale <= visible as f32 + 0.001);
        }
    }

    #[test]
    fn full_occlusion_does_not_produce_zero_scale_or_nonfinite_geometry() {
        for inset in [640, 800, u32::MAX] {
            let (scale, width, height) = layout([1136, 640], inset, (2.0, 568.0, 320.0));
            assert!(scale > 0.0 && scale.is_finite());
            assert!(width.is_finite() && height.is_finite());
        }
    }
}

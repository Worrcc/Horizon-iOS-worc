//! Pure aim assistance for caller-supplied, visible targets. All angles are in
//! degrees: yaw 0 faces south and 90 west; pitch -90 faces up and 90 down.

/// Direction from the player's eyes to the nearest point of a target's hitbox.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub yaw: f32,
    pub pitch: f32,
    /// Distance in blocks. The caller filters range and line of sight.
    pub distance: f32,
}

/// Aim assistance tuning. Use [`Config::clamped`] to obtain valid settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Range in blocks, clamped to 2..=32.
    pub range: f32,
    /// Angular radius in degrees, clamped to 1..=10.
    pub cone: f32,
    /// Look slowdown at the centre, clamped to 0..=0.8.
    pub slowdown: f32,
    /// Pull in degrees per second, clamped to 0..=20.
    pub pull: f32,
}

impl Config {
    /// Clamp every setting to its supported range, treating non-finite values
    /// as zero before clamping.
    pub fn clamped(self) -> Self {
        Self {
            range: finite_or_zero(self.range).clamp(2.0, 32.0),
            cone: finite_or_zero(self.cone).clamp(1.0, 10.0),
            slowdown: finite_or_zero(self.slowdown).clamp(0.0, 0.8),
            pull: finite_or_zero(self.pull).clamp(0.0, 20.0),
        }
    }
}

impl Default for Config {
    /// The Medium preset.
    fn default() -> Self {
        Self {
            range: 16.0,
            cone: 6.0,
            slowdown: 0.5,
            pull: 8.0,
        }
    }
}

/// Whether a finite distance is within the configured range (inclusive).
pub fn target_ok(distance: f32, cfg: &Config) -> bool {
    distance.is_finite() && distance <= cfg.clamped().range
}

/// Adjust a requested (yaw, pitch) turn, leaving a still view untouched.
///
/// Angular distance is the length of the shortest yaw/pitch offset. Both
/// slowdown and pull fade linearly to zero at the cone edge. Pull follows the
/// target offset and is capped by the distance remaining along that direction
/// after slowdown; it never adds motion past the target. The player's own turn
/// can still pass the target. Range and visibility filtering belong to the caller.
/// Non-finite turn input returns (0, 0); other non-finite values act as zero.
pub fn assist(
    look: (f32, f32),
    input: (f32, f32),
    seconds: f32,
    targets: &[Target],
    cfg: &Config,
) -> (f32, f32) {
    if !input.0.is_finite() || !input.1.is_finite() {
        return (0.0, 0.0);
    }
    if input.0.abs() < 1e-6 && input.1.abs() < 1e-6 {
        return input;
    }

    let cfg = cfg.clamped();
    // Widen before arithmetic so even extreme finite f32 inputs cannot overflow.
    let look = (
        f64::from(finite_or_zero(look.0)),
        f64::from(finite_or_zero(look.1)),
    );
    let cone = f64::from(cfg.cone);
    let nearest = targets
        .iter()
        .filter_map(|target| {
            let yaw =
                f64::from(finite_or_zero(target.yaw)).rem_euclid(360.0) - look.0.rem_euclid(360.0);
            let yaw = (yaw + 180.0).rem_euclid(360.0) - 180.0;
            let pitch = f64::from(finite_or_zero(target.pitch)) - look.1;
            let angle = yaw.hypot(pitch);
            (angle <= cone).then_some((yaw, pitch, angle))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    let Some((yaw, pitch, angle)) = nearest else {
        return input;
    };

    let falloff = 1.0 - angle / cone;
    let speed = 1.0 - f64::from(cfg.slowdown) * falloff;
    let input = (f64::from(input.0), f64::from(input.1));
    let mut adjusted = (input.0 * speed, input.1 * speed);
    if angle > 0.0 && falloff > 0.0 {
        let direction = (yaw / angle, pitch / angle);
        let remaining = (angle - adjusted.0 * direction.0 - adjusted.1 * direction.1).max(0.0);
        let seconds = f64::from(finite_or_zero(seconds)).clamp(0.0, 0.1);
        let input_scale = (input.0.hypot(input.1) / 2.0).min(1.0);
        let pull = (f64::from(cfg.pull) * seconds * input_scale * falloff).min(remaining);
        adjusted.0 += direction.0 * pull;
        adjusted.1 += direction.1 * pull;
    }
    let limit = f64::from(f32::MAX);
    (
        adjusted.0.clamp(-limit, limit) as f32,
        adjusted.1.clamp(-limit, limit) as f32,
    )
}

fn finite_or_zero(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(yaw: f32, pitch: f32) -> Target {
        Target {
            yaw,
            pitch,
            distance: 4.0,
        }
    }

    fn assert_turn(actual: (f32, f32), expected: (f32, f32)) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-6 && (actual.1 - expected.1).abs() < 1e-6,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn no_input_is_returned_unchanged() {
        for input in [(0.0, -0.0), (0.0000005, -0.0000009)] {
            let result = assist(
                (0.0, 0.0),
                input,
                0.1,
                &[target(1.0, 1.0)],
                &Config::default(),
            );
            assert_eq!(result.0.to_bits(), input.0.to_bits());
            assert_eq!(result.1.to_bits(), input.1.to_bits());
        }
    }

    #[test]
    fn no_target_in_cone_leaves_input_unchanged() {
        for targets in [&[][..], &[target(20.0, 0.0)][..]] {
            assert_eq!(
                assist((0.0, 0.0), (2.0, -1.0), 0.1, targets, &Config::default()),
                (2.0, -1.0)
            );
        }
    }

    #[test]
    fn dead_centre_halves_input_exactly() {
        assert_eq!(
            assist(
                (12.0, -30.0),
                (2.0, -4.0),
                0.1,
                &[target(12.0, -30.0)],
                &Config::default()
            ),
            (1.0, -2.0)
        );
    }

    #[test]
    fn cone_edge_has_no_slowdown_or_pull() {
        assert_eq!(
            assist(
                (0.0, 0.0),
                (2.0, -1.0),
                0.1,
                &[target(6.0, 0.0)],
                &Config::default()
            ),
            (2.0, -1.0)
        );
    }

    #[test]
    fn pull_is_capped_and_fades_with_angle() {
        let cfg = Config {
            slowdown: 0.0,
            ..Config::default()
        };
        // Halfway to the cone edge: 8 deg/s * 0.1 s * 1/2 = 0.4 degrees.
        let result = assist((0.0, 0.0), (2.0, 0.0), 0.1, &[target(3.0, 0.0)], &cfg);
        assert_turn(result, (2.4, 0.0));
        assert!(result.0 - 2.0 <= 0.8);
    }

    #[test]
    fn pull_never_overshoots_remaining_offset() {
        let cfg = Config {
            slowdown: 0.0,
            pull: 20.0,
            ..Config::default()
        };
        // Asking for -2 gives full pull toward +0.1: two degrees cannot reach it.
        assert_turn(
            assist((0.0, 0.0), (-2.0, 0.0), 0.1, &[target(0.1, 0.0)], &cfg),
            (-1.0 / 30.0, 0.0),
        );
        // A perpendicular input also gives full pull, capped at the 0.1 yaw gap.
        assert_eq!(
            assist((0.0, 0.0), (0.0, 2.0), 0.1, &[target(0.1, 0.0)], &cfg),
            (0.1, 2.0)
        );
        // Slow input leaves only 0.02 degrees to the target; cap the added turn.
        assert_eq!(
            assist((0.0, 0.0), (0.08, 0.0), 0.1, &[target(0.1, 0.0)], &cfg),
            (0.1, 0.0)
        );
        // The player's own overshoot must not receive additional forward pull.
        assert_eq!(
            assist((0.0, 0.0), (2.0, 0.0), 0.1, &[target(0.1, 0.0)], &cfg),
            (2.0, 0.0)
        );
    }

    #[test]
    fn yaw_wrap_uses_shortest_offset_in_both_directions() {
        let cfg = Config {
            cone: 4.0,
            slowdown: 0.0,
            ..Config::default()
        };
        assert_turn(
            assist((359.0, 0.0), (-2.0, 0.0), 0.1, &[target(1.0, 0.0)], &cfg),
            (-1.6, 0.0),
        );
        assert_turn(
            assist((1.0, 0.0), (2.0, 0.0), 0.1, &[target(359.0, 0.0)], &cfg),
            (1.6, 0.0),
        );
    }

    #[test]
    fn pointing_input_at_outside_target_does_not_select_it() {
        assert_eq!(
            assist(
                (0.0, 0.0),
                (7.0, 0.0),
                0.1,
                &[target(7.0, 0.0)],
                &Config::default()
            ),
            (7.0, 0.0)
        );
    }

    #[test]
    fn nearest_angular_target_wins_regardless_of_order_or_distance() {
        let cfg = Config {
            pull: 0.0,
            ..Config::default()
        };
        let far_angle = Target {
            distance: 1.0,
            ..target(-4.0, 0.0)
        };
        let near_angle = Target {
            distance: 10.0,
            ..target(2.0, 0.0)
        };
        // Angle 2 / cone 6 gives falloff 2/3 and speed 1 - (1/2 * 2/3) = 2/3.
        for targets in [[far_angle, near_angle], [near_angle, far_angle]] {
            assert_turn(
                assist((0.0, 0.0), (3.0, -3.0), 0.1, &targets, &cfg),
                (2.0, -2.0),
            );
        }
    }

    #[test]
    fn combined_yaw_pitch_distance_defines_cone() {
        let cfg = Config {
            cone: 5.0,
            pull: 0.0,
            ..Config::default()
        };
        // A 3-4-5 triangle lies exactly on the edge. (4, 4) lies outside it.
        for offset in [(3.0, 4.0), (4.0, 4.0)] {
            assert_eq!(
                assist(
                    (0.0, 0.0),
                    (2.0, -1.0),
                    0.1,
                    &[target(offset.0, offset.1)],
                    &cfg
                ),
                (2.0, -1.0)
            );
        }
    }

    #[test]
    fn diagonal_pull_follows_target_offset() {
        let cfg = Config {
            cone: 10.0,
            slowdown: 0.0,
            pull: 20.0,
            ..Config::default()
        };
        // Offset (3, -4), length 5, falloff 1/2: one degree along (3/5, -4/5).
        assert_turn(
            assist((0.0, 0.0), (0.0, 2.0), 0.1, &[target(3.0, -4.0)], &cfg),
            (0.6, 1.2),
        );
    }

    #[test]
    fn tiny_input_scales_pull_by_its_length() {
        let cfg = Config {
            slowdown: 0.0,
            ..Config::default()
        };
        // Input length 0.5 gives 1/4 strength; falloff at 3 degrees is 1/2.
        assert_turn(
            assist((0.0, 0.0), (0.3, 0.4), 0.1, &[target(3.0, 0.0)], &cfg),
            (0.4, 0.4),
        );
    }

    #[test]
    fn frame_time_is_clamped() {
        let cfg = Config {
            slowdown: 0.0,
            ..Config::default()
        };
        for (seconds, expected) in [(-1.0, 2.0), (0.0, 2.0), (0.05, 2.2), (1.0, 2.4)] {
            assert_turn(
                assist((0.0, 0.0), (2.0, 0.0), seconds, &[target(3.0, 0.0)], &cfg),
                (expected, 0.0),
            );
        }
    }

    #[test]
    fn clamped_enforces_all_documented_ranges() {
        assert_eq!(
            Config {
                range: -1.0,
                cone: 20.0,
                slowdown: -0.5,
                pull: 30.0
            }
            .clamped(),
            Config {
                range: 2.0,
                cone: 10.0,
                slowdown: 0.0,
                pull: 20.0
            }
        );
        assert_eq!(
            Config {
                range: 100.0,
                cone: 0.0,
                slowdown: 1.0,
                pull: -1.0
            }
            .clamped(),
            Config {
                range: 32.0,
                cone: 1.0,
                slowdown: 0.8,
                pull: 0.0
            }
        );
        assert_eq!(
            Config::default().clamped(),
            Config {
                range: 16.0,
                cone: 6.0,
                slowdown: 0.5,
                pull: 8.0
            }
        );
    }

    #[test]
    fn target_ok_checks_finite_distance_and_inclusive_range() {
        let cfg = Config::default();
        for distance in [0.0, 15.0, 16.0] {
            assert!(target_ok(distance, &cfg));
        }
        for distance in [16.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!target_ok(distance, &cfg));
        }
        assert!(target_ok(
            32.0,
            &Config {
                range: 100.0,
                ..cfg
            }
        ));
        assert!(!target_ok(
            32.01,
            &Config {
                range: 100.0,
                ..cfg
            }
        ));
    }

    #[test]
    fn non_finite_turn_input_returns_zero() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for input in [(bad, 1.0), (1.0, bad), (bad, bad)] {
                assert_eq!(
                    assist(
                        (0.0, 0.0),
                        input,
                        0.1,
                        &[target(1.0, 0.0)],
                        &Config::default()
                    ),
                    (0.0, 0.0)
                );
            }
        }
    }

    #[test]
    fn other_non_finite_values_act_as_zero() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                assist(
                    (bad, bad),
                    (2.0, -4.0),
                    bad,
                    &[target(bad, bad)],
                    &Config::default()
                ),
                (1.0, -2.0)
            );
            assert_eq!(
                Config {
                    range: bad,
                    cone: bad,
                    slowdown: bad,
                    pull: bad
                }
                .clamped(),
                Config {
                    range: 2.0,
                    cone: 1.0,
                    slowdown: 0.0,
                    pull: 0.0
                }
            );
            assert_turn(
                assist(
                    (0.0, 0.0),
                    (3.0, 0.0),
                    bad,
                    &[target(2.0, 0.0)],
                    &Config::default(),
                ),
                (2.0, 0.0),
            );
        }
    }

    #[test]
    fn extreme_finite_values_do_not_overflow() {
        let cfg = Config {
            slowdown: 0.0,
            ..Config::default()
        };
        assert_eq!(
            assist(
                (0.0, 0.0),
                (f32::MAX, f32::MAX),
                0.1,
                &[target(1.0, 1.0)],
                &cfg
            ),
            (f32::MAX, f32::MAX)
        );
        assert_eq!(
            assist(
                (f32::MAX, -f32::MAX),
                (2.0, 1.0),
                0.1,
                &[target(-f32::MAX, f32::MAX)],
                &cfg
            ),
            (2.0, 1.0)
        );
    }
}

//! Runtime overrides: settings on disk always remain the player's choices.
use crate::ThermalState;
use std::time::Duration;

#[derive(Debug, Default)]
pub struct Governor {
    pub state: ThermalState,
    cooling: Option<(ThermalState, Duration)>,
}
impl Governor {
    /// Escalate immediately; require 30 uninterrupted seconds in the same lower state to relax.
    pub fn update(&mut self, observed: ThermalState, now: Duration) {
        if observed >= self.state {
            self.state = observed;
            self.cooling = None;
        } else {
            let (candidate, since) = self.cooling.get_or_insert((observed, now));
            if *candidate != observed {
                *candidate = observed;
                *since = now;
            }
            if now.saturating_sub(*since) >= Duration::from_secs(30) {
                self.state = observed;
                self.cooling = None;
            }
        }
    }

    pub fn frame_cap(&self, player: i32) -> i32 {
        match self.state {
            ThermalState::Serious => player.min(60),
            ThermalState::Critical => player.min(30),
            _ => player,
        }
    }

    pub fn resolution_scale(&self, player: u32) -> u32 {
        match self.state {
            ThermalState::Serious => player.saturating_sub(15).max(50),
            ThermalState::Critical => 50,
            _ => player,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ThermalState::*;

    #[test]
    fn heat_escalates_immediately_and_cooling_requires_a_stable_state() {
        let mut g = Governor::default();
        for (seconds, observed, expected) in [
            (0, Fair, Fair),
            (1, Serious, Serious),
            (2, Critical, Critical),
            (3, Serious, Critical),
            (32, Serious, Critical),
            (33, Serious, Serious),
            (34, Fair, Serious),
            (50, Nominal, Serious),
            (79, Nominal, Serious),
            (80, Nominal, Nominal),
        ] {
            g.update(observed, Duration::from_secs(seconds));
            assert_eq!(g.state, expected, "at {seconds}s");
        }
    }

    #[test]
    fn reheating_cancels_cooling_and_overrides_preserve_lower_player_caps() {
        let mut g = Governor::default();
        g.update(Critical, Duration::ZERO);
        g.update(Fair, Duration::from_secs(1));
        g.update(Critical, Duration::from_secs(29));
        g.update(Fair, Duration::from_secs(30));
        g.update(Fair, Duration::from_secs(59));
        assert_eq!(g.state, Critical);
        assert_eq!(
            (g.frame_cap(120), g.frame_cap(20), g.resolution_scale(75)),
            (30, 20, 50)
        );
        g.update(Fair, Duration::from_secs(60));
        assert_eq!((g.frame_cap(120), g.resolution_scale(75)), (120, 75));
        g.update(Serious, Duration::from_secs(61));
        assert_eq!(
            (g.frame_cap(120), g.frame_cap(20), g.resolution_scale(75)),
            (60, 20, 60)
        );
        assert_eq!(g.resolution_scale(50), 50);
    }
}

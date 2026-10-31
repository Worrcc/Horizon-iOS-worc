//! Orders server mods give content mods' mobs (capability `entities`, docs/mods.md): where to
//! walk and look, ahead of the mob's own goals, and which of its own animations it plays.
use super::{
    Mob,
    goals::{Cx, Goal, LOOK, MOVE},
};
use std::sync::Arc;

/// A mob's standing orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Orders {
    /// Walk to this point at this multiple of its speed.
    pub walk: Option<([f64; 3], f64)>,
    /// Keep its head turned to this point.
    pub look: Option<[f64; 3]>,
    /// The animation an order plays (mobs/geo.rs `animate`), and the age it started at.
    pub animation: Option<(Arc<str>, f32)>,
}

/// The priority of [`Obey`]: ahead of every vanilla goal (0 is the first vanilla uses).
pub const PRIORITY: i32 = -1;

/// Carries out walk and look orders while there are any; the mob's own goals that move or
/// look wait meanwhile. Registered on every content mod's mob (`ai::register`).
#[derive(Clone, Debug, Default)]
pub struct Obey {
    /// The point its path leads to now.
    heading: Option<[f64; 3]>,
}
impl Goal for Obey {
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.orders.walk.is_some() || m.orders.look.is_some()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.heading = None;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        if self.heading.take().is_some() {
            m.ai.nav.stop();
        }
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        match m.orders.walk {
            Some((to, speed)) if self.heading != Some(to) => {
                self.heading = Some(to);
                if !m.nav_move_to(cx.w, to, speed) {
                    // No path: the order is spent.
                    m.orders.walk = None;
                }
            }
            Some(_) if m.ai.nav.is_done() => {
                m.orders.walk = None;
                self.heading = None;
            }
            Some(_) => {}
            None if self.heading.take().is_some() => m.ai.nav.stop(),
            None => {}
        }
        if let Some(at) = m.orders.look {
            m.look_at(at);
        } else if let Some(to) = self.heading {
            m.look_at([to[0], m.position.y + m.eye_height(), to[2]]);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

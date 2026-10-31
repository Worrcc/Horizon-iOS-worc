//! Orders server mods give content mods' mobs (capability `entities`, docs/mods.md): where to
//! walk and look, ahead of the mob's own goals, and which of its own animations it plays.
use super::{
    Mob,
    goals::{Cx, Goal, LOOK, MOVE},
};
use horizon_world::nbt::{Compound, Tag};
use std::{collections::BTreeMap, sync::Arc};

/// The most `set-data` entries a mob keeps.
pub const DATA_ENTRIES: usize = 32;
/// The save's keys: Horizon's own, beside vanilla's.
const DATA: &str = "horizon:data";
const ANIMATION: &str = "horizon:animation";

/// A mob's standing orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Orders {
    /// Walk to this point at this multiple of its speed.
    pub walk: Option<([f64; 3], f64)>,
    /// Keep its head turned to this point.
    pub look: Option<[f64; 3]>,
    /// The animation an order plays (mobs/geo.rs `animate`), and the age (ticks) it started at:
    /// elapsed ticks are `age.wrapping_sub(start)`, so a start before the age a loaded mob
    /// begins at (0) still counts right.
    pub animation: Option<(Arc<str>, u32)>,
    /// What server mods stored on the mob (`set-data`, `spawn`): kept in its save.
    pub data: BTreeMap<String, String>,
}

/// Writes the mob's data and ordered animation into its save, or takes them out.
pub fn write_nbt(m: &Mob, map: &mut Compound) {
    map.remove(&DATA.into());
    map.remove(&ANIMATION.into());
    if !m.orders.data.is_empty() {
        let data = m.orders.data.iter();
        let data = data.map(|(k, v)| (k.as_str(), Tag::String(v.as_str().into())));
        map.insert(DATA.into(), Tag::compound(data));
    }
    if let Some((name, start)) = &m.orders.animation {
        let ticks = m.age.wrapping_sub(*start).min(i32::MAX as u32) as i32;
        let animation = [
            ("name", Tag::String((&**name).into())),
            ("ticks", Tag::Int(ticks)),
        ];
        map.insert(ANIMATION.into(), Tag::compound(animation));
    }
}

/// Reads what [`write_nbt`] saved: the animation goes on where it was.
pub fn read_nbt(m: &mut Mob, tag: &Tag) {
    if let Some(Tag::Compound(data)) = tag.get(DATA) {
        let strings = data.iter().filter_map(|(k, v)| match v {
            Tag::String(s) => Some((k.to_string_lossy(), s.to_string_lossy())),
            _ => None,
        });
        m.orders.data = strings.take(DATA_ENTRIES).collect();
    }
    if let Some(a) = tag.get(ANIMATION)
        && let Some(Tag::String(name)) = a.get("name")
    {
        let ticks = match a.get("ticks") {
            Some(Tag::Int(t)) => (*t).max(0) as u32,
            _ => 0,
        };
        let start = m.age.wrapping_sub(ticks);
        m.orders.animation = Some((Arc::from(name.to_string_lossy()), start));
    }
}

/// The priority of [`Obey`]: ahead of every vanilla goal (0 is the first vanilla uses).
pub const PRIORITY: i32 = -1;

/// Carries out walk and look orders while there are any; the mob's own goals that move or
/// look wait meanwhile. Registered on every content mod's mob (`ai::register`).
#[derive(Clone, Debug, Default)]
pub struct Obey {
    /// The point its path leads to now, and the speed it was ordered at.
    heading: Option<([f64; 3], f64)>,
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
            Some(walk) if self.heading != Some(walk) => {
                self.heading = Some(walk);
                let (to, speed) = walk;
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
        } else if let Some((to, _)) = self.heading {
            m.look_at([to[0], m.position.y + m.eye_height(), to[2]]);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

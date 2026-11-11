//! Minimal API 22 container example, not the Mouse Tweaks mod. Right-drag
//! places one per visited slot. A screen button latches Shift across screens.
use horizon_mod_sdk::{ContainerPointerKind, Mod, Registration, container};

struct Example {
    shift: bool,
    visited: Vec<u32>,
}
impl Example {
    fn button(&self) {
        // The label is mod-provided text, like ui::Widget labels.
        container::set_screen_button(0, -24.0, 4.0, "minecraft:arrow", self.shift, "Shift");
    }
}
impl Mod for Example {
    fn init() -> Result<(Self, Registration), String> {
        let m = Self {
            shift: false,
            visited: vec![],
        };
        m.button();
        Ok((m, Registration::new()))
    }
    fn container_pointer(
        &mut self,
        kind: ContainerPointerKind,
        _: f32,
        _: f32,
        button: u32,
        slot: Option<u32>,
        _: bool,
    ) {
        if button != 1 {
            return;
        }
        if kind == ContainerPointerKind::Up {
            self.visited.clear();
            return;
        }
        let Some(state) = container::state() else {
            return;
        };
        let Some(carried) = state.carried else {
            return;
        };
        if kind == ContainerPointerKind::Down {
            self.visited.clear();
        }
        container::take_drag();
        let Some(slot) = slot.and_then(|id| state.slots.iter().find(|s| s.index == id)) else {
            return;
        };
        if slot.accepts_item
            && slot.count < slot.max_stack
            && (slot.count == 0 || slot.same_key == carried.same_key)
            && !self.visited.contains(&slot.index)
        {
            self.visited.push(slot.index);
            container::click(slot.index, 1, false);
        }
    }
    fn container_button(&mut self, _: u32) {
        self.shift = !self.shift;
        container::set_shift_latch(self.shift);
        self.button();
    }
    fn container_closed(&mut self) {
        self.visited.clear();
    }
}
horizon_mod_sdk::export_mod!(Example);

//! Horizon's WASM container integration. All mutations use menu_click, the
//! same prediction/ContainerClick path the window uses for real slot clicks.
use super::Game;
use horizon_client::hud::HudBatch;
use winit::event::WindowEvent;

#[cfg(feature = "mods")]
use {
    super::{Screen, event_loop::EventLoopCtl},
    horizon_client::{
        container_screen::Button,
        items::ItemStack,
        menu::{Input, Menu},
        touch_controls::Output,
    },
    horizon_mods::{
        ContainerPart, ContainerPointerKind, ContainerReply, ContainerRequest, ContainerSlot,
        ContainerStack, ContainerState, Mods,
    },
    std::{
        collections::HashMap,
        hash::{Hash, Hasher},
    },
    winit::event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase},
};

#[cfg(feature = "mods")]
#[derive(Default)]
pub(super) struct ContainerScreenHost {
    open: bool,
    button_press: Option<u32>,
    touch: ContainerTouch,
}

#[cfg(feature = "mods")]
fn same_key(stack: &ItemStack) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    stack.item.hash(&mut hasher);
    stack.damage.hash(&mut hasher);
    stack.data().hash(&mut hasher);
    hasher.finish()
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn mod_container_event(&mut self, _: &WindowEvent) -> bool {
        false
    }
    pub(super) fn mod_container_lifecycle(&mut self) {}
    pub(super) fn mod_container_closed(&mut self) {}
    pub(super) fn draw_mod_container_buttons(&mut self, _: &mut HudBatch) {}
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// Run guest code off the game thread while servicing only its checked
    /// container calls here. This is scoped, synchronous, and never reenters WASM.
    pub(super) fn with_mod_container<R: Send>(
        &mut self,
        run: impl FnOnce(&mut Mods) -> R + Send,
    ) -> R {
        if self.screen != Screen::Inventory || !self.mods.mods.wants_container() {
            let result = run(&mut self.mods.mods);
            self.inv_clicks.set_shift_latch(
                self.screen == Screen::Inventory && self.mods.mods.container_shift_latched(),
            );
            return result;
        }
        let mut mods = std::mem::take(&mut self.mods.mods);
        let latches = mods.container_shift();
        let result = mods.with_container(
            |request| {
                // click()'s explicit shift alone chooses PICKUP/QUICK_MOVE (as
                // Mouse Tweaks' clickSlot): latches only shift the player's own
                // clicks and are reported in pointer events.
                let latch = latches.latched();
                self.inv_clicks.set_shift_latch(latch);
                self.container_request(request)
            },
            run,
        );
        self.mods.mods = mods;
        self.inv_clicks.set_shift_latch(
            self.screen == Screen::Inventory && self.mods.mods.container_shift_latched(),
        );
        result
    }

    fn container_request(&mut self, request: ContainerRequest) -> ContainerReply {
        if self.screen != Screen::Inventory || !self.mods.container.open || self.dialog_open() {
            return None;
        }
        match request {
            ContainerRequest::State => self.mod_container_state(),
            ContainerRequest::Click {
                slot,
                button,
                shift,
            } => {
                let slot = slot as usize;
                if button <= 1 && self.slots().contains(&slot) {
                    let input = if shift {
                        Input::QuickMove
                    } else {
                        Input::Pickup
                    };
                    self.menu_click(Some(slot), button as i32, input);
                    if let Some(key) = self.container {
                        self.container_changed(key);
                    }
                }
                None
            }
            ContainerRequest::TakeDrag => {
                self.inv_clicks.take_drag();
                if self.inv_clicks.drag_taken() {
                    self.inv_drag = None;
                    self.last_inv_click = None;
                    self.tweak_release();
                }
                None
            }
        }
    }

    fn mod_container_state(&mut self) -> Option<ContainerState> {
        let renderer = self.renderer.as_ref()?;
        let batch = HudBatch::new([renderer.config.width, renderer.config.height]);
        let image = self.container_image(&batch);
        let origin = self.inventory_origin(&batch);
        let ids = self.slots();
        let stacks: HashMap<usize, Option<ItemStack>> =
            ids.iter().map(|&i| (i, self.slot_view(i))).collect();
        let carried = self.carried.clone();
        let default = ItemStack::new("minecraft:stone", 1);
        let mut rules = HashMap::new();
        self.menu_session(|menu, env, to_menu| {
            for &id in &ids {
                let Some(index) = to_menu(id) else {
                    continue;
                };
                let own = stacks[&id].as_ref();
                let placement = carried.as_ref().or(own).unwrap_or(&default);
                let maximum = own.or(carried.as_ref()).unwrap_or(&default);
                if let (Some((accepts, _, result)), Some((_, max, _))) = (
                    menu.inspect_slot(env, index, placement),
                    menu.inspect_slot(env, index, maximum),
                ) {
                    rules.insert(id, (accepts, max, result));
                }
            }
            Some(false) // read only: never copy the temporary menu back
        });
        let item_max = |stack: &ItemStack| {
            // Exactly the component-aware item limit used by the menu engine.
            Menu::item_max_stack(world_items!(self), stack)
        };
        let slots = ids
            .into_iter()
            .map(|id| {
                let stack = stacks[&id].as_ref();
                let (sx, sy) = self.slot_position(id);
                let result = id == 200
                    || id == super::stations::RESULT
                    || (id == 309
                        && matches!(
                            self.container_kind(),
                            Some(horizon_client::containers::BlockEntity::Crafter(_))
                        ))
                    || (id == 302
                        && matches!(
                            self.container_kind(),
                            Some(horizon_client::containers::BlockEntity::Furnace(_))
                        ));
                let (accepts, max, result) = rules.get(&id).copied().unwrap_or_else(|| {
                    let probe = carried.as_ref().or(stack).unwrap_or(&default);
                    (
                        !result && !self.refuses(id, probe),
                        stack.or(carried.as_ref()).map_or(64, item_max),
                        result,
                    )
                });
                let part = if result {
                    ContainerPart::CraftingOutput
                } else {
                    match id {
                        0..=8 => ContainerPart::Hotbar,
                        9..=35 => ContainerPart::PlayerMain,
                        500..=503 => ContainerPart::Armor,
                        504 => ContainerPart::Offhand,
                        100..=108 | 300..=499 => ContainerPart::Container,
                        _ => ContainerPart::Other,
                    }
                };
                ContainerSlot {
                    index: id as u32,
                    x: origin.0 + sx,
                    y: origin.1 + sy,
                    item: stack.map_or_else(String::new, |s| s.item.to_string()),
                    count: stack.map_or(0, |s| s.count),
                    max_stack: max,
                    same_key: stack.map_or(0, same_key),
                    part,
                    accepts_item: accepts,
                }
            })
            .collect();
        Some(ContainerState {
            slots,
            carried: carried.as_ref().map(|s| ContainerStack {
                item: s.item.to_string(),
                count: s.count,
                max_stack: item_max(s),
                same_key: same_key(s),
            }),
            x: image.left as f32,
            y: image.top as f32,
            width: image.width as f32,
            height: image.height as f32,
        })
    }

    /// How often the mods' frame hook runs: everywhere but on a phone, every frame. On a
    /// phone it is thinned to save battery, except while a mod's own screen or the
    /// container the mods draw on is up, and on the frame the container opens or closes
    /// (the condition of `mod_container_lifecycle`), so those events are never delayed.
    #[cfg(not(target_os = "ios"))]
    pub(super) fn mod_frame_rate(&self) -> super::mods::FrameRate {
        super::mods::FrameRate::Full
    }
    #[cfg(target_os = "ios")]
    pub(super) fn mod_frame_rate(&self) -> super::mods::FrameRate {
        use super::mods::FrameRate;
        let lifecycle_changes =
            (self.screen == Screen::Inventory && self.player.is_some()) != self.mods.container.open;
        if (self.screen == Screen::Mod && self.mods.screen_open())
            || self.mods.container.open
            || lifecycle_changes
        {
            FrameRate::Full
        } else if self.pauses_game() {
            FrameRate::Paused
        } else {
            FrameRate::Hz(if self.phone_low_power() { 30 } else { 60 })
        }
    }

    pub(super) fn mod_container_lifecycle(&mut self) {
        let open = self.screen == Screen::Inventory && self.player.is_some();
        if open == self.mods.container.open {
            return;
        }
        if open {
            self.mods.container.open = true;
            self.with_mod_container(Mods::container_opened);
            self.show_mod_output();
        } else {
            self.mod_container_closed();
        }
    }
    pub(super) fn mod_container_closed(&mut self) {
        if !std::mem::take(&mut self.mods.container.open) {
            return;
        }
        self.mods.container.button_press = None;
        if self.mods.container.touch.dragging {
            self.physical_buttons.remove(&MouseButton::Left);
            self.track_drag_button(MouseButton::Left, false);
        }
        self.mods.container.touch = ContainerTouch::default();
        self.inv_clicks.cancel_drag();
        self.inv_drag = None;
        self.with_mod_container(Mods::container_closed);
        self.show_mod_output();
    }

    fn container_button_at(&self, at: (f32, f32)) -> Option<(usize, u32)> {
        let r = self.renderer.as_ref()?;
        let batch = HudBatch::new([r.config.width, r.config.height]);
        let image = self.container_image(&batch);
        self.mods
            .mods
            .container_buttons()
            .filter_map(|(owner, b)| {
                let x = image.left as f32 + b.dx;
                let y = image.top as f32 + b.dy;
                (at.0 >= x && at.0 < x + 20.0 && at.1 >= y && at.1 < y + 20.0)
                    .then_some((owner, b.id))
            })
            .last()
    }

    /// Mouse events, also used by touch's synthetic left mouse events. Position
    /// is computed before vanilla updates cursor; host clicks target slot ids.
    pub(super) fn mod_container_event(&mut self, event: &WindowEvent) -> bool {
        self.mod_container_lifecycle();
        self.inv_clicks.set_shift_latch(
            self.screen == Screen::Inventory && self.mods.mods.container_shift_latched(),
        );
        if !self.mods.container.open {
            return false;
        }
        let at = match event {
            WindowEvent::CursorMoved { position, .. } => {
                let Some(r) = &self.renderer else {
                    return false;
                };
                let scale = HudBatch::new([r.config.width, r.config.height]).scale;
                (position.x as f32 / scale, position.y as f32 / scale)
            }
            _ => self.cursor,
        };
        let slot = self
            .renderer
            .as_ref()
            .and_then(|r| {
                let batch = HudBatch::new([r.config.width, r.config.height]);
                self.slot_at(&batch, at.0, at.1).map(|s| s as u32)
            })
            .filter(|_| self.container_button_at(at).is_none());
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let (number, button) = match button {
                    MouseButton::Left => (0, Button::Left),
                    MouseButton::Right => (1, Button::Right),
                    MouseButton::Middle => (2, Button::Middle),
                    _ => return false,
                };
                let down = *state == ElementState::Pressed;
                if down {
                    self.inv_clicks.pointer_down(button);
                }
                let kind = if down {
                    ContainerPointerKind::Down
                } else {
                    ContainerPointerKind::Up
                };
                let shift = self.modifiers.shift_key();
                self.with_mod_container(|mods| {
                    mods.container_pointer(kind, at.0, at.1, number, slot, shift)
                });
                self.show_mod_output();
                if self.screen != Screen::Inventory {
                    return true;
                }
                if !down && self.mods.container.button_press == Some(number) {
                    self.mods.container.button_press = None;
                    self.inv_clicks.cancel_drag();
                    return true;
                }
                if down && let Some((owner, id)) = self.container_button_at(at) {
                    self.mods.container.button_press = Some(number);
                    self.inv_clicks.take_drag();
                    self.inv_drag = None;
                    if number == 0 {
                        self.with_mod_container(|mods| mods.container_button(owner, id));
                        self.show_mod_output();
                    }
                    return true;
                }
                down && self.inv_clicks.drag_taken()
            }
            WindowEvent::CursorMoved { .. } => {
                if self.mods.container.button_press.is_some() {
                    self.cursor = at;
                    return true;
                }
                for (number, button) in [
                    (0, MouseButton::Left),
                    (1, MouseButton::Right),
                    (2, MouseButton::Middle),
                ] {
                    if self.physical_buttons.contains(&button) {
                        let shift = self.modifiers.shift_key();
                        self.with_mod_container(|mods| {
                            mods.container_pointer(
                                ContainerPointerKind::Drag,
                                at.0,
                                at.1,
                                number,
                                slot,
                                shift,
                            )
                        });
                    }
                }
                self.show_mod_output();
                false
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 30.0) as f32,
                };
                let steps = (if self.settings.discrete_scroll {
                    horizon_client::mouse::java_signum(f64::from(steps)) as f32
                } else {
                    steps
                }) * self.settings.mouse_wheel_sensitivity;
                let consumed =
                    self.with_mod_container(|mods| mods.container_scroll(at.0, at.1, steps, slot));
                self.show_mod_output();
                consumed
            }
            _ => false,
        }
    }

    pub(super) fn draw_mod_container_buttons(&mut self, batch: &mut HudBatch) {
        let image = self.container_image(batch);
        let buttons: Vec<_> = self
            .mods
            .mods
            .container_buttons()
            .map(|(_, b)| b.clone())
            .collect();
        let mut tooltip = None;
        for b in buttons {
            let (x, y) = (image.left as f32 + b.dx, image.top as f32 + b.dy);
            super::touch::draw_container_frame(batch, &self.gui.atlas, [x, y, 20.0, 20.0], b.lit);
            self.draw_item_model(batch, &ItemStack::new(&b.icon, 1), x + 2.0, y + 2.0);
            if self.cursor.0 >= x
                && self.cursor.0 < x + 20.0
                && self.cursor.1 >= y
                && self.cursor.1 < y + 20.0
            {
                tooltip = Some(b.tooltip);
            }
        }
        if let Some(text) = tooltip.filter(|s| !s.is_empty()) {
            self.draw_tooltip_lines(batch, &[(text, 0xFFFFFFFF)], self.cursor.0, self.cursor.1);
        }
    }

    /// Single-finger drag starts on movement; a second finger takes the vanilla drag
    /// and scrolls at the pair's midpoint. Remaining fingers stay inert.
    pub(super) fn mod_container_touch(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        touch: &Touch,
        at: (f32, f32),
        scale: f32,
    ) -> bool {
        if self.screen != Screen::Inventory
            || (!self.mods.mods.wants_container() && self.mods.container.touch.fingers.is_empty())
        {
            return false;
        }
        let events = self.mods.container.touch.event(touch.id, touch.phase, at);
        for event in events {
            match event {
                ContainerTouchEvent::Output(output) => self.apply_touch(event_loop, output, scale),
                ContainerTouchEvent::Take => {
                    self.inv_clicks.take_drag();
                    self.inv_drag = None;
                }
            }
        }
        true
    }
}

#[cfg(feature = "mods")]
#[derive(Default)]
struct ContainerTouch {
    fingers: Vec<(u64, (f32, f32))>,
    dragging: bool,
    pending: bool,
    /// Where a two-finger scroll aims: the pair's midpoint when the second
    /// finger landed. It stays put (as a trackpad's cursor does) so the
    /// gesture keeps scrolling the slot it started on.
    anchor: Option<(f32, f32)>,
}
#[cfg(feature = "mods")]
enum ContainerTouchEvent {
    Output(Output),
    Take,
}
#[cfg(feature = "mods")]
impl ContainerTouch {
    fn midpoint(&self) -> Option<(f32, f32)> {
        let [(_, a), (_, b), ..] = self.fingers.as_slice() else {
            return None;
        };
        Some(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
    }
    fn event(&mut self, id: u64, phase: TouchPhase, at: (f32, f32)) -> Vec<ContainerTouchEvent> {
        use ContainerTouchEvent::{Output as Out, Take};
        let mut out = Vec::new();
        match phase {
            TouchPhase::Started => {
                if self.fingers.iter().any(|f| f.0 == id) {
                    return out;
                }
                if self.fingers.is_empty() {
                    // Defer the press until movement or a tap's release, so
                    // starting a two-finger scroll never picks up an item.
                    self.pending = true;
                    out.push(Out(Output::Pointer(at.0, at.1)));
                } else if std::mem::take(&mut self.dragging) {
                    out.extend([Take, Out(Output::PointerButton(false))]);
                }
                if !self.fingers.is_empty() {
                    self.pending = false;
                }
                self.fingers.push((id, at));
                if self.fingers.len() == 2 {
                    self.anchor = self.midpoint();
                }
            }
            TouchPhase::Moved => {
                let old = self.midpoint();
                let Some(finger) = self.fingers.iter_mut().find(|f| f.0 == id) else {
                    return out;
                };
                if self.pending && (at.0 - finger.1.0).hypot(at.1 - finger.1.1) >= 4.0 {
                    self.pending = false;
                    self.dragging = true;
                    out.push(Out(Output::PointerButton(true)));
                }
                if !self.pending {
                    finger.1 = at;
                }
                if self.dragging {
                    out.push(Out(Output::Pointer(at.0, at.1)));
                } else if let (Some(old), Some(new), Some(aim)) =
                    (old, self.midpoint(), self.anchor)
                {
                    out.extend([
                        Out(Output::Pointer(aim.0, aim.1)),
                        Out(Output::Scroll((new.1 - old.1) / 12.0)),
                    ]);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(i) = self.fingers.iter().position(|f| f.0 == id) else {
                    return out;
                };
                self.fingers.remove(i);
                if self.fingers.len() < 2 {
                    self.anchor = None;
                }
                if std::mem::take(&mut self.dragging) {
                    if phase == TouchPhase::Cancelled {
                        out.push(Take);
                    }
                    out.extend([
                        Out(Output::Pointer(at.0, at.1)),
                        Out(Output::PointerButton(false)),
                    ]);
                } else if std::mem::take(&mut self.pending) && phase == TouchPhase::Ended {
                    out.extend([
                        Out(Output::Pointer(at.0, at.1)),
                        Out(Output::PointerButton(true)),
                        Out(Output::PointerButton(false)),
                    ]);
                }
            }
        }
        out
    }
}

#[cfg(all(test, feature = "mods"))]
mod container_screen_tests {
    use super::*;
    #[test]
    fn stack_key_includes_components_but_not_count() {
        let a = ItemStack::new("minecraft:stone", 1);
        let mut b = a.clone();
        b.count = 64;
        assert_eq!(same_key(&a), same_key(&b));
        b.damage = 1;
        assert_ne!(same_key(&a), same_key(&b));
        assert_ne!(
            same_key(&a),
            same_key(&a.clone().with_potion("minecraft:healing"))
        );
    }
    #[test]
    fn stack_key_matches_runtime_component_equality() {
        let base = ItemStack::new("minecraft:filled_map", 1);
        let mut modified = base.clone();
        // A transient component omitted by disk codecs still affects merging.
        modified.data_mut().map_post_processing = Some(1);
        assert!(!base.same_item_same_components(&modified));
        assert_ne!(same_key(&base), same_key(&modified));
        let mut other = modified.clone();
        // Wire provenance and the selected item in a bundle do not affect merging.
        other.data_mut().wire =
            horizon_client::item_data::Wire::new(base.clone(), vec![1, 2, 3].into());
        other.data_mut().bundle_selected = horizon_client::item_data::BundleSelected(2);
        assert!(modified.same_item_same_components(&other));
        assert_eq!(same_key(&modified), same_key(&other));
    }

    #[test]
    fn touch_two_finger_scroll_does_not_click_the_initial_slot() {
        let mut t = ContainerTouch::default();
        assert!(matches!(
            t.event(1, TouchPhase::Started, (20.0, 20.0)).as_slice(),
            [ContainerTouchEvent::Output(Output::Pointer(..))]
        ));
        assert!(t.event(2, TouchPhase::Started, (40.0, 20.0)).is_empty());
        assert!(matches!(
            t.event(1, TouchPhase::Moved, (20.0, 44.0)).as_slice(),
            [
                ContainerTouchEvent::Output(Output::Pointer(..)),
                ContainerTouchEvent::Output(Output::Scroll(1.0))
            ]
        ));
        assert!(t.event(2, TouchPhase::Ended, (40.0, 20.0)).is_empty());
        assert!(t.event(1, TouchPhase::Ended, (20.0, 44.0)).is_empty());
        // A fresh single-finger tap still produces exactly one press/release.
        t.event(3, TouchPhase::Started, (20.0, 20.0));
        assert!(matches!(
            t.event(3, TouchPhase::Ended, (20.0, 20.0)).as_slice(),
            [
                ContainerTouchEvent::Output(Output::Pointer(..)),
                ContainerTouchEvent::Output(Output::PointerButton(true)),
                ContainerTouchEvent::Output(Output::PointerButton(false))
            ]
        ));
    }

    #[test]
    fn touch_drag_then_two_finger_scroll_has_no_extra_click() {
        let mut t = ContainerTouch::default();
        let _ = t.event(1, TouchPhase::Started, (20.0, 20.0));
        assert!(matches!(
            t.event(1, TouchPhase::Moved, (40.0, 20.0)).as_slice(),
            [
                ContainerTouchEvent::Output(Output::PointerButton(true)),
                ContainerTouchEvent::Output(Output::Pointer(40.0, 20.0))
            ]
        ));
        assert!(matches!(
            t.event(2, TouchPhase::Started, (60.0, 20.0)).as_slice(),
            [
                ContainerTouchEvent::Take,
                ContainerTouchEvent::Output(Output::PointerButton(false))
            ]
        ));
        assert!(matches!(
            t.event(2, TouchPhase::Moved, (60.0, 44.0)).as_slice(),
            [
                ContainerTouchEvent::Output(Output::Pointer(50.0, 20.0)),
                ContainerTouchEvent::Output(Output::Scroll(1.0))
            ]
        ));
        assert!(t.event(2, TouchPhase::Ended, (60.0, 44.0)).is_empty());
        assert!(t.event(1, TouchPhase::Ended, (40.0, 20.0)).is_empty());
    }
}

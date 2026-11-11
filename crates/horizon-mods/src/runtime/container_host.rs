//! API 22: synchronous container access and optional input callbacks.
//! The game stays on its owning thread. During a hook, a scoped WASM worker
//! asks it for live reads/clicks over a rendezvous channel. No borrowed Game
//! pointers escape into a Store, and a click completes before the guest resumes.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender, SyncSender},
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "container-host",
    });
}
pub(super) use bindings::ContainerHost;
pub use bindings::exports::horizon::mods::container_events::PointerKind as ContainerPointerKind;
use bindings::horizon::mods::container;
pub use bindings::horizon::mods::container::{
    ContainerState, Part as ContainerPart, Slot as ContainerSlot, Stack as ContainerStack,
};

/// A container call serviced by the client's normal menu/input path.
#[derive(Debug)]
pub enum ContainerRequest {
    State,
    Click { slot: u32, button: u32, shift: bool },
    TakeDrag,
}
/// Answer to a live read; mutation calls answer with None.
pub type ContainerReply = Option<ContainerState>;
type Message = (ContainerRequest, SyncSender<ContainerReply>);
pub(super) type Bridge = Arc<Mutex<Option<Sender<Message>>>>;

/// Live OR of the running mods' Shift latches, shared with the game while
/// hooks run. Weak references also handle mods started/replaced in a reload.
#[derive(Clone, Default)]
pub struct ContainerShift(Arc<Mutex<Vec<std::sync::Weak<AtomicBool>>>>);
impl ContainerShift {
    pub fn latched(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .any(|l| l.load(Ordering::Relaxed))
    }
    fn register(&self, latch: &Arc<AtomicBool>) {
        let mut latches = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        latches.retain(|l| l.strong_count() != 0);
        latches.push(Arc::downgrade(latch));
    }
}

/// One mod's persistent button, relative to the open container background.
#[derive(Clone, Debug)]
pub struct ContainerButton {
    pub id: u32,
    pub dx: f32,
    pub dy: f32,
    pub icon: String,
    pub lit: bool,
    pub tooltip: String,
}
#[derive(Default)]
pub(super) struct ContainerData {
    pub buttons: Vec<ContainerButton>,
    // An Arc permits the game thread to observe changes while a hook runs.
    pub latch: Arc<AtomicBool>,
    pub bridge: Bridge,
    pub(super) actions: usize,
}
impl Drop for ContainerData {
    fn drop(&mut self) {
        self.latch.store(false, Ordering::Relaxed);
    }
}
impl ContainerData {
    pub fn new(bridge: Bridge, shift: &ContainerShift) -> Self {
        let latch = Arc::default();
        shift.register(&latch);
        Self {
            bridge,
            buttons: Vec::new(),
            latch,
            actions: 0,
        }
    }
    fn request(&mut self, request: ContainerRequest) -> ContainerReply {
        // Fuel alone cannot bound work done by the game for a guest loop.
        if self.actions >= 256 && !matches!(request, ContainerRequest::State) {
            return None;
        }
        if !matches!(request, ContainerRequest::State) {
            self.actions += 1;
        }
        let bridge = self.bridge.lock().ok()?.clone()?;
        let (reply, answer) = mpsc::sync_channel(0);
        bridge.send((request, reply)).ok()?;
        answer.recv().ok().flatten()
    }
    pub fn reset_budget(&mut self) {
        self.actions = 0;
    }
}

impl container::Host for State {
    fn state(&mut self) -> Option<ContainerState> {
        self.container.request(ContainerRequest::State)
    }
    fn click(&mut self, slot: u32, button: u32, shift: bool) {
        if button <= 1 {
            self.container.request(ContainerRequest::Click {
                slot,
                button,
                shift,
            });
        }
    }
    fn take_drag(&mut self) {
        self.container.request(ContainerRequest::TakeDrag);
    }
    fn set_shift_latch(&mut self, on: bool) {
        self.container.latch.store(on, Ordering::Relaxed);
    }
    fn set_screen_button(
        &mut self,
        id: u32,
        dx: f32,
        dy: f32,
        icon: String,
        lit: bool,
        tooltip: String,
    ) {
        if !dx.is_finite() || !dy.is_finite() || !resource_id(&icon) {
            return;
        }
        let button = ContainerButton {
            id,
            dx,
            dy,
            icon,
            lit,
            tooltip: clip(tooltip),
        };
        if let Some(old) = self.container.buttons.iter_mut().find(|b| b.id == id) {
            *old = button;
        } else if self.container.buttons.len() < 8 {
            self.container.buttons.push(button);
        }
    }
    fn clear_screen_button(&mut self, id: u32) {
        self.container.buttons.retain(|b| b.id != id);
    }
}

type PointerArgs = (ContainerPointerKind, f32, f32, u32, Option<u32>, bool);
#[derive(Default)]
pub(super) struct ContainerCallbacks {
    pointer: Option<TypedFunc<PointerArgs, ()>>,
    scroll: Option<TypedFunc<(f32, f32, f32, Option<u32>), (bool,)>>,
    button: Option<TypedFunc<(u32,), ()>>,
    opened: Option<TypedFunc<(), ()>>,
    closed: Option<TypedFunc<(), ()>>,
}
impl ContainerCallbacks {
    pub fn new(store: &mut Store<State>, instance: &Instance) -> Self {
        let Some(interface) =
            instance.get_export_index(&mut *store, None, "horizon:mods/container-events@0.1.0")
        else {
            return Self::default();
        };
        macro_rules! callback {
            ($name:literal) => {
                instance
                    .get_export_index(&mut *store, Some(&interface), $name)
                    .and_then(|index| instance.get_typed_func(&mut *store, index).ok())
            };
        }
        Self {
            pointer: callback!("pointer"),
            scroll: callback!("scroll"),
            button: callback!("button"),
            opened: callback!("opened"),
            closed: callback!("closed"),
        }
    }
}
impl Mods {
    /// Whether the game's hook needs live container access (client capability only).
    pub fn wants_container(&self) -> bool {
        self.entries.iter().any(|e| {
            e.run.is_some()
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::Container))
        })
    }
    pub fn container_shift(&self) -> ContainerShift {
        self.container_shift.clone()
    }
    pub fn container_shift_latched(&self) -> bool {
        self.container_shift.latched()
    }
    pub fn container_buttons(&self) -> impl Iterator<Item = (usize, &ContainerButton)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.run.as_ref().map(|r| (i, r)))
            .flat_map(|(i, r)| r.store.data().container.buttons.iter().map(move |b| (i, b)))
    }
    /// Runs a hook with synchronous live container host calls. The handler
    /// executes on the calling thread and may borrow the game for this scope.
    pub fn with_container<R: Send>(
        &mut self,
        mut handle: impl FnMut(ContainerRequest) -> ContainerReply,
        run: impl FnOnce(&mut Self) -> R + Send,
    ) -> R {
        if !self.wants_container() {
            return run(self);
        }
        let (sender, requests) = mpsc::channel();
        *self
            .container_bridge
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender);
        let bridge = self.container_bridge.clone();
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                // Always disconnect, even if a Rust panic unwinds the worker.
                struct Disconnect(Bridge);
                impl Drop for Disconnect {
                    fn drop(&mut self) {
                        self.0
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take();
                    }
                }
                let _disconnect = Disconnect(bridge);
                run(self)
            });
            for (request, reply) in requests {
                let _ = reply.send(handle(request));
            }
            worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
    fn container_event<P, R>(
        &mut self,
        i: usize,
        get: impl FnOnce(&ContainerCallbacks) -> Option<TypedFunc<P, R>>,
        args: P,
    ) -> Option<R>
    where
        P: wasmtime::component::ComponentNamedList + wasmtime::component::Lower + 'static,
        R: wasmtime::component::ComponentNamedList + wasmtime::component::Lift + 'static,
    {
        let func = self
            .entries
            .get(i)?
            .run
            .as_ref()
            .and_then(|r| get(&r.container))?;
        self.call(i, FUEL_EVENT, |r| func.call(&mut r.store, args))
    }
    pub fn container_pointer(
        &mut self,
        kind: ContainerPointerKind,
        x: f32,
        y: f32,
        button: u32,
        slot: Option<u32>,
        shift: bool,
    ) {
        if !x.is_finite() || !y.is_finite() || button > 2 {
            return;
        }
        for i in 0..self.entries.len() {
            let shift = shift || self.container_shift_latched();
            self.container_event(i, |c| c.pointer, (kind, x, y, button, slot, shift));
        }
    }
    /// First consumer in load order wins. A trap is not a consumed scroll.
    pub fn container_scroll(&mut self, x: f32, y: f32, steps: f32, slot: Option<u32>) -> bool {
        if !x.is_finite() || !y.is_finite() || !steps.is_finite() {
            return false;
        }
        for i in 0..self.entries.len() {
            if self.container_event(i, |c| c.scroll, (x, y, steps, slot)) == Some((true,)) {
                return true;
            }
        }
        false
    }
    pub fn container_button(&mut self, owner: usize, id: u32) {
        if self
            .container_buttons()
            .any(|(i, b)| i == owner && b.id == id)
        {
            self.container_event(owner, |c| c.button, (id,));
        }
    }
    pub fn container_opened(&mut self) {
        for i in 0..self.entries.len() {
            self.container_event(i, |c| c.opened, ());
        }
    }
    pub fn container_closed(&mut self) {
        for i in 0..self.entries.len() {
            self.container_event(i, |c| c.closed, ());
        }
    }
}

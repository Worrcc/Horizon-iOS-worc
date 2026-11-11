//! Button combinations and their clock-driven gameplay resolver. No backend or wall clock.
use super::Button;
use std::time::Duration;

pub const CHORD_WINDOW: Duration = Duration::from_millis(50);

/// Declaration order is kept for the label; matching treats a chord as an unordered pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Single(Button),
    Chord(Button, Button),
}
impl Binding {
    pub fn parse(text: &str) -> Option<Self> {
        match text.split_once('+') {
            Some((a, b)) => {
                let (a, b) = (Button::parse(a)?, Button::parse(b)?);
                (a != b).then_some(Self::Chord(a, b))
            }
            None => Button::parse(text).map(Self::Single),
        }
    }
    pub fn name(self) -> String {
        match self {
            Self::Single(b) => b.name().to_owned(),
            Self::Chord(a, b) => format!("{}+{}", a.name(), b.name()),
        }
    }
    pub fn mask(self) -> u32 {
        match self {
            Self::Single(b) => b.bit(),
            Self::Chord(a, b) => a.bit() | b.bit(),
        }
    }
    pub fn same_buttons(self, other: Self) -> bool {
        self.mask() == other.mask()
    }
    pub fn label(self, style: super::GlyphStyle, by_letter: bool) -> String {
        let label = |b: Button| (if by_letter { b.by_letter() } else { b }).label(style);
        match self {
            Self::Single(b) => label(b).to_owned(),
            Self::Chord(a, b) => format!("{} + {}", label(a), label(b)),
        }
    }
}

/// A binding capture finishes on the first release, or on the second press.
#[derive(Clone, Debug, Default)]
pub struct Capture {
    first: Option<Button>,
}
impl Capture {
    pub fn edge(&mut self, button: Button, down: bool) -> Option<Binding> {
        match (self.first, down) {
            (None, true) => {
                self.first = Some(button);
                None
            }
            (Some(a), true) if a != button => {
                self.first = None;
                Some(Binding::Chord(a, button))
            }
            (Some(a), false) if a == button => {
                self.first = None;
                Some(Binding::Single(a))
            }
            _ => None,
        }
    }
    pub fn first(&self) -> Option<Button> {
        self.first
    }
}

pub type ActionEdge = (String, bool);

/// Every completed chord fires, including chords that share a held member. Identical
/// combinations can drive several actions. A consumed member's single stays suppressed
/// until it lifts, but it can still complete another chord with a fresh second press.
#[derive(Default)]
pub struct Resolver {
    bindings: Vec<(String, Binding)>,
    held: u32,
    consumed: u32,
    blocked: u32,
    pending: [Option<Duration>; 16],
    active: Vec<(String, Binding)>,
}
impl Resolver {
    pub fn held(&self, action: &str) -> bool {
        self.active.iter().any(|(a, _)| a == action)
    }
    pub fn cancel(&mut self) -> Vec<ActionEdge> {
        self.held = 0;
        self.consumed = 0;
        self.blocked = 0;
        self.pending.fill(None);
        self.active.drain(..).map(|(a, _)| (a, false)).collect()
    }
    fn end(&mut self, mask: u32, out: &mut Vec<ActionEdge>) {
        self.active.retain(|(a, b)| {
            if b.mask() & mask == 0 {
                true
            } else {
                out.push((a.clone(), false));
                false
            }
        });
    }
    fn begin(&mut self, binding: Binding, out: &mut Vec<ActionEdge>) {
        for (action, b) in &self.bindings {
            if b.same_buttons(binding) {
                out.push((action.clone(), true));
                self.active.push((action.clone(), *b));
            }
        }
    }
    /// Edges in one poll form a batch. A second press at exactly 50 ms wins over expiration.
    /// Rebinding releases the old actions and blocks already-held buttons until they lift.
    pub fn advance(
        &mut self,
        now: Duration,
        edges: &[(Button, bool)],
        bindings: &[(String, Binding)],
    ) -> Vec<ActionEdge> {
        let mut out = Vec::new();
        if self.bindings != bindings {
            let held = self.held;
            out.extend(self.cancel());
            self.held = held;
            self.consumed = held;
            self.blocked = held;
            self.bindings = bindings.to_vec();
        }
        for &(b, down) in edges.iter().filter(|(_, down)| !down) {
            debug_assert!(!down);
            if self.held & b.bit() == 0 {
                continue;
            }
            if self.pending[b as usize].take().is_some() {
                self.begin(Binding::Single(b), &mut out);
            }
            self.end(b.bit(), &mut out);
            self.held &= !b.bit();
            self.consumed &= !b.bit();
            self.blocked &= !b.bit();
        }
        let mut pressed = 0;
        for &(b, down) in edges {
            if down && self.held & b.bit() == 0 {
                self.held |= b.bit();
                pressed |= b.bit();
            }
        }
        let candidates: Vec<Binding> = self.bindings.iter().map(|(_, b)| *b).collect();
        for binding in candidates {
            if !matches!(binding, Binding::Chord(_, _)) {
                continue;
            }
            let mask = binding.mask();
            if mask & pressed == 0
                || self.held & mask != mask
                || self.blocked & mask != 0
                || self
                    .active
                    .iter()
                    .any(|(_, b)| matches!(b, Binding::Chord(_, _)) && b.same_buttons(binding))
            {
                continue;
            }
            // Starting a chord releases member singles, while other chords keep their holds.
            self.active.retain(|(action, b)| {
                if matches!(b, Binding::Single(_)) && b.mask() & mask != 0 {
                    out.push((action.clone(), false));
                    false
                } else {
                    true
                }
            });
            for b in Button::ALL {
                if mask & b.bit() != 0 {
                    self.pending[b as usize] = None;
                }
            }
            self.consumed |= mask;
            self.begin(binding, &mut out);
        }
        let chord_members = self
            .bindings
            .iter()
            .filter(|(_, b)| matches!(b, Binding::Chord(_, _)))
            .fold(0, |mask, (_, b)| mask | b.mask());
        for b in Button::ALL {
            if self.consumed & b.bit() != 0 {
                continue;
            }
            if pressed & b.bit() != 0 {
                if chord_members & b.bit() != 0 {
                    self.pending[b as usize] = Some(now);
                } else {
                    self.begin(Binding::Single(b), &mut out);
                }
            }
            if self.pending[b as usize].is_some_and(|at| now.saturating_sub(at) >= CHORD_WINDOW) {
                self.pending[b as usize] = None;
                self.begin(Binding::Single(b), &mut out);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Button::{North as Y, South as A, West as X};
    fn bindings() -> Vec<(String, Binding)> {
        vec![
            ("a".into(), Binding::Single(A)),
            ("y".into(), Binding::Single(Y)),
            ("x".into(), Binding::Single(X)),
            ("ya".into(), Binding::Chord(Y, A)),
        ]
    }
    fn step(
        r: &mut Resolver,
        ms: u64,
        edges: &[(Button, bool)],
        binds: &[(String, Binding)],
    ) -> Vec<(String, bool)> {
        r.advance(Duration::from_millis(ms), edges, binds)
    }
    fn events(es: &[(&str, bool)]) -> Vec<ActionEdge> {
        es.iter().map(|(a, d)| ((*a).into(), *d)).collect()
    }
    #[test]
    fn press_and_release_orders_suppress_singles() {
        for (first, second) in [(Y, A), (A, Y)] {
            for released in [Y, A] {
                let mut r = Resolver::default();
                let b = bindings();
                assert!(step(&mut r, 0, &[(first, true)], &b).is_empty());
                assert_eq!(
                    step(&mut r, 30, &[(second, true)], &b),
                    events(&[("ya", true)])
                );
                assert!(r.held("ya"));
                assert!(!r.held("a"));
                assert_eq!(
                    step(&mut r, 40, &[(released, false)], &b),
                    events(&[("ya", false)])
                );
                assert!(step(&mut r, 150, &[], &b).is_empty());
                assert!(step(&mut r, 160, &[(first, false), (second, false)], &b).is_empty());
            }
        }
    }
    #[test]
    fn timing_boundary_and_held_then_press() {
        for delay in [49, 50] {
            let mut r = Resolver::default();
            let b = bindings();
            step(&mut r, 0, &[(Y, true)], &b);
            assert_eq!(
                step(&mut r, delay, &[(A, true)], &b),
                events(&[("ya", true)])
            );
        }
        let mut r = Resolver::default();
        let b = bindings();
        step(&mut r, 0, &[(Y, true)], &b);
        assert!(step(&mut r, 49, &[], &b).is_empty());
        assert_eq!(step(&mut r, 50, &[], &b), events(&[("y", true)]));
        assert_eq!(
            step(&mut r, 500, &[(A, true)], &b),
            events(&[("y", false), ("ya", true)])
        );
    }
    #[test]
    fn quick_tap_and_unrelated_single_are_immediate() {
        let mut r = Resolver::default();
        let b = bindings();
        assert_eq!(step(&mut r, 0, &[(X, true)], &b), events(&[("x", true)]));
        assert_eq!(
            step(&mut r, 1, &[(X, false), (Y, true)], &b),
            events(&[("x", false)])
        );
        assert_eq!(
            step(&mut r, 10, &[(Y, false)], &b),
            events(&[("y", true), ("y", false)])
        );
        assert!(step(&mut r, 100, &[], &b).is_empty());
    }
    #[test]
    fn shared_member_three_buttons_and_duplicate_chords() {
        let mut b = bindings();
        b.push(("yx".into(), Binding::Chord(Y, X)));
        b.push(("ay".into(), Binding::Chord(A, Y)));
        let mut r = Resolver::default();
        assert_eq!(
            step(&mut r, 0, &[(Y, true), (A, true), (X, true)], &b),
            events(&[("ya", true), ("ay", true), ("yx", true)])
        );
        assert!(step(&mut r, 50, &[], &b).is_empty());
        assert_eq!(
            step(&mut r, 60, &[(A, false)], &b),
            events(&[("ya", false), ("ay", false)])
        );
        assert!(r.held("yx"));
        assert_eq!(
            step(&mut r, 70, &[(X, false)], &b),
            events(&[("yx", false)])
        );
        assert!(step(&mut r, 80, &[], &b).is_empty());
        // Y remains held: a fresh X press completes Y+X again without a Y single.
        assert_eq!(step(&mut r, 90, &[(X, true)], &b), events(&[("yx", true)]));
        assert_eq!(
            step(&mut r, 100, &[(Y, false)], &b),
            events(&[("yx", false)])
        );
        assert!(step(&mut r, 150, &[], &b).is_empty());
    }
    #[test]
    fn third_button_can_complete_a_second_chord_while_the_first_is_held() {
        let mut b = bindings();
        b.push(("yx".into(), Binding::Chord(Y, X)));
        let mut r = Resolver::default();
        assert_eq!(
            step(&mut r, 0, &[(Y, true), (A, true)], &b),
            events(&[("ya", true)])
        );
        assert_eq!(step(&mut r, 100, &[(X, true)], &b), events(&[("yx", true)]));
        assert!(r.held("ya") && r.held("yx"));
        assert_eq!(
            step(&mut r, 200, &[(Y, false)], &b),
            events(&[("ya", false), ("yx", false)])
        );
        assert!(step(&mut r, 300, &[(A, false), (X, false)], &b).is_empty());
    }
    #[test]
    fn binding_change_releases_and_requires_fresh_press() {
        let mut r = Resolver::default();
        let b = bindings();
        step(&mut r, 0, &[(X, true)], &b);
        let changed = vec![("new".into(), Binding::Single(X))];
        assert_eq!(step(&mut r, 1, &[], &changed), events(&[("x", false)]));
        assert!(step(&mut r, 2, &[(X, false)], &changed).is_empty());
        assert_eq!(
            step(&mut r, 3, &[(X, true)], &changed),
            events(&[("new", true)])
        );
        assert_eq!(r.cancel(), events(&[("new", false)]));
    }
    #[test]
    fn capture_single_chord_and_labels() {
        let mut c = Capture::default();
        assert_eq!(c.edge(Y, true), None);
        assert_eq!(c.edge(Y, false), Some(Binding::Single(Y)));
        assert_eq!(c.edge(Y, true), None);
        let chord = c.edge(A, true).unwrap();
        assert_eq!(chord.label(super::super::GlyphStyle::Xbox, false), "Y + A");
        assert!(chord.same_buttons(Binding::Chord(A, Y)));
        assert_eq!(Binding::parse("north+south"), Some(chord));
        for bad in ["south+south", "south+oops", "north+south+west"] {
            assert_eq!(Binding::parse(bad), None);
        }
    }
}

//! Pure touch layout: stable defaults, automatic key labels and placement, and player overrides.
use std::collections::{BTreeMap, BTreeSet};
pub type Rect = [f32; 4];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Show {
    #[default]
    Default,
    Always,
    Riding,
    Never,
}
impl Show {
    pub fn visible(self, default: bool, riding: bool) -> bool {
        match self {
            Self::Default => default,
            Self::Always => true,
            Self::Riding => riding,
            Self::Never => false,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Default => Self::Always,
            Self::Always => Self::Riding,
            Self::Riding => Self::Never,
            Self::Never => Self::Default,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Always => "always",
            Self::Riding => "riding",
            Self::Never => "never",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "default" => Self::Default,
            "always" => Self::Always,
            "riding" => Self::Riding,
            "never" => Self::Never,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    /// None means an automatic 20px button, in the input's stable registration order.
    pub default: Option<Rect>,
    /// Hidden until the player shows it (automatic mod-key buttons, unless the
    /// mod asked for them shown).
    pub hidden_by_default: bool,
    /// Current context, including the player's Show choice; independent of initial hiding.
    pub visible_now: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Override {
    /// Top-left coordinates as fractions of the whole GUI viewport.
    pub x: f32,
    pub y: f32,
    pub hidden: bool,
    pub show: Show,
    /// The player placed it: `x` and `y` hold. False for a button only shown or
    /// hidden, which keeps its default place (an automatic one keeps flowing).
    pub moved: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides(pub BTreeMap<String, Override>);
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: String,
    pub rect: Option<Rect>,
    pub hidden: bool,
}

pub fn contains([x, y, w, h]: Rect, at: (f32, f32)) -> bool {
    at.0 >= x && at.1 >= y && at.0 < x + w && at.1 < y + h
}
pub fn overlaps(a: Rect, b: Rect, gap: f32) -> bool {
    a[0] < b[0] + b[2] + gap
        && b[0] < a[0] + a[2] + gap
        && a[1] < b[1] + b[3] + gap
        && b[1] < a[1] + a[3] + gap
}
pub fn clamp([x, y, w, h]: Rect, size: (f32, f32), snap: bool) -> Rect {
    let (w, h) = (w.min(size.0).max(0.0), h.min(size.1).max(0.0));
    let axis = |v: f32, max: f32| {
        if snap {
            (v / 2.0).round().clamp(0.0, (max / 2.0).floor()) * 2.0
        } else {
            v.clamp(0.0, max)
        }
    };
    [axis(x, size.0 - w), axis(y, size.1 - h), w, h]
}
pub fn fractions(rect: Rect, size: (f32, f32), hidden: bool) -> Override {
    let r = clamp(rect, size, true);
    Override {
        x: r[0] / size.0.max(1.0),
        y: r[1] / size.1.max(1.0),
        hidden,
        show: if hidden { Show::Never } else { Show::Default },
        moved: true,
    }
}
pub fn apply(default: Rect, size: (f32, f32), value: Override) -> Rect {
    clamp(
        [value.x * size.0, value.y * size.1, default[2], default[3]],
        size,
        true,
    )
}
impl Overrides {
    pub fn reset(&mut self, id: &str) {
        self.0.remove(id);
    }
    pub fn reset_all(&mut self) {
        self.0.clear();
    }
    pub fn show(&self, id: &str) -> Show {
        self.0.get(id).map_or(Show::Default, |v| v.show)
    }
    /// Changes visibility without pinning an automatic button to its current position.
    pub fn set_show(&mut self, id: &str, show: Show) {
        let show = if id == "game.pause" {
            Show::Default
        } else {
            show
        };
        let value = self.0.entry(id.into()).or_insert(Override {
            x: -1.0,
            y: -1.0,
            hidden: false,
            show: Show::Default,
            moved: false,
        });
        value.show = show;
        value.hidden = show == Show::Never;
    }
    /// The legacy Hide/Show action maps to Never/Default.
    pub fn set_hidden(&mut self, id: &str, hidden: bool) {
        self.set_show(id, if hidden { Show::Never } else { Show::Default });
    }
    pub fn set(&mut self, id: &str, rect: Rect, size: (f32, f32), hidden: bool) {
        let mut value = fractions(rect, size, hidden && id != "game.pause");
        if let Some(old) = self.0.get(id).filter(|v| v.hidden == value.hidden) {
            value.show = old.show;
        }
        self.0.insert(id.into(), value);
    }
    /// JSON-quoted id, x/y fractions, hidden boolean, optional Show value, separated by tabs.
    /// Ignore malformed lines; preserve unknown ids for temporarily disabled mods.
    pub fn parse(text: &str) -> Self {
        let mut values = Self::default();
        for line in text.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            if !(4..=5).contains(&fields.len()) {
                continue;
            }
            let Some(mut show) = fields
                .get(4)
                .map_or(Some(Show::Default), |s| Show::parse(s))
            else {
                continue;
            };
            if let [id, x, y, hidden, ..] = fields.as_slice()
                && let (Ok(id), Ok(x), Ok(y), Ok(hidden)) = (
                    serde_json::from_str::<String>(id),
                    x.parse::<f32>(),
                    y.parse::<f32>(),
                    hidden.parse::<bool>(),
                )
                && !id.is_empty()
                && x.is_finite()
                && y.is_finite()
            {
                // -1 -1: shown or hidden where it is, not placed.
                let moved = (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y);
                if !moved && (x, y) != (-1.0, -1.0) {
                    continue;
                }
                if hidden {
                    show = Show::Never;
                }
                if id == "game.pause" {
                    show = Show::Default;
                }
                let hidden = show == Show::Never;
                values.0.insert(
                    id,
                    Override {
                        x,
                        y,
                        hidden,
                        show,
                        moved,
                    },
                );
            }
        }
        values
    }
    pub fn encode(&self) -> String {
        self.0
            .iter()
            .map(|(id, v)| {
                let (x, y) = if v.moved { (v.x, v.y) } else { (-1.0, -1.0) };
                format!(
                    "{}\t{}\t{}\t{}\t{}\n",
                    serde_json::to_string(id).expect("string"),
                    x,
                    y,
                    v.hidden && id != "game.pause",
                    v.show.as_str()
                )
            })
            .collect()
    }
}

/// Abbreviations for one mod's automatic keys. Unique labels retain their original spelling;
/// collisions get a numeric suffix, also avoiding labels already used by other keys.
pub fn labels(names: &[&str]) -> Vec<String> {
    let bases: Vec<String> = names
        .iter()
        .map(|s| {
            let words: Vec<_> = s
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .collect();
            match words.as_slice() {
                [] => "?".into(),
                [word] => word.chars().take(4).collect(),
                _ => words
                    .iter()
                    .filter_map(|s| s.chars().next())
                    .flat_map(char::to_uppercase)
                    .take(4)
                    .collect(),
            }
        })
        .collect();
    let mut used: BTreeSet<String> = bases
        .iter()
        .filter(|s| bases.iter().filter(|b| *b == *s).count() == 1)
        .cloned()
        .collect();
    bases
        .iter()
        .map(|base| {
            if bases.iter().filter(|b| *b == base).count() == 1 {
                return base.clone();
            }
            for n in 1.. {
                let suffix = n.to_string();
                let candidate = format!(
                    "{}{}",
                    base.chars()
                        .take(4usize.saturating_sub(suffix.len()))
                        .collect::<String>(),
                    suffix
                );
                if used.insert(candidate.clone()) {
                    return candidate;
                }
            }
            unreachable!()
        })
        .collect()
}

/// Resolve fixed controls first (including moved auto keys), then flow untouched auto keys.
/// Columns start below the top row, stop before the lower action cluster, and wrap left.
/// Obstacles include all visible fixed controls and mod-authored buttons.
pub fn resolve(size: (f32, f32), entries: &[Entry], overrides: &Overrides) -> Vec<Placed> {
    let mut placed: Vec<_> = entries
        .iter()
        .map(|e| {
            let value = overrides.0.get(&e.id);
            let placed = value.filter(|v| v.moved);
            let default = e.default.or_else(|| placed.map(|_| [0.0, 0.0, 20.0, 20.0]));
            Placed {
                id: e.id.clone(),
                rect: default
                    .map(|r| placed.map_or_else(|| clamp(r, size, false), |v| apply(r, size, *v))),
                hidden: e.id != "game.pause"
                    && (value.map_or(e.hidden_by_default, |v| v.hidden) || !e.visible_now),
            }
        })
        .collect();
    for i in 0..placed.len() {
        if placed[i].rect.is_some() {
            continue;
        }
        'columns: for column in 0..((size.0 / 22.0).ceil() as usize) {
            let x = size.0 - 24.0 - column as f32 * 22.0;
            if x < 2.0 {
                break;
            }
            let mut bottom = size.1 - 26.0;
            for p in &placed {
                if !p.hidden
                    && matches!(
                        p.id.as_str(),
                        "game.drop"
                            | "game.use"
                            | "game.jump"
                            | "game.sneak"
                            | "game.sprint"
                            | "game.attack"
                    )
                    && let Some(r) = p.rect
                    && x < r[0] + r[2] + 2.0
                    && r[0] < x + 22.0
                {
                    bottom = bottom.min(r[1] - 2.0);
                }
            }
            let mut y = 76.0;
            while y + 20.0 <= bottom {
                let candidate = [x, y, 20.0, 20.0];
                if placed
                    .iter()
                    .all(|p| p.hidden || p.rect.is_none_or(|r| !overlaps(candidate, r, 2.0)))
                {
                    placed[i].rect = Some(candidate);
                    break 'columns;
                }
                y += 22.0;
            }
        }
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id: &str, default: Option<Rect>) -> Entry {
        Entry {
            id: id.into(),
            default,
            hidden_by_default: false,
            visible_now: true,
        }
    }
    #[test]
    fn show_settings_round_trip_with_legacy_lines_and_unmoved_marker() {
        let legacy =
            Overrides::parse("\"game.jump\"\t0.25\t0.5\tfalse\n\"mod.old.key\"\t-1\t-1\ttrue\n");
        assert_eq!(legacy.show("game.jump"), Show::Default);
        assert_eq!(legacy.show("mod.old.key"), Show::Never);
        assert!(!legacy.0["mod.old.key"].moved);
        let mut choices = legacy;
        for (id, show) in [
            ("game.jump", Show::Always),
            ("game.sprint", Show::Riding),
            ("mod.a.key", Show::Never),
            ("mod.b.key", Show::Default),
        ] {
            choices.set_show(id, show);
        }
        assert_eq!(Overrides::parse(&choices.encode()), choices);
        choices.set(
            "game.sprint",
            [101.0, 0.0, 32.0, 24.0],
            (320.0, 180.0),
            false,
        );
        assert_eq!(choices.show("game.sprint"), Show::Riding);
        choices.set_hidden("game.sprint", true);
        assert_eq!(choices.show("game.sprint"), Show::Never);
        choices.set_hidden("game.sprint", false);
        assert_eq!(choices.show("game.sprint"), Show::Default);
        assert!(choices.0["game.sprint"].moved);
        assert_eq!(
            Overrides::parse("\"game.jump\"\t-1\t-1\ttrue\talways").show("game.jump"),
            Show::Never
        );
        assert!(Overrides::parse("\"x\"\t-1\t-1\tfalse\tbogus").0.is_empty());
    }

    #[test]
    fn context_hidden_buttons_leave_automatic_space_and_player_show_wins() {
        let mut contextual = entry("mod.dragon.fire", Some([616.0, 76.0, 20.0, 20.0]));
        let auto = entry("mod.other.key", None);
        for (show, riding, visible) in [
            (Show::Default, false, false),
            (Show::Default, true, true),
            (Show::Always, false, true),
            (Show::Riding, false, false),
            (Show::Riding, true, true),
            (Show::Never, true, false),
        ] {
            contextual.visible_now = show.visible(riding, riding);
            let mut overrides = Overrides::default();
            overrides.set_show(&contextual.id, show);
            let p = resolve(
                (640.0, 360.0),
                &[contextual.clone(), auto.clone()],
                &overrides,
            );
            assert_eq!(!p[0].hidden, visible);
            assert_eq!(
                p[1].rect,
                Some(if visible {
                    [616.0, 98.0, 20.0, 20.0]
                } else {
                    [616.0, 76.0, 20.0, 20.0]
                })
            );
        }
        // Always/Riding ignore a failed holding condition as well.
        assert!(Show::Always.visible(false, false));
        assert!(Show::Riding.visible(false, true));
    }

    #[test]
    fn columns_wrap_before_lower_cluster_and_skip_all_obstacles() {
        let mut entries = vec![
            entry("game.pause", Some([616.0, 54.0, 20.0, 20.0])),
            entry("game.drop", Some([602.0, 272.0, 32.0, 24.0])),
            entry("mod.own.key", Some([594.0, 76.0, 20.0, 20.0])),
        ];
        for n in 0..10 {
            entries.push(entry(&format!("mod.auto.{n}"), None));
        }
        let p = resolve((640.0, 360.0), &entries, &Overrides::default());
        assert_eq!(p[3].rect, Some([616.0, 76.0, 20.0, 20.0]));
        assert_eq!(p[10].rect, Some([616.0, 230.0, 20.0, 20.0]));
        assert_eq!(p[11].rect, Some([594.0, 98.0, 20.0, 20.0]));
        for (i, a) in p.iter().enumerate().skip(3) {
            for b in &p[..i] {
                assert!(
                    !overlaps(a.rect.unwrap(), b.rect.unwrap(), 0.0),
                    "{} {}",
                    a.id,
                    b.id
                );
            }
        }
    }
    #[test]
    fn auto_buttons_avoid_every_game_control_and_custom_button_at_phone_sizes() {
        use crate::touch_controls::{Button, Layout, STICK_RADIUS};
        for (width, height) in [(320.0, 180.0), (633.0, 292.0), (320.0, 600.0)] {
            for main_hand_left in [false, true] {
                let layout = Layout {
                    width,
                    height,
                    main_hand_left,
                };
                let mut entries: Vec<_> = Button::ALL
                    .into_iter()
                    .map(|b| entry(b.id(), Some(layout.button(b))))
                    .collect();
                let (x, y) = layout.stick_rest();
                entries.push(entry(
                    "game.stick",
                    Some([
                        x - STICK_RADIUS,
                        y - STICK_RADIUS,
                        2.0 * STICK_RADIUS,
                        2.0 * STICK_RADIUS,
                    ]),
                ));
                entries.push(entry(
                    "mod.custom.key",
                    Some([width - 68.0, 76.0, 20.0, 20.0]),
                ));
                for n in 0..16 {
                    entries.push(entry(&format!("mod.auto.{n}"), None));
                }
                let p = resolve((width, height), &entries, &Overrides::default());
                for (i, a) in p.iter().enumerate().skip(Button::ALL.len() + 2) {
                    let rect = a.rect.expect("phone has space for sixteen auto keys");
                    for b in &p[..i] {
                        assert!(
                            !overlaps(rect, b.rect.unwrap(), 0.0),
                            "{} overlaps {} at {width}x{height}",
                            a.id,
                            b.id
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn small_screens_exhaust_space_without_creating_overlaps() {
        let entries: Vec<_> = (0..30).map(|n| entry(&format!("key{n}"), None)).collect();
        let p = resolve((90.0, 130.0), &entries, &Overrides::default());
        assert!(p.iter().any(|p| p.rect.is_none()));
        for (i, a) in p.iter().enumerate() {
            if let Some(a) = a.rect {
                for b in &p[..i] {
                    if let Some(b) = b.rect {
                        assert!(!overlaps(a, b, 0.0));
                    }
                }
            }
        }
    }
    #[test]
    fn labels_use_characters_initials_and_unique_suffixes_per_mod() {
        assert_eq!(
            labels(&[
                "Map",
                "Waypoints",
                "Show recipes",
                "Full Screen Map",
                "étoile"
            ]),
            ["Map", "Wayp", "SR", "FSM", "étoi"]
        );
        assert_eq!(labels(&["ABCD", "ABCDE", "ABC1"]), ["ABC2", "ABC3", "ABC1"]);
        assert_eq!(labels(&["Show recipes", "Search recipes"]), ["SR1", "SR2"]);
        assert_eq!(labels(&["Show recipes"]), ["SR"]);
    }
    #[test]
    fn overrides_survive_resize_clamp_snap_and_override_mod_positions() {
        let mut o = Overrides::default();
        o.set(
            "mod.a.key",
            [101.0, 39.0, 20.0, 20.0],
            (400.0, 200.0),
            false,
        );
        let e = [entry("mod.a.key", Some([300.0, 0.0, 20.0, 20.0]))];
        assert_eq!(
            resolve((800.0, 400.0), &e, &o)[0].rect,
            Some([204.0, 80.0, 20.0, 20.0])
        );
        assert_eq!(
            apply(
                [0.0, 0.0, 50.0, 50.0],
                (100.0, 80.0),
                Override {
                    x: 0.99,
                    y: 0.99,
                    hidden: false,
                    show: Show::Default,
                    moved: true,
                }
            ),
            [50.0, 30.0, 50.0, 50.0]
        );
    }
    #[test]
    fn automatic_buttons_can_start_hidden_and_be_shown_without_being_pinned() {
        let mut quiet = entry("mod.a.map", None);
        quiet.hidden_by_default = true;
        let e = [quiet, entry("mod.a.zoom", None)];
        let mut o = Overrides::default();
        let p = resolve((640.0, 360.0), &e, &o);
        assert!(p[0].hidden);
        assert!(!p[1].hidden);
        // Shown where it is: it keeps flowing with the automatic buttons.
        o.set_hidden("mod.a.map", false);
        let p = resolve((640.0, 360.0), &e, &o);
        assert!(!p[0].hidden);
        assert!(!o.0["mod.a.map"].moved);
        let flowed = p[0].rect.unwrap();
        assert_eq!(flowed[2], 20.0);
        // Hiding a visible-by-default one is remembered, and survives the file.
        o.set_hidden("mod.a.zoom", true);
        let again = Overrides::parse(&o.encode());
        assert_eq!(again, o);
        let p = resolve((640.0, 360.0), &e, &again);
        assert!(!p[0].hidden && p[1].hidden);
        assert_eq!(p[0].rect, Some(flowed));
        // A moved button stays where the player put it when hidden and shown.
        o.set(
            "mod.a.zoom",
            [100.0, 100.0, 20.0, 20.0],
            (640.0, 360.0),
            false,
        );
        o.set_hidden("mod.a.zoom", true);
        o.set_hidden("mod.a.zoom", false);
        assert!(o.0["mod.a.zoom"].moved);
        assert_eq!(
            resolve((640.0, 360.0), &e, &o)[1].rect,
            Some([100.0, 100.0, 20.0, 20.0])
        );
        // Lines out of range other than the -1 -1 marker are still ignored.
        assert!(Overrides::parse("\"x\"\t-0.5\t0.2\tfalse").0.is_empty());
    }

    #[test]
    fn hidden_buttons_leave_auto_space_pause_cannot_hide_and_reset_restores_defaults() {
        let e = [
            entry("mod.a.key", Some([616.0, 76.0, 20.0, 20.0])),
            entry("mod.a.auto", None),
            entry("game.pause", Some([616.0, 54.0, 20.0, 20.0])),
        ];
        let mut o = Overrides::default();
        o.set("mod.a.key", e[0].default.unwrap(), (640.0, 360.0), true);
        o.set("game.pause", e[2].default.unwrap(), (640.0, 360.0), true);
        let p = resolve((640.0, 360.0), &e, &o);
        assert!(p[0].hidden);
        assert!(!p[2].hidden);
        assert_eq!(p[1].rect, Some([616.0, 76.0, 20.0, 20.0]));
        o.reset("mod.a.key");
        assert!(!resolve((640.0, 360.0), &e, &o)[0].hidden);
        assert!(o.0.contains_key("game.pause"));
        o.reset_all();
        assert!(o.0.is_empty());
    }
    #[test]
    fn pinned_auto_keys_stay_put_as_other_keys_arrive_and_go() {
        let mut o = Overrides::default();
        o.set(
            "mod.a.pinned",
            [100.0, 100.0, 20.0, 20.0],
            (640.0, 360.0),
            false,
        );
        let e = [
            entry("mod.a.first", None),
            entry("mod.a.pinned", None),
            entry("mod.a.last", None),
        ];
        let before = resolve((640.0, 360.0), &e, &o);
        let after = resolve((640.0, 360.0), &e[1..], &o);
        assert_eq!(before[1].rect, after[0].rect);
        assert_eq!(before[0].rect, after[1].rect);
    }
    #[test]
    fn file_round_trip_supports_arbitrary_key_names_and_rejects_bad_coordinates() {
        let mut o = Overrides::default();
        o.set(
            "mod.a.key with\tnewline\n",
            [20.0, 40.0, 20.0, 20.0],
            (400.0, 200.0),
            true,
        );
        assert_eq!(Overrides::parse(&o.encode()), o);
        assert!(
            Overrides::parse("\"bad\"\tNaN\t0\tfalse\n\"bad\"\t-1\t0\tfalse")
                .0
                .is_empty()
        );
        assert!(!Overrides::parse("\"game.pause\"\t0\t0\ttrue").0["game.pause"].hidden);
    }
}

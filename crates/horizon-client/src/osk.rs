//! The on-screen keyboard touch and controller users type with (Horizon's
//! own; docs/controllers.md, "Text fields"): a QWERTY grid and a symbols
//! grid in Horizon's design language, moved over with the D-pad or stick.
//! A types the key, X deletes, Y is a space, Start is Enter, B closes it.
//! It only produces [`Output`]s; the game feeds them to the focused field
//! the way typed keys reach it.
use crate::gamepad::{Button, GlyphStyle};
use crate::ui_theme::{self, Face, Ui, color};

/// What a key press does to the focused field.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Text(String),
    Backspace,
    Enter,
    /// The keyboard closes; the field keeps its text.
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Key {
    Char(char),
    Shift,
    Layer,
    Space,
    Backspace,
    Done,
}

/// A row of keys and their widths in key units (every row is 10 wide).
type Row = &'static [(Key, f32)];

const fn c(ch: char) -> (Key, f32) {
    (Key::Char(ch), 1.0)
}
const BOTTOM: Row = &[
    (Key::Shift, 1.5),
    (Key::Layer, 1.5),
    (Key::Space, 4.0),
    (Key::Backspace, 1.5),
    (Key::Done, 1.5),
];
const LETTERS: [Row; 5] = [
    &[
        c('1'),
        c('2'),
        c('3'),
        c('4'),
        c('5'),
        c('6'),
        c('7'),
        c('8'),
        c('9'),
        c('0'),
    ],
    &[
        c('q'),
        c('w'),
        c('e'),
        c('r'),
        c('t'),
        c('y'),
        c('u'),
        c('i'),
        c('o'),
        c('p'),
    ],
    &[
        c('a'),
        c('s'),
        c('d'),
        c('f'),
        c('g'),
        c('h'),
        c('j'),
        c('k'),
        c('l'),
        c('\''),
    ],
    &[
        c('z'),
        c('x'),
        c('c'),
        c('v'),
        c('b'),
        c('n'),
        c('m'),
        c(','),
        c('.'),
        c('-'),
    ],
    BOTTOM,
];
const SYMBOLS: [Row; 5] = [
    &[
        c('!'),
        c('@'),
        c('#'),
        c('$'),
        c('%'),
        c('^'),
        c('&'),
        c('*'),
        c('('),
        c(')'),
    ],
    &[
        c('/'),
        c('\\'),
        c('_'),
        c('='),
        c('+'),
        c('['),
        c(']'),
        c('{'),
        c('}'),
        c('|'),
    ],
    &[
        c(';'),
        c(':'),
        c('"'),
        c('<'),
        c('>'),
        c('?'),
        c('~'),
        c('`'),
        c('§'),
        c('°'),
    ],
    &[
        c('é'),
        c('è'),
        c('à'),
        c('ç'),
        c('ù'),
        c('ü'),
        c('ö'),
        c('ä'),
        c('ß'),
        c('ñ'),
    ],
    BOTTOM,
];

const UNIT: f32 = 50.0;
const GAP: f32 = 6.0;
const KEY_H: f32 = 38.0;
const WIDTH: f32 = 10.0 * UNIT + 9.0 * GAP;
const HEIGHT: f32 = 5.0 * KEY_H + 4.0 * GAP + 24.0 + 30.0;

fn origin([w, h]: [f32; 2], at_top: bool) -> (f32, f32) {
    (
        ((w - WIDTH) / 2.0).round(),
        if at_top { 16.0 } else { h - HEIGHT - 16.0 },
    )
}

fn key_rect(row: Row, r: usize, col: usize, size: [f32; 2], at_top: bool) -> [f32; 4] {
    let (x, y) = origin(size, at_top);
    let (left, units) = Keyboard::span(row, col);
    [
        x + left * (UNIT + GAP),
        y + r as f32 * (KEY_H + GAP),
        units * UNIT + (units - 1.0) * GAP,
        KEY_H,
    ]
}

fn close_rect(size: [f32; 2], at_top: bool) -> [f32; 4] {
    let (x, y) = origin(size, at_top);
    [x, y + 5.0 * (KEY_H + GAP), WIDTH, 30.0]
}

fn contains([x, y, w, h]: [f32; 4], (px, py): (f32, f32)) -> bool {
    px >= x && px < x + w && py >= y && py < y + h
}

/// The keyboard's state while open.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    row: usize,
    col: usize,
    symbols: bool,
    /// The next letter is upper case (Shift), or every letter (double Shift).
    shift: bool,
    caps: bool,
}
impl Default for Keyboard {
    fn default() -> Self {
        // Focus starts on `q`.
        Self {
            row: 1,
            col: 0,
            symbols: false,
            shift: false,
            caps: false,
        }
    }
}

impl Keyboard {
    fn rows(&self) -> &'static [Row; 5] {
        if self.symbols { &SYMBOLS } else { &LETTERS }
    }
    /// A key's left edge and width in key units.
    fn span(row: Row, col: usize) -> (f32, f32) {
        let left: f32 = row[..col].iter().map(|k| k.1).sum();
        (left, row[col].1)
    }
    fn key(&self) -> Key {
        self.rows()[self.row][self.col].0
    }
    /// A D-pad step: across a row with wrap-around; up and down to the key
    /// under the focused key's centre (wrapping top and bottom).
    pub fn step(&mut self, button: Button) {
        let rows = self.rows();
        let row = rows[self.row];
        match button {
            Button::DPadLeft => self.col = (self.col + row.len() - 1) % row.len(),
            Button::DPadRight => self.col = (self.col + 1) % row.len(),
            Button::DPadUp | Button::DPadDown => {
                let (left, width) = Self::span(row, self.col);
                let centre = left + width / 2.0;
                self.row = if button == Button::DPadUp {
                    (self.row + rows.len() - 1) % rows.len()
                } else {
                    (self.row + 1) % rows.len()
                };
                let next = rows[self.row];
                self.col = (0..next.len())
                    .find(|&i| {
                        let (l, w) = Self::span(next, i);
                        centre < l + w
                    })
                    .unwrap_or(next.len() - 1);
            }
            _ => {}
        }
    }
    fn letter(&self, ch: char) -> String {
        if self.shift || self.caps {
            ch.to_uppercase().collect()
        } else {
            ch.to_string()
        }
    }
    /// A pressed: the focused key's output (Shift and the symbols switch only
    /// change the keyboard).
    pub fn press(&mut self) -> Option<Output> {
        match self.key() {
            Key::Char(ch) => {
                let text = self.letter(ch);
                self.shift = false;
                Some(Output::Text(text))
            }
            Key::Shift => {
                // Shift, then Shift again locks capitals; a third press lets go.
                (self.shift, self.caps) = match (self.shift, self.caps) {
                    (false, false) => (true, false),
                    (true, false) => (false, true),
                    _ => (false, false),
                };
                None
            }
            Key::Layer => {
                self.symbols = !self.symbols;
                self.col = self.col.min(self.rows()[self.row].len() - 1);
                None
            }
            Key::Space => Some(Output::Text(" ".into())),
            Key::Backspace => Some(Output::Backspace),
            Key::Done => Some(Output::Enter),
        }
    }
    /// A pad button while the keyboard is open; `None` when it does nothing
    /// to the field (moving, Shift).
    pub fn button(&mut self, button: Button) -> Option<Output> {
        match button {
            Button::South => self.press(),
            Button::West => Some(Output::Backspace),
            Button::North => Some(Output::Text(" ".into())),
            Button::Start => Some(Output::Enter),
            Button::East => Some(Output::Close),
            Button::LeftStick | Button::LeftTrigger => {
                self.shift = !self.shift;
                None
            }
            Button::RightBumper | Button::LeftBumper => {
                self.symbols = !self.symbols;
                self.col = self.col.min(self.rows()[self.row].len() - 1);
                None
            }
            Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight => {
                self.step(button);
                None
            }
            _ => None,
        }
    }

    /// Pure geometry shared with drawing: gaps do not hit a key.
    fn key_at(&self, size: [f32; 2], at_top: bool, at: (f32, f32)) -> Option<Key> {
        self.rows().iter().enumerate().find_map(|(r, row)| {
            row.iter().enumerate().find_map(|(col, (key, _))| {
                contains(key_rect(row, r, col, size, at_top), at).then_some(*key)
            })
        })
    }

    /// Pure hit test returning the output at a point in the same dp units as
    /// drawing. Shift/layer keys produce no field output. Use `tap` to also
    /// apply the key to the keyboard's selection, shift and layer state.
    pub fn hit_test(&self, size: [f32; 2], at_top: bool, at: (f32, f32)) -> Option<Output> {
        self.clone().tap(size, at_top, at)
    }

    /// Tap a key, applying Shift/layer state and returning the field's output.
    /// This has no window or renderer dependencies; `size` and `at` are in dp.
    pub fn tap(&mut self, size: [f32; 2], at_top: bool, at: (f32, f32)) -> Option<Output> {
        if contains(close_rect(size, at_top), at) {
            return Some(Output::Close);
        }
        let key = self.key_at(size, at_top, at)?;
        let (r, col) = self
            .rows()
            .iter()
            .enumerate()
            .find_map(|(r, row)| row.iter().position(|(k, _)| *k == key).map(|col| (r, col)))?;
        self.row = r;
        self.col = col;
        self.press()
    }

    /// The touch keyboard uses the same keys, with a tappable Close footer.
    pub fn draw_touch(&self, ui: &mut Ui, at_top: bool, glyphs: GlyphStyle) {
        self.draw(ui, at_top, glyphs);
        let [x, y, w, h] = close_rect([ui.w, ui.h], at_top);
        ui.rounded(x, y, w, h, ui_theme::RADIUS, color::BG);
        ui.text_centered(
            Face::Regular,
            ui_theme::SMALL,
            x + w / 2.0,
            y + 2.0,
            color::TEXT,
            "Close",
        );
    }

    /// Draw the keyboard across the bottom of the screen, or the top when
    /// the field it types into is in the lower half.
    pub fn draw(&self, ui: &mut Ui, at_top: bool, glyphs: GlyphStyle) {
        let width = WIDTH;
        let height = HEIGHT;
        let (x0, y0) = origin([ui.w, ui.h], at_top);
        ui.rounded(
            x0 - 12.0,
            y0 - 12.0,
            width + 24.0,
            height,
            ui_theme::RADIUS_PLATE,
            color::with_alpha(color::BG, 0.94),
        );
        ui.border(
            x0 - 12.0,
            y0 - 12.0,
            width + 24.0,
            height,
            ui_theme::RADIUS_PLATE,
            color::BORDER,
        );
        for (r, row) in self.rows().iter().enumerate() {
            for (i, (key, _)) in row.iter().enumerate() {
                let [x, y, w, _] = key_rect(row, r, i, [ui.w, ui.h], at_top);
                let focused = r == self.row && i == self.col;
                let lit = matches!(key, Key::Shift) && (self.shift || self.caps)
                    || matches!(key, Key::Layer) && self.symbols;
                let (plate, ink) = if focused {
                    (color::ACCENT, color::ON_ACCENT)
                } else if lit {
                    (color::HOVER, color::TEXT)
                } else {
                    (color::RAISED, color::TEXT)
                };
                ui.rounded(x, y, w, KEY_H, ui_theme::RADIUS, plate);
                let label = match key {
                    Key::Char(ch) => self.letter(*ch),
                    Key::Shift if self.caps => "CAPS".into(),
                    Key::Shift => "Shift".into(),
                    Key::Layer if self.symbols => "abc".into(),
                    Key::Layer => "&123".into(),
                    Key::Space => "Space".into(),
                    Key::Backspace => "Delete".into(),
                    Key::Done => "Done".into(),
                };
                let lw = ui.measure(Face::Medium, ui_theme::BODY, &label);
                ui.text_line(
                    Face::Medium,
                    ui_theme::BODY,
                    KEY_H,
                    x + ((w - lw) / 2.0).round(),
                    y,
                    ink,
                    &label,
                );
            }
        }
        let hints = [
            (Button::South, "Type"),
            (Button::West, "Delete"),
            (Button::North, "Space"),
            (Button::LeftTrigger, "Shift"),
            (Button::RightBumper, "Symbols"),
            (Button::Start, "Done"),
            (Button::East, "Close"),
        ];
        let line: Vec<String> = hints
            .iter()
            .map(|(b, what)| format!("{} {what}", b.label(glyphs)))
            .collect();
        ui.text_centered(
            Face::Regular,
            ui_theme::SMALL,
            x0 + width / 2.0,
            y0 + 5.0 * (KEY_H + GAP) + 2.0,
            color::MUTED,
            &line.join("   "),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_ten_units_wide() {
        for rows in [&LETTERS, &SYMBOLS] {
            for row in rows {
                let units: f32 = row.iter().map(|k| k.1).sum();
                assert_eq!(units, 10.0);
            }
        }
    }

    #[test]
    fn taps_match_drawn_keys_and_ignore_gaps() {
        let size = [1040.0, 700.0];
        for top in [false, true] {
            let mut k = Keyboard::default();
            for symbols in [false, true] {
                k.symbols = symbols;
                for (r, row) in k.rows().iter().enumerate() {
                    for (col, (key, _)) in row.iter().enumerate() {
                        let [x, y, w, h] = key_rect(row, r, col, size, top);
                        assert_eq!(k.key_at(size, top, (x + w / 2.0, y + h / 2.0)), Some(*key));
                        assert_eq!(k.key_at(size, top, (x + w + GAP / 2.0, y + h / 2.0)), None);
                        assert_eq!(k.key_at(size, top, (x, y + h + GAP / 2.0)), None);
                    }
                }
            }
            assert_eq!(k.key_at(size, top, (-1.0, -1.0)), None);
            let before = k.clone();
            assert_eq!(k.tap(size, top, (f32::NAN, 0.0)), None);
            assert_eq!(k, before);
        }
    }

    #[test]
    fn touch_shift_layer_space_delete_done_and_close() {
        let size = [1040.0, 700.0];
        for top in [false, true] {
            let mut k = Keyboard::default();
            let tap = |k: &mut Keyboard, r, col| {
                let [x, y, w, h] = key_rect(k.rows()[r], r, col, size, top);
                let at = (x + w / 2.0, y + h / 2.0);
                let before = k.clone();
                let expected = k.hit_test(size, top, at);
                assert_eq!(*k, before, "hit testing must not change the keyboard");
                let out = k.tap(size, top, at);
                assert_eq!(out, expected);
                out
            };
            assert_eq!(tap(&mut k, 4, 0), None);
            assert_eq!(tap(&mut k, 1, 0), Some(Output::Text("Q".into())));
            assert_eq!(tap(&mut k, 1, 0), Some(Output::Text("q".into())));
            tap(&mut k, 4, 0);
            tap(&mut k, 4, 0);
            assert_eq!(tap(&mut k, 1, 0), Some(Output::Text("Q".into())));
            assert_eq!(tap(&mut k, 1, 0), Some(Output::Text("Q".into())));
            tap(&mut k, 4, 0);
            tap(&mut k, 4, 1);
            assert_eq!(tap(&mut k, 1, 0), Some(Output::Text("/".into())));
            tap(&mut k, 4, 1);
            assert_eq!(tap(&mut k, 1, 0), Some(Output::Text("q".into())));
            assert_eq!(tap(&mut k, 4, 2), Some(Output::Text(" ".into())));
            assert_eq!(tap(&mut k, 4, 3), Some(Output::Backspace));
            assert_eq!(tap(&mut k, 4, 4), Some(Output::Enter));
            let [x, y, w, h] = close_rect(size, top);
            assert_eq!(
                k.tap(size, top, (x + w / 2.0, y + h / 2.0)),
                Some(Output::Close)
            );
        }
    }

    #[test]
    fn types_shifts_and_moves() {
        let mut k = Keyboard::default();
        assert_eq!(k.press(), Some(Output::Text("q".into())));
        k.step(Button::DPadRight);
        k.button(Button::LeftTrigger);
        assert_eq!(k.press(), Some(Output::Text("W".into())));
        assert_eq!(k.press(), Some(Output::Text("w".into())), "one-shot shift");
        // Left wraps to the end of the row.
        k.col = 0;
        k.step(Button::DPadLeft);
        assert_eq!(k.press(), Some(Output::Text("p".into())));
        // Down from `p` to the row of `'`, then the bottom row: `Done` sits
        // under the right edge; up from the wide Space bar lands mid-row.
        k.step(Button::DPadDown);
        k.step(Button::DPadDown);
        k.step(Button::DPadDown);
        assert_eq!(k.press(), Some(Output::Enter));
        k.col = 2;
        assert_eq!(k.press(), Some(Output::Text(" ".into())));
        k.step(Button::DPadUp);
        assert_eq!(k.press(), Some(Output::Text("n".into())));
        // Up from the top row wraps to the bottom row.
        k.row = 0;
        k.col = 0;
        k.step(Button::DPadUp);
        assert_eq!(k.key(), Key::Shift);
        // Shift twice locks capitals; the symbols grid has no case.
        k.press();
        k.press();
        k.row = 1;
        k.col = 0;
        assert_eq!(k.press(), Some(Output::Text("Q".into())));
        assert_eq!(k.press(), Some(Output::Text("Q".into())));
        k.button(Button::RightBumper);
        assert_eq!(k.press(), Some(Output::Text("/".into())));
        assert_eq!(k.button(Button::West), Some(Output::Backspace));
        assert_eq!(k.button(Button::East), Some(Output::Close));
        assert_eq!(k.button(Button::Start), Some(Output::Enter));
    }
}

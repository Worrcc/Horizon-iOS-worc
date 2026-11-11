//! The touch layout editor shares the resolved controls with play; Options owns pause/blur.
use super::*;
use horizon_client::{text::Text, widgets};

#[derive(Default)]
pub(super) struct Editor {
    selected: Option<String>,
    pointer: Option<Drag>,
    finger: Option<u64>,
}
struct Drag {
    item: Option<String>,
    origin: Rect,
    start: (f32, f32),
    action: Option<usize>,
    moved: bool,
}
const NAME_Y: f32 = 44.0;
const BAND_HEIGHT: f32 = NAME_Y + 12.0;

fn band_offset(size: (f32, f32), bottom: bool) -> f32 {
    if bottom {
        (size.1 - BAND_HEIGHT).max(0.0)
    } else {
        0.0
    }
}
fn toolbar(size: (f32, f32), bottom: bool) -> [Rect; 5] {
    let show_width = 86.0_f32.min((size.0 - 12.0).max(0.0));
    let w = ((size.0 - 12.0 - show_width) / 4.0).clamp(0.0, 58.0);
    let left = (size.0 - (4.0 * w + show_width + 8.0)) / 2.0;
    std::array::from_fn(|i| {
        [
            left + i as f32 * (w + 2.0),
            20.0 + band_offset(size, bottom),
            if i == 4 { show_width } else { w },
            20.0,
        ]
    })
}
impl Editor {
    fn bottom(&self, items: &[TouchItem]) -> bool {
        items
            .iter()
            .find(|i| self.selected.as_deref() == Some(&i.id))
            .and_then(|i| i.rect)
            .is_some_and(|r| r[1] < BAND_HEIGHT)
    }
    fn press(&mut self, items: &[TouchItem], size: (f32, f32), at: (f32, f32)) {
        let bar = toolbar(size, self.bottom(items));
        let action = bar.iter().position(|&r| touch_layout::contains(r, at));
        // The gaps belong to the bar too. A press on it never captures a control below it.
        let on_bar = touch_layout::contains(
            [
                bar[0][0],
                bar[0][1],
                bar[4][0] + bar[4][2] - bar[0][0],
                bar[0][3],
            ],
            at,
        );
        let hit = if on_bar {
            None
        } else {
            items
                .iter()
                .rev()
                .find(|item| item.rect.is_some_and(|r| touch_layout::contains(r, at)))
        };
        if !on_bar {
            self.selected = hit.map(|i| i.id.clone());
        }
        self.pointer = Some(Drag {
            item: hit.map(|i| i.id.clone()),
            origin: hit.and_then(|i| i.rect).unwrap_or([0.0; 4]),
            start: at,
            action,
            moved: false,
        });
    }
}

impl Drag {
    fn rect(&mut self, at: (f32, f32), size: (f32, f32)) -> Option<Rect> {
        self.item.as_ref()?;
        let dx = at.0 - self.start.0;
        let dy = at.1 - self.start.1;
        if !self.moved && dx.hypot(dy) < horizon_client::touch_controls::TAP_SLOP {
            return None;
        }
        self.moved = true;
        let mut rect = self.origin;
        rect[0] += dx;
        rect[1] += dy;
        Some(touch_layout::clamp(rect, size, true))
    }
}
impl Game<'_> {
    pub(in crate::game) fn cancel_touch_editor_pointer(&mut self) {
        if let Some(editor) = &mut self.touch.editor {
            editor.pointer = None;
            editor.finger = None;
            self.save_touch_layout();
        }
    }
    pub(in crate::game) fn touch_editor_open(&self) -> bool {
        self.screen == Screen::Options && self.touch.editor.is_some()
    }
    pub(in crate::game) fn open_touch_editor(&mut self) -> horizon_client::Result<()> {
        if !enabled() {
            return Err("touch controls are unavailable (set HORIZON_TOUCH=1 on desktop)".into());
        }
        if self.screen != Screen::Options {
            self.capture(false);
            self.open_options_at(
                self.title.is_some(),
                horizon_client::options_ui::Page::Controls,
            );
        }
        #[cfg(feature = "mods")]
        self.release_mod_touches();
        self.touch.controls.release_all();
        self.keys.clear();
        self.physical_keys.clear();
        self.left = false;
        self.right = false;
        self.show_keyboard(false);
        self.touch.editor = Some(Editor::default());
        Ok(())
    }
    pub(in crate::game) fn save_touch_layout(&self) {
        let path = Self::options_path().with_file_name("horizon-touch.txt");
        let tmp = path.with_extension("txt.tmp");
        if let Err(error) = std::fs::write(&tmp, self.touch.overrides.encode())
            .and_then(|_| std::fs::rename(&tmp, &path))
        {
            log::warn!("Horizon: saving touch layout {}: {error}", path.display());
        }
    }
    pub(in crate::game) fn close_touch_editor(&mut self) {
        self.save_touch_layout();
        self.touch.editor = None;
        self.dragging = false;
    }
    pub(in crate::game) fn draw_touch_editor(&self, batch: &mut HudBatch) {
        let Some(editor) = &self.touch.editor else {
            return;
        };
        let atlas = &self.gui.atlas;
        ui::menu_background(
            batch,
            atlas,
            0.0,
            0.0,
            batch.width,
            batch.height,
            self.player.is_some(),
        );
        let layout = Layout {
            width: batch.width,
            height: batch.height,
            main_hand_left: self.settings.main_hand_left,
        };
        let items = self.touch_items(&layout);
        for item in &items {
            let Some(rect) = item.rect else { continue };
            let start = batch.vertices.len();
            self.draw_touch_item(batch, item, &layout, true);
            if item.hidden {
                for v in &mut batch.vertices[start..] {
                    v.color = (v.color & 0x00FFFFFF) | (((v.color >> 24) / 3) << 24);
                }
            }
            draw_button_selection(
                batch,
                atlas,
                rect,
                SLOT_FRAME,
                editor.selected.as_deref() == Some(&item.id),
            );
            widgets::record(&item.name, rect, true);
        }
        if editor
            .pointer
            .as_ref()
            .is_some_and(|p| p.item.is_some() && p.moved)
        {
            return;
        }
        let size = (batch.width, batch.height);
        let offset = band_offset(size, editor.bottom(&items));
        let title = lang::get("horizon.touch.edit_title");
        batch.text(
            atlas,
            &title,
            (batch.width - HudBatch::text_width(atlas, &title)) / 2.0,
            6.0 + offset,
            0xFFFFFFFF,
            true,
        );
        let selected = items
            .iter()
            .find(|i| editor.selected.as_deref() == Some(&i.id));
        let name = selected.map_or_else(|| lang::get("horizon.touch.select"), |i| i.name.clone());
        batch.text(
            atlas,
            &name,
            (batch.width - HudBatch::text_width(atlas, &name)) / 2.0,
            NAME_Y + offset,
            0xFFFFFFFF,
            true,
        );
        let labels = [
            if selected.is_some_and(|i| i.hidden) {
                "horizon.touch.show"
            } else {
                "horizon.touch.hide"
            },
            "controls.reset",
            "horizon.touch.reset_all",
            "gui.done",
            match selected
                .map(|i| self.touch.overrides.show(&i.id))
                .unwrap_or_default()
            {
                Show::Default => "horizon.touch.show.default",
                Show::Always => "horizon.touch.show.always",
                Show::Riding => "horizon.touch.show.riding",
                Show::Never => "horizon.touch.show.never",
            },
        ];
        for (i, [x, y, w, h]) in toolbar(size, editor.bottom(&items)).into_iter().enumerate() {
            let active = match i {
                0 | 4 => selected.is_some_and(|i| i.id != "game.pause"),
                1 => selected.is_some(),
                _ => true,
            };
            ui::button(
                batch,
                atlas,
                x,
                y,
                w,
                h,
                &Text::tr(labels[i], &[]),
                active,
                ui::hovered(self.cursor, x, y, w, h),
            );
        }
    }
    pub(in crate::game) fn touch_editor_pointer(&mut self, down: bool) {
        let Some((layout, _)) = self.touch_layout() else {
            return;
        };
        let items = self.touch_items(&layout);
        let size = (layout.width, layout.height);
        let at = self.cursor;
        let Some(editor) = &mut self.touch.editor else {
            return;
        };
        if down {
            editor.press(&items, size, at);
            return;
        }
        let Some(pointer) = editor.pointer.take() else {
            return;
        };
        if !pointer.moved
            && let Some(action) = pointer
                .action
                .filter(|&i| touch_layout::contains(toolbar(size, editor.bottom(&items))[i], at))
        {
            let selected = items
                .iter()
                .find(|i| editor.selected.as_deref() == Some(&i.id));
            match action {
                0 => {
                    if let Some(item) = selected
                        && item.id != "game.pause"
                    {
                        self.touch.overrides.set_hidden(&item.id, !item.hidden);
                    }
                }
                1 => {
                    if let Some(item) = selected {
                        self.touch.overrides.reset(&item.id);
                    }
                }
                2 => self.touch.overrides.reset_all(),
                3 => {
                    self.close_touch_editor();
                    return;
                }
                4 => {
                    if let Some(item) = selected.filter(|i| i.id != "game.pause") {
                        let show = self.touch.overrides.show(&item.id).next();
                        self.touch.overrides.set_show(&item.id, show);
                    }
                }
                _ => {}
            }
        }
        self.save_touch_layout();
    }
    pub(in crate::game) fn touch_editor_drag(&mut self) {
        let size = self.gui_size();
        let Some(editor) = &mut self.touch.editor else {
            return;
        };
        let Some(drag) = &mut editor.pointer else {
            return;
        };
        let Some(rect) = drag.rect(self.cursor, size) else {
            return;
        };
        let Some(id) = &drag.item else { return };
        editor.selected = Some(id.clone());
        let hidden = self.touch.overrides.0.get(id).is_some_and(|v| v.hidden);
        self.touch.overrides.set(id, rect, size, hidden);
    }
    pub(in crate::game) fn touch_editor_touch(
        &mut self,
        id: u64,
        phase: TouchPhase,
        at: (f32, f32),
    ) {
        let Some(editor) = &mut self.touch.editor else {
            return;
        };
        match phase {
            TouchPhase::Started if editor.finger.is_none() => {
                editor.finger = Some(id);
                self.cursor = at;
                self.touch_editor_pointer(true);
            }
            TouchPhase::Moved if editor.finger == Some(id) => {
                self.cursor = at;
                self.touch_editor_drag();
            }
            TouchPhase::Ended if editor.finger == Some(id) => {
                editor.finger = None;
                self.cursor = at;
                self.touch_editor_pointer(false);
            }
            TouchPhase::Cancelled if editor.finger == Some(id) => {
                editor.finger = None;
                editor.pointer = None;
                self.save_touch_layout();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, rect: Rect) -> TouchItem {
        TouchItem {
            id: id.into(),
            name: id.into(),
            label: String::new(),
            rect: Some(rect),
            hidden: false,
            default_visible: true,
            condition: String::new(),
            kind: TouchKind::Stick,
        }
    }

    #[test]
    fn toolbar_is_centred_below_title_with_name_underneath_at_phone_sizes() {
        for size in [(320.0, 180.0), (854.0, 480.0), (200.0, 600.0)] {
            let bar = toolbar(size, false);
            assert_eq!(bar[0][0], size.0 - bar[4][0] - bar[4][2]);
            for (i, rect) in bar.iter().enumerate() {
                assert_eq!(rect[1], 20.0);
                assert_eq!(rect[3], 20.0);
                assert!(rect[0] >= 0.0 && rect[0] + rect[2] <= size.0);
                assert!(rect[1] + rect[3] < NAME_Y);
                if i > 0 {
                    assert_eq!(rect[0] - bar[i - 1][0] - bar[i - 1][2], 2.0);
                }
            }
        }
    }

    #[test]
    fn default_stick_and_its_lower_edge_are_available_for_dragging() {
        for main_hand_left in [false, true] {
            let size = (320.0, 180.0);
            let layout = Layout {
                width: size.0,
                height: size.1,
                main_hand_left,
            };
            let (x, y) = layout.stick_rest();
            let r = horizon_client::touch_controls::STICK_RADIUS;
            let stick = item("game.stick", [x - r, y - r, 2.0 * r, 2.0 * r]);
            let mut editor = Editor::default();
            // The old selected-name line crossed the stick near its lower edge.
            for at in [(x, y), (x, size.1 - 36.0), (x, y + r - 1.0)] {
                assert!(touch_layout::contains(stick.rect.unwrap(), at));
                editor.press(std::slice::from_ref(&stick), size, at);
                let drag = editor.pointer.as_mut().unwrap();
                assert_eq!(drag.item.as_deref(), Some("game.stick"));
                assert_eq!(drag.action, None);
                assert!(drag.rect((at.0 + 10.0, at.1 - 10.0), size).is_some());
            }
            // Any control in the old footer strip can also be selected now.
            editor.press(
                &[item("game.jump", [4.0, size.1 - 28.0, 58.0, 24.0])],
                size,
                (20.0, size.1 - 14.0),
            );
            assert_eq!(
                editor.pointer.as_ref().unwrap().item.as_deref(),
                Some("game.jump")
            );
        }
    }

    #[test]
    fn toolbar_and_its_gaps_never_drag_the_control_beneath_them() {
        let size = (640.0, 360.0);
        let bar = toolbar(size, false);
        let mut editor = Editor {
            selected: Some("game.jump".into()),
            ..Editor::default()
        };
        for (i, rect) in bar.into_iter().enumerate() {
            let underneath = item(
                "game.stick",
                [rect[0] - 2.0, rect[1] - 2.0, rect[2] + 4.0, rect[3] + 4.0],
            );
            editor.press(&[underneath], size, (rect[0] + 4.0, rect[1] + 4.0));
            assert_eq!(editor.selected.as_deref(), Some("game.jump"));
            let drag = editor.pointer.as_mut().unwrap();
            assert_eq!(drag.action, Some(i));
            assert!(drag.rect((rect[0] + 20.0, 100.0), size).is_none());
            assert!(drag.item.is_none());
        }
        let gap = (bar[0][0] + bar[0][2] + 1.0, 25.0);
        editor.press(
            &[item("game.stick", [gap.0 - 5.0, 20.0, 20.0, 20.0])],
            size,
            gap,
        );
        assert!(editor.pointer.as_ref().unwrap().item.is_none());
        assert_eq!(editor.selected.as_deref(), Some("game.jump"));
    }

    #[test]
    fn a_control_can_be_dragged_to_y_zero_and_grabbed_again() {
        let size = (320.0, 180.0);
        let mut editor = Editor::default();
        let control = item("game.jump", [100.0, 100.0, 20.0, 20.0]);
        editor.press(std::slice::from_ref(&control), size, (110.0, 110.0));
        let rect = editor
            .pointer
            .as_mut()
            .unwrap()
            .rect((211.0, 1.0), size)
            .unwrap();
        assert_eq!(rect, [202.0, 0.0, 20.0, 20.0]);
        let mut overrides = Overrides::default();
        overrides.set("game.jump", rect, size, false);
        let saved = touch_layout::apply(control.rect.unwrap(), size, overrides.0["game.jump"]);
        editor.press(
            &[item("game.jump", saved)],
            size,
            (saved[0] + 5.0, saved[1] + 5.0),
        );
        assert_eq!(
            editor.pointer.as_ref().unwrap().item.as_deref(),
            Some("game.jump")
        );
    }

    #[test]
    fn large_controls_still_fit_and_can_be_grabbed_on_short_screens() {
        let size = (320.0, 80.0);
        let mut editor = Editor::default();
        editor.press(
            &[item("game.stick", [0.0, 16.0, 64.0, 64.0])],
            size,
            (10.0, 60.0),
        );
        let rect = editor
            .pointer
            .as_mut()
            .unwrap()
            .rect((10.0, -100.0), size)
            .unwrap();
        assert_eq!(rect, [0.0, 0.0, 64.0, 64.0]);
        editor.press(&[item("game.stick", rect)], size, (50.0, 10.0));
        assert_eq!(
            editor.pointer.as_ref().unwrap().item.as_deref(),
            Some("game.stick")
        );
    }

    #[test]
    fn bar_flips_to_bottom_when_selection_is_at_top_and_routes_presses() {
        let size = (320.0, 180.0);
        let top = item("game.jump", [100.0, 0.0, 32.0, 24.0]);
        let mut editor = Editor {
            selected: Some(top.id.clone()),
            ..Editor::default()
        };
        assert!(editor.bottom(std::slice::from_ref(&top)));
        let bar = toolbar(size, true);
        assert!(bar[0][1] > size.1 - BAND_HEIGHT);
        assert_eq!(NAME_Y + band_offset(size, true), size.1 - 12.0);
        for (action, rect) in bar.into_iter().enumerate() {
            let underneath = item("game.stick", rect);
            editor.press(
                &[item("game.jump", top.rect.unwrap()), underneath],
                size,
                (rect[0] + 1.0, rect[1] + 1.0),
            );
            let pointer = editor.pointer.as_ref().unwrap();
            assert_eq!(pointer.action, Some(action));
            assert!(pointer.item.is_none());
            assert_eq!(editor.selected.as_deref(), Some("game.jump"));
        }
        let gap = (bar[0][0] + bar[0][2] + 1.0, bar[0][1] + 1.0);
        editor.press(
            &[
                item("game.jump", top.rect.unwrap()),
                item("game.stick", [gap.0 - 2.0, gap.1 - 2.0, 10.0, 10.0]),
            ],
            size,
            gap,
        );
        assert!(editor.pointer.as_ref().unwrap().item.is_none());
        editor.press(&[top], size, (110.0, 10.0));
        assert_eq!(
            editor.pointer.as_ref().unwrap().item.as_deref(),
            Some("game.jump")
        );
    }
}

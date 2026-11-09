//! WASM browser bookmarks, left of the container, sharing the browser's slots.
use super::{
    Game,
    native_hooks::NativeStack as BrowserStack,
    native_panel::{COLUMNS, Layout, MARGIN, Owner, SLOT, inside},
};
use horizon_client::{hud::HudBatch, items::ItemStack};

#[derive(Default)]
pub(super) struct Bookmarks {
    pub(super) of: Option<(usize, u64)>,
    pub(super) items: Vec<BrowserStack>,
    pub(super) stacks: Vec<Option<ItemStack>>,
    page: usize,
}

fn left_layout(window_left: f32, height: f32) -> Option<Layout> {
    let right = window_left - MARGIN;
    let width = right - MARGIN;
    let columns = ((width / SLOT).floor().max(0.0) as usize).min(COLUMNS.0);
    let top = MARGIN + 17.0 + 4.0;
    let rows = ((height - MARGIN - top) / SLOT).floor().max(0.0) as usize;
    (columns >= COLUMNS.1 && rows > 0).then_some(Layout {
        area: [MARGIN, 0.0, width, height],
        grid: (
            (MARGIN + (width - columns as f32 * SLOT) / 2.0).floor(),
            top,
        ),
        columns,
        rows,
        arrows_y: MARGIN,
        search: [0.0; 4],
    })
}

impl Game<'_> {
    pub(super) fn bookmarks_layout(&self, batch: &HudBatch) -> Option<Layout> {
        if self.panel.bookmarks.items.is_empty() || self.recipe_book_open() {
            return None;
        }
        left_layout(self.container_image(batch).left as f32, batch.height)
    }

    pub(super) fn bookmarks_hovered(
        &self,
        layout: &Layout,
    ) -> Option<(usize, BrowserStack, ItemStack)> {
        let b = &self.panel.bookmarks;
        let page = b
            .page
            .min(b.stacks.len().div_ceil(layout.per_page()).saturating_sub(1));
        (0..layout.per_page()).find_map(|n| {
            let (x, y) = layout.slot(n);
            let index = page * layout.per_page() + n;
            inside(self.cursor, [x, y, SLOT, SLOT]).then_some(())?;
            Some((
                index,
                b.items.get(index)?.clone(),
                b.stacks.get(index)?.clone()?,
            ))
        })
    }

    pub(super) fn draw_browser_bookmarks(&mut self, batch: &mut HudBatch) -> Option<ItemStack> {
        let layout = self.bookmarks_layout(batch)?;
        let b = &mut self.panel.bookmarks;
        let pages = b.stacks.len().div_ceil(layout.per_page()).max(1);
        b.page = b.page.min(pages - 1);
        let page = b.page;
        let stacks = b
            .stacks
            .iter()
            .skip(page * layout.per_page())
            .take(layout.per_page())
            .cloned()
            .collect::<Vec<_>>();
        self.draw_browser_grid(batch, &layout, &stacks, page, pages);
        self.bookmarks_hovered(&layout).map(|(_, _, stack)| stack)
    }

    pub(super) fn bookmarks_press(&mut self, batch: &HudBatch, owner: &Owner, right: bool) -> bool {
        let Owner::Wasm(owner) = owner else {
            return false;
        };
        let Some(layout) = self.bookmarks_layout(batch) else {
            return false;
        };
        if !inside(self.cursor, layout.area) {
            return false;
        }
        let b = &mut self.panel.bookmarks;
        let pages = b.stacks.len().div_ceil(layout.per_page()).max(1);
        b.page = b.page.min(pages - 1);
        for (i, (x, y)) in layout.arrows().into_iter().enumerate() {
            if inside(self.cursor, [x, y, 12.0, 17.0])
                && ((i == 0 && b.page > 0) || (i == 1 && b.page + 1 < pages))
            {
                b.page = if i == 0 { b.page - 1 } else { b.page + 1 };
                self.audio
                    .play("minecraft:ui.button.click", None, 0.25, 1.0);
                return true;
            }
        }
        if let Some((index, _, _)) = self.bookmarks_hovered(&layout) {
            self.mods.mods.browser_ext_event(
                *owner,
                horizon_mods::BrowserExtEvent::BookmarkClicked(horizon_mods::BrowserClick {
                    index: index as u32,
                    right,
                }),
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_panel_bookmarks_fits_left_of_window() {
        let layout = left_layout(125.0, 240.0).unwrap();
        assert_eq!((layout.columns, layout.rows), (6, 11));
        assert_eq!(layout.grid, (8.0, 29.0));
        assert!(layout.grid.0 + layout.width() <= 117.0);
        assert_eq!(left_layout(800.0, 240.0).unwrap().columns, 12);
        assert_eq!(left_layout(52.0, 240.0).unwrap().columns, 2);
        assert!(left_layout(51.0, 240.0).is_none());
        assert!(left_layout(125.0, 54.0).is_none());
    }
}

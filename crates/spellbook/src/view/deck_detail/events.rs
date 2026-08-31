//! Event-path helpers for the deck detail screen: the two rect conversions
//! that keep the content and overlay event paths in agreement, the confirm
//! button pair, and the ownership tag.

use engine::ui::widgets::{Rect, WidgetEvent};
use spellbook_core::client::Command;

use super::super::ScreenCtx;
use super::{SortMode, GroupBy, ViewMode};

// ---------------------------------------------------------------------------
// Event rect helpers
// ---------------------------------------------------------------------------

/// Simulated window for events coming down the content path (the shell
/// passes the content rect): a generous band below for confirm buttons.
pub(crate) fn content_to_window(content: Rect) -> Rect {
    Rect::new(
        content.x,
        content.y,
        content.w.max(400.0),
        (content.h + content.y).max(500.0),
    )
}

/// The Confirmar / Cancelar buttons of both dialogs, centred at the bottom
/// of the window band.
pub(crate) fn confirm_buttons(window: Rect) -> (Rect, Rect) {
    let cx = window.x + window.w / 2.0;
    let by = window.y + window.h / 2.0 + 40.0;
    (
        Rect::new(cx - 230.0, by, 140.0, 42.0),
        Rect::new(cx + 90.0, by, 120.0, 42.0),
    )
}


/// The ownership tag's (label, color), mirroring the JS's ownTag classes.
pub(crate) fn ownership_tag(o: &spellbook_core::wizard::CardOwnership) -> (String, [f32; 4]) {
    use spellbook_core::wizard::OwnershipStatus;
    match o.status {
        OwnershipStatus::Missing => ("Não tenho".to_string(), [0.83, 0.30, 0.27, 1.0]),
        OwnershipStatus::OwnedInDeck => (o.deck.clone().unwrap_or_else(|| "Em outro deck".into()), [0.95, 0.71, 0.19, 1.0]),
        OwnershipStatus::OwnedFree => ("Livre na coleção".to_string(), [0.36, 0.72, 0.44, 1.0]),
        OwnershipStatus::OwnedHere => (String::new(), [0.36, 0.72, 0.44, 1.0]),
    }
}

impl super::DeckDetailScreen {
    // -- Toolbar / event routing ----------------------------------------------

    pub(crate) fn export_menu_rect(&self, content: Rect) -> Rect {
        let t = self.toolbar_rects(content);
        Rect::new(t.sort.x + 150.0, t.sort.y + t.sort.h, 220.0, 78.0)
    }

    pub(crate) fn sort_menu_rect(&self, content: Rect) -> Rect {
        let t = self.toolbar_rects(content);
        Rect::new(
            t.sort.x,
            t.sort.y + t.sort.h + 4.0,
            220.0,
            34.0 * SortMode::ALL.len() as f32 + 8.0,
        )
    }

    /// Toolbar pointer routing, shared by the page and overlay paths.
    pub(crate) fn toolbar_click(&mut self, x: f32, y: f32, content: Rect, _ctx: &mut ScreenCtx) -> bool {
        let t = self.toolbar_rects(content);
        if self.export_menu_open {
            let menu = self.export_menu_rect(content);
            if menu.contains(x, y) {
                let format = if y < menu.y + menu.h / 2.0 {
                    "moxfield"
                } else {
                    "text"
                };
                self.export_format = Some(format.to_string());
                if let Some(tx) = &self.tx
                    && let Some(id) = self.deck_id
                {
                    let _ = tx.send(Command::ExportDeck {
                        deck_id: id,
                        format: format.to_string(),
                    });
                }
                self.export_menu_open = false;
                return true;
            }
            self.export_menu_open = false;
            return true;
        }
        if self.sort_menu_open {
            let menu = self.sort_menu_rect(content);
            if menu.contains(x, y) {
                let i = ((y - menu.y - 4.0) / 34.0).clamp(0.0, 4.0) as usize;
                self.sort = SortMode::from_index(i);
            }
            self.sort_menu_open = false;
            return true;
        }
        if t.filter.contains(x, y) {
            let r = self
                .filter_bar
                .handle_event(&WidgetEvent::MouseDown { x, y }, t.filter, content);
            return r.clicked || r.changed;
        }
        for (i, r) in t.views.iter().enumerate() {
            if r.contains(x, y) {
                self.view_mode = ViewMode::from_index(i);
                return true;
            }
        }
        for (i, r) in t.groups.iter().enumerate() {
            if r.contains(x, y) {
                self.group_by = GroupBy::from_index(i);
                return true;
            }
        }
        if t.sort.contains(x, y) {
            self.sort_menu_open = true;
            return true;
        }
        false
    }
}

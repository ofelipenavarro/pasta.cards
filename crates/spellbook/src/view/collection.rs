//! The collection: every physical copy, free or sleeved in a deck.
//!
//! Port of the web UI's `#collection` view (`desktop/ui/js/views/collection.js`).
//! Shows a searchable, filterable grid of collection entries with artwork,
//! quantity and allocation badges, plus the add-card and card-detail modals.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use engine::compositor::{Compositor, LayerId, SceneNode};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{
    Button, Chip, EmptyState, EventResult, Rect, WidgetEvent, rounded_rect, rounded_rect_stroke,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::images;
use spellbook_core::ops::collection::CollectionEntry;

use super::{EditKey, Route, ScreenCtx, grid_columns, text};
use crate::art::ArtCache;
use crate::view::components::add_card::{AddCardAnswer, AddCardModal};
use crate::view::components::card_modal::{CardModal, CardModalAnswer};
use crate::view::components::filters::{FilterBar, matches_filters};
use crate::view::components::search_field::SearchField;

const SEARCH_H: f32 = 52.0; // LabeledField::height()
const SEARCH_W: f32 = 260.0;
const CHIP_H: f32 = 24.0;
const CHIP_GAP: f32 = 8.0;
const FILTER_TOGGLE_W: f32 = 40.0;
const FILTER_TOGGLE_H: f32 = 36.0;
const GRID_GAP: f32 = 16.0;
const TILE_MIN_W: f32 = 160.0;
const TILE_MAX_W: f32 = 240.0;
/// MTG normal card aspect: 488 × 680.
const CARD_ASPECT: f32 = 680.0 / 488.0;
/// Space under the art for the card name and allocation badges.
const BODY_H: f32 = 56.0;
const SEARCH_DEBOUNCE: f32 = 0.250;

/// What the pointer is over. Layout functions below provide the rects, so
/// hover/click never disagree with the pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    AddButton,
    StatusChip(usize),
    Card(usize),
}

pub struct CollectionScreen {
    entries: Vec<CollectionEntry>,
    status: String,
    search: String,
    search_field: SearchField,
    status_chips: [Chip; 3],
    filter_bar: FilterBar,
    add_btn: Button,
    add_card_modal: AddCardModal,
    add_card_open: bool,
    card_modal: Option<CardModal>,
    hover: Option<Hit>,
    debounce: f32,
    search_dirty: bool,
    loading: bool,
    empty: EmptyState,
    loading_empty: EmptyState,
    filter_empty: EmptyState,
    /// Content rect of the last layout pass, so overlay-side handling of the
    /// filter menu computes the same geometry the render drew.
    last_content: Rect,
    /// Command sender captured on enter so `tick` can fire debounced reloads.
    tx: Option<Sender<Command>>,
}

impl CollectionScreen {
    pub fn new(theme: &Theme) -> Self {
        let mut search_field = SearchField::new_without_callback(
            "Buscar carta (PT ou EN)…",
            theme,
        );
        search_field.focus();
        Self {
            entries: Vec::new(),
            status: "all".to_string(),
            search: String::new(),
            search_field,
            status_chips: [
                Chip::new("Todas").selected(true).interactive(true),
                Chip::new("Em decks").interactive(true),
                Chip::new("Livres").interactive(true),
            ],
            filter_bar: FilterBar::new(theme, true),
            add_btn: Button::new("+ Adicionar carta"),
            add_card_modal: AddCardModal::new(theme),
            add_card_open: false,
            card_modal: None,
            hover: None,
            debounce: 0.0,
            search_dirty: false,
            loading: true,
            empty: EmptyState::new(
                "Coleção vazia",
                "Adicione cartas uma a uma ou colando uma lista de nomes.",
            )
            .icon("book-open"),
            loading_empty: EmptyState::new(
                "Carregando coleção…",
                "Lendo as cópias do banco local.",
            )
            .icon("book-open"),
            filter_empty: EmptyState::new(
                "Nenhuma carta corresponde aos filtros",
                "Ajuste ou limpe os filtros para ver mais cartas.",
            )
            .icon("book-open"),
            last_content: Rect::new(0.0, 0.0, 800.0, 600.0),
            tx: None,
        }
    }

    pub fn on_enter(&mut self, _route: Route, ctx: &mut ScreenCtx) {
        self.loading = true;
        self.entries.clear();
        self.tx = Some(ctx.tx.clone());
        ctx.send(Command::ListCollection {
            status: self.status.clone(),
            q: self.search.clone(),
        });
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = false;
        if let Some(modal) = &mut self.card_modal {
            changed |= modal.on_event(event, ctx);
        }
        changed |= self.add_card_modal.on_event(event, ctx);

        if let Event::CollectionListed(list) = event {
            self.entries = list.clone();
            self.entries.sort_by(|a, b| a.card_name.cmp(&b.card_name));
            self.loading = false;
            return true;
        }
        changed
    }

    pub fn handle_text(&mut self, s: &str, _ctx: &mut ScreenCtx) -> bool {
        if self.add_card_open {
            return self.add_card_modal.handle_text(s);
        }
        if let Some(modal) = &mut self.card_modal {
            return modal.handle_text(s);
        }
        let consumed = self.search_field.handle_text(s);
        if consumed {
            let value = self.search_field.value().to_string();
            if value != self.search {
                self.search = value;
                self.search_dirty = true;
                self.debounce = SEARCH_DEBOUNCE;
            }
        }
        consumed
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> bool {
        if self.add_card_open {
            return self.add_card_modal.handle_edit_key(key, ctx).changed;
        }
        if let Some(modal) = &mut self.card_modal {
            return modal.handle_edit_key(key, ctx).changed;
        }
        let old = self.search_field.value().to_string();
        let consumed = self.search_field.handle_edit_key(key);
        if consumed {
            let value = self.search_field.value().to_string();
            if value != self.search || value != old {
                self.search = value;
                self.search_dirty = true;
                self.debounce = SEARCH_DEBOUNCE;
            }
        }
        consumed
    }

    pub fn handle_escape(&mut self) -> bool {
        if self.add_card_open {
            if self.add_card_modal.handle_escape() {
                return true;
            }
            self.add_card_open = false;
            return true;
        }
        if let Some(modal) = &mut self.card_modal {
            if modal.handle_escape() {
                return true;
            }
            self.card_modal = None;
            return true;
        }
        false
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let mut animating = false;
        animating |= self.search_field.tick(dt);

        let mut dummy_actions = Vec::new();
        let mut ctx = ScreenCtx {
            tx: self.tx.as_ref().expect("tx set on enter"),
            actions: &mut dummy_actions,
        };
        if self.add_card_open {
            animating |= self.add_card_modal.tick(dt, &mut ctx);
        }
        if let Some(modal) = &mut self.card_modal {
            animating |= modal.tick(dt, &mut ctx);
        }

        if self.search_dirty {
            self.debounce -= dt;
            if self.debounce <= 0.0 {
                self.search_dirty = false;
                ctx.send(Command::ListCollection {
                    status: self.status.clone(),
                    q: self.search.clone(),
                });
            }
        }
        animating
    }

    /// Filtered view: server search already applied; the chip filters run
    /// client-side over the loaded list, exactly as the JS did.
    fn visible_entries(&self) -> Vec<&CollectionEntry> {
        self.entries
            .iter()
            .filter(|e| matches_filters(*e, &self.filter_bar.state))
            .collect()
    }

    pub fn overlay_open(&self) -> bool {
        self.add_card_open || self.card_modal.is_some() || self.filter_bar.is_open()
    }

    pub fn handle_overlay_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        if self.add_card_open {
            let (answer, result) = self.add_card_modal.handle_event(event, window, ctx);
            match answer {
                Some(AddCardAnswer::Saved) => {
                    self.add_card_open = false;
                    ctx.toast("Carta adicionada.", Intent::Constructive);
                    self.reload();
                }
                Some(AddCardAnswer::Cancelled) => {
                    self.add_card_open = false;
                }
                None => {}
            }
            return result;
        }
        if let Some(modal) = &mut self.card_modal {
            let (answer, result) = modal.handle_event(event, window, ctx);
            if matches!(answer, Some(CardModalAnswer::Closed)) {
                self.card_modal = None;
                self.reload();
            }
            return result;
        }
        // Open filter menu floats over the grid and eats the event first.
        let toggle = self.filter_toggle_rect(self.last_content);
        let result = self
            .filter_bar
            .handle_event(event, toggle, window);
        if result.clicked || result.changed {
            return result;
        }
        EventResult::IGNORED
    }

    pub fn render_overlay(
        &mut self,
        c: &mut Compositor,
        layer: LayerId,
        window: Rect,
        theme: &Theme,
        art: &mut ArtCache,
    ) {
        if self.add_card_open {
            self.add_card_modal.render(c, layer, window, theme);
        } else if let Some(modal) = &mut self.card_modal {
            modal.render(c, layer, window, theme, art);
        } else {
            // The open filter menu floats over content, outside the scroll clip.
            let toggle = self.filter_toggle_rect(self.last_content);
            self.filter_bar.render(c, toggle, layer, theme);
        }
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        let controls_bottom = self.grid_y(content);
        if self.visible_entries().is_empty() {
            return (controls_bottom + 240.0 - content.y).max(content.h);
        }
        let rects = self.visible_tile_rects(content);
        let bottom = rects.last().map(|r| r.y + r.h).unwrap_or(controls_bottom);
        (bottom + 24.0 - content.y).max(content.h)
    }

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        content: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        self.last_content = content;
        match *event {
            WidgetEvent::MouseMove { x, y } => {
                let hover = self.hit_at(x, y, content);
                if hover != self.hover {
                    self.hover = hover;
                    EventResult::changed()
                } else {
                    EventResult::IGNORED
                }
            }
            WidgetEvent::MouseDown { x, y } => {
                // The filter toggle lives in the controls row; the menu, when
                // open, is handled by the overlay path but the toggle itself
                // must flip here.
                let toggle = self.filter_toggle_rect(content);
                if toggle.contains(x, y) {
                    return self.filter_bar.handle_event(event, toggle, content);
                }
                match self.hit_at(x, y, content) {
                    Some(Hit::AddButton) => {
                        self.add_card_open = true;
                        self.add_card_modal.open(ctx);
                        EventResult::clicked()
                    }
                    Some(Hit::StatusChip(i)) => {
                        self.select_status(i, ctx);
                        EventResult::clicked()
                    }
                    Some(Hit::Card(i)) => {
                        if let Some(entry) = self.visible_entries().get(i) {
                            let name = entry.card_name.clone();
                            self.card_modal = Some(CardModal::open(&name, ctx));
                        }
                        EventResult::clicked()
                    }
                    None => EventResult::IGNORED,
                }
            }
            _ => EventResult::IGNORED,
        }
    }

    pub fn render(
        &mut self,
        c: &mut Compositor,
        _layer: LayerId,
        content: Rect,
        theme: &Theme,
        art: &mut ArtCache,
    ) {
        self.last_content = content;
        let search_rect = self.search_rect(content);
        self.search_field.render(c, search_rect, theme);

        for (i, rect) in self.chip_rects(content).iter().enumerate() {
            self.status_chips[i].render(c, *rect, theme);
        }

        let toggle_rect = self.filter_toggle_rect(content);
        // Menu body (when open) renders in the overlay pass; the toggle is inline.
        self.filter_bar.render(c, toggle_rect, _layer, theme);

        let add_rect = self.add_btn_rect(content);
        self.add_btn.render(c, add_rect, theme);

        let visible: Vec<&CollectionEntry> = self.visible_entries();
        if visible.is_empty() {
            let empty_rect = Rect::new(content.x, self.grid_y(content), content.w, 200.0);
            if self.loading && self.entries.is_empty() {
                self.loading_empty.render(c, empty_rect, theme);
            } else if !self.entries.is_empty() {
                self.filter_empty.render(c, empty_rect, theme);
            } else {
                self.empty.render(c, empty_rect, theme);
            }
            return;
        }

        let rects = self.visible_tile_rects(content);
        for (i, rect) in rects.iter().enumerate() {
            if let Some(entry) = visible.get(i) {
                let hovered = self.hover == Some(Hit::Card(i));
                self.render_tile(c, *rect, entry, hovered, art, theme);
            }
        }
    }

    // -- Layout ---------------------------------------------------------------

    fn search_rect(&self, content: Rect) -> Rect {
        Rect::new(content.x, content.y, SEARCH_W.min(content.w), SEARCH_H)
    }

    fn chip_rects(&self, content: Rect) -> Vec<Rect> {
        let mut rects = Vec::with_capacity(self.status_chips.len());
        let search_rect = self.search_rect(content);
        let mut x = search_rect.x + search_rect.w + 16.0;
        let y = content.y + (SEARCH_H - CHIP_H) / 2.0;
        for chip in &self.status_chips {
            let (w, _) = chip.preferred_size();
            rects.push(Rect::new(x, y, w, CHIP_H));
            x += w + CHIP_GAP;
        }
        rects
    }

    fn add_btn_rect(&self, content: Rect) -> Rect {
        let (w, h) = self.add_btn.preferred_size();
        Rect::new(
            (content.x + content.w - w).max(content.x),
            content.y + (SEARCH_H - h) / 2.0,
            w,
            h,
        )
    }

    /// The filter toggle sits between the status chips and the add button.
    fn filter_toggle_rect(&self, content: Rect) -> Rect {
        let chips = self.chip_rects(content);
        let last = chips.last().copied().unwrap_or(Rect::new(
            content.x + SEARCH_W + 16.0,
            content.y,
            0.0,
            0.0,
        ));
        Rect::new(
            last.x + last.w + CHIP_GAP,
            content.y + (SEARCH_H - FILTER_TOGGLE_H) / 2.0,
            FILTER_TOGGLE_W,
            FILTER_TOGGLE_H,
        )
    }

    /// Tile rects for the *filtered* view — hover/click/render all use this,
    /// so the rect a click lands in is the rect that was drawn.
    fn visible_tile_rects(&self, content: Rect) -> Vec<Rect> {
        let (cols, col_w) = grid_columns(content.w, TILE_MIN_W, TILE_MAX_W, GRID_GAP);
        let tile_h = col_w * CARD_ASPECT + BODY_H;
        let y0 = self.grid_y(content);
        self.visible_entries()
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let (row, col) = (i / cols, i % cols);
                Rect::new(
                    content.x + col as f32 * (col_w + GRID_GAP),
                    y0 + row as f32 * (tile_h + GRID_GAP),
                    col_w,
                    tile_h,
                )
            })
            .collect()
    }

    fn grid_y(&self, content: Rect) -> f32 {
        content.y + SEARCH_H + 24.0
    }

    #[cfg(test)]
    fn tile_rects(&self, content: Rect) -> Vec<Rect> {
        self.visible_tile_rects(content)
    }

    fn hit_at(&self, x: f32, y: f32, content: Rect) -> Option<Hit> {
        if self.add_btn_rect(content).contains(x, y) {
            return Some(Hit::AddButton);
        }
        for (i, rect) in self.chip_rects(content).iter().enumerate() {
            if rect.contains(x, y) {
                return Some(Hit::StatusChip(i));
            }
        }
        for (i, rect) in self.visible_tile_rects(content).iter().enumerate() {
            if rect.contains(x, y) {
                return Some(Hit::Card(i));
            }
        }
        None
    }

    // -- Actions --------------------------------------------------------------

    fn select_status(&mut self, idx: usize, ctx: &mut ScreenCtx) {
        let status = match idx {
            1 => "allocated",
            2 => "free",
            _ => "all",
        };
        if self.status == status {
            return;
        }
        self.status = status.to_string();
        for (i, chip) in self.status_chips.iter_mut().enumerate() {
            chip.selected = i == idx;
        }
        self.loading = true;
        ctx.send(Command::ListCollection {
            status: self.status.clone(),
            q: self.search.clone(),
        });
    }

    fn reload(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Command::ListCollection {
                status: self.status.clone(),
                q: self.search.clone(),
            });
        }
    }

    // -- Tile rendering -------------------------------------------------------

    fn render_tile(
        &self,
        c: &mut Compositor,
        rect: Rect,
        entry: &CollectionEntry,
        hovered: bool,
        art: &mut ArtCache,
        theme: &Theme,
    ) {
        // Tile backing.
        c.push(rounded_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            theme.radius.lg,
            if hovered {
                theme.glass.surface_hover.0
            } else {
                theme.glass.surface.0
            },
        ));
        c.push(rounded_rect_stroke(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            theme.radius.lg,
            if hovered {
                theme.glass.edge.0
            } else {
                theme.glass.edge_soft.0
            },
            1.0,
        ));

        // Card art.
        let art_h = rect.w * CARD_ASPECT;
        let rel = entry
            .image
            .as_ref()
            .and_then(|i| i.rel.as_deref())
            .map(|r| images::with_variant(r, "normal"));
        match rel.as_deref().and_then(|r| art.get(r)) {
            Some(handle) => {
                c.push(SceneNode::Image {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: art_h,
                    image: handle,
                    corner_radius: theme.radius.md,
                });
            }
            None => {
                c.push(rounded_rect(
                    rect.x,
                    rect.y,
                    rect.w,
                    art_h,
                    theme.radius.md,
                    theme.glass.surface_active.0,
                ));
                text(
                    c,
                    &entry.card_name,
                    12.0,
                    500,
                    rect.x + 10.0,
                    rect.y + art_h / 2.0 - 8.0,
                    theme.colors.text_dim.0,
                );
            }
        }

        // Quantity badge, top-right of the art.
        let qty_label = format!("{}x", entry.total_quantity);
        let qty_w = qty_label.len() as f32 * 7.5 + 14.0;
        let badge_x = rect.x + rect.w - qty_w - 8.0;
        let badge_y = rect.y + 8.0;
        c.push(rounded_rect(
            badge_x,
            badge_y,
            qty_w,
            20.0,
            10.0,
            with_alpha(theme.colors.surface.0, 0.92),
        ));
        text(
            c,
            &qty_label,
            11.0,
            600,
            badge_x + 7.0,
            badge_y + 4.0,
            theme.colors.text.0,
        );

        // Name and allocation badges under the art.
        let mut by = rect.y + art_h + 10.0;
        text(
            c,
            &entry.card_name,
            13.0,
            600,
            rect.x + 8.0,
            by,
            theme.colors.text.0,
        );
        by += 20.0;

        let (free, allocated) = Self::deck_summary(entry);
        if free > 0 && !allocated.is_empty() {
            let label = format!("{free} livre{}", if free > 1 { "s" } else { "" });
            let bw = label.len() as f32 * 6.6 + 12.0;
            c.push(rounded_rect(
                rect.x + 8.0,
                by,
                bw,
                18.0,
                9.0,
                theme.colors.success.0,
            ));
            text(
                c,
                &label,
                10.0,
                600,
                rect.x + 14.0,
                by + 3.0,
                [0.05, 0.05, 0.05, 1.0],
            );
            by += 22.0;
        }
        if !allocated.is_empty() {
            let label: Vec<String> = allocated
                .iter()
                .map(|(name, qty)| {
                    if *qty > 1 {
                        format!("{} ({}x)", name, qty)
                    } else {
                        name.clone()
                    }
                })
                .collect();
            text(
                c,
                &label.join(" + "),
                10.0,
                500,
                rect.x + 8.0,
                by,
                theme.colors.text_dim.0,
            );
        }
    }

    fn deck_summary(entry: &CollectionEntry) -> (i64, Vec<(String, i64)>) {
        let mut free = 0;
        let mut by_deck: HashMap<String, i64> = HashMap::new();
        for copy in &entry.decks {
            if copy.deck_name == "Livre" {
                free += copy.quantity;
            } else {
                *by_deck.entry(copy.deck_name.clone()).or_insert(0) += copy.quantity;
            }
        }
        let mut allocated: Vec<_> = by_deck.into_iter().collect();
        allocated.sort_by(|a, b| a.0.cmp(&b.0));
        (free, allocated)
    }
}

/// RGBA with overridden alpha, matching the helper in `view/mod.rs`.
fn with_alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellbook_core::client::Command;

    fn test_screen() -> CollectionScreen {
        let theme = Theme::hoff();
        CollectionScreen::new(&theme)
    }

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        // Leaking the channel is fine for tests: it just gives us a stable
        // sender reference without an owning heap value.
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    #[test]
    fn new_screen_has_no_overlay_open() {
        let screen = test_screen();
        assert!(!screen.overlay_open());
    }

    #[test]
    fn default_status_is_all() {
        let screen = test_screen();
        assert_eq!(screen.status, "all");
        assert!(screen.status_chips[0].selected);
        assert!(!screen.status_chips[1].selected);
        assert!(!screen.status_chips[2].selected);
    }

    #[test]
    fn on_enter_lists_collection_with_current_status_and_search() {
        let mut screen = test_screen();
        let (mut ctx, rx) = test_ctx();
        screen.on_enter(Route::Collection, &mut ctx);
        let cmd = rx.try_recv().expect("expected a command");
        match cmd {
            Command::ListCollection { status, q } => {
                assert_eq!(status, "all");
                assert_eq!(q, "");
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn collection_listed_populates_entries() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        let entries = vec![CollectionEntry {
            card_name: "Sol Ring".to_string(),
            total_quantity: 1,
            decks: vec![],
            entry_ids: vec![1],
            first_added: None,
            last_added: None,
            type_line: None,
            mana_cost: None,
            colors: None,
            rarity: None,
            price_usd: None,
            cmc: None,
            image: None,
            image_back: None,
        }];
        let changed = screen.on_event(&Event::CollectionListed(entries), &mut ctx);
        assert!(changed);
        assert_eq!(screen.entries.len(), 1);
        assert!(!screen.loading);
    }

    #[test]
    fn select_status_updates_state_and_sends_command() {
        let mut screen = test_screen();
        let (mut ctx, rx) = test_ctx();
        screen.on_enter(Route::Collection, &mut ctx);
        let _ = rx.try_recv(); // drain first command

        screen.select_status(2, &mut ctx);
        assert_eq!(screen.status, "free");
        assert!(screen.status_chips[2].selected);
        assert!(!screen.status_chips[0].selected);

        let cmd = rx.try_recv().expect("expected a command");
        match cmd {
            Command::ListCollection { status, q } => {
                assert_eq!(status, "free");
                assert_eq!(q, "");
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }
}

//! Wishlist: cards the user wants and does not own yet.
//!
//! Port of `desktop/ui/js/views/wishlist.js`. Shares the collection's
//! shape where the payloads match: a search field, status-independent
//! chips, a tile grid with artwork and quantity badges, the add-card
//! modal for new wishes and the card modal for inspecting a group. The
//! wishlist-only actions (acquire into the collection, remove one copy)
//! are per-group buttons on the tile.

use std::sync::mpsc::Sender;

use engine::compositor::{Compositor, LayerId, SceneNode};
use engine::theme::{Intent, Theme};
use engine::ui::{icons};
use engine::ui::widgets::{
    Button, EmptyState, EventResult, Rect, WidgetEvent, rounded_rect, rounded_rect_stroke,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::images;
use spellbook_core::ops::wishlist::WishlistGroup;

use super::{EditKey, Route, ScreenCtx, grid_columns, text};
use crate::art::ArtCache;
use crate::view::components::add_card::{AddCardAnswer, AddCardModal};
use crate::view::components::card_modal::{CardModal, CardModalAnswer};
use crate::view::components::filters::{FilterBar, matches_filters};
use crate::view::components::search_field::SearchField;

const SEARCH_H: f32 = 52.0; // LabeledField::height()
const SEARCH_W: f32 = 260.0;
const FILTER_TOGGLE_W: f32 = 40.0;
const FILTER_TOGGLE_H: f32 = 36.0;
const CHIP_GAP: f32 = 8.0;
const GRID_GAP: f32 = 16.0;
const TILE_MIN_W: f32 = 160.0;
const TILE_MAX_W: f32 = 240.0;
/// MTG normal card aspect: 488 × 680.
const CARD_ASPECT: f32 = 680.0 / 488.0;
/// Space under the art for name, price and action buttons.
const BODY_H: f32 = 92.0;
const SEARCH_DEBOUNCE: f32 = 0.250;

/// What the pointer is over. Layout functions below provide the rects, so
/// hover/click never disagree with the pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    AddButton,
    Card(usize),
    Acquire(usize),
    Remove(usize),
}

pub struct WishlistScreen {
    /// (card name, entry id) pending the "tirar da wishlist?" confirm.
    drop_confirm: Option<(String, i64)>,
    wishes: Vec<WishlistGroup>,
    search: String,
    search_field: SearchField,
    filter_bar: FilterBar,
    /// Filtered view of `wishes` — hover/click/render all index into this.
    visible: Vec<usize>,
    add_btn: Button,
    add_card_modal: AddCardModal,
    add_card_open: bool,
    card_modal: Option<CardModal>,
    /// Which group the card modal was opened from, so acquire/remove hit
    /// the right `WishlistEntry` when the action lands.
    modal_group: Option<usize>,
    hover: Option<Hit>,
    debounce: f32,
    search_dirty: bool,
    loading: bool,
    /// Group index whose acquire is in flight (one unit at a time).
    acquiring: Option<usize>,
    /// Group index whose remove is in flight.
    removing: Option<usize>,
    /// Content rect of the last layout pass, for the filter menu geometry.
    last_content: Rect,
    empty: EmptyState,
    loading_empty: EmptyState,
    filter_empty: EmptyState,
    /// Command sender captured on enter so reloads work from anywhere.
    tx: Option<Sender<Command>>,
}

impl WishlistScreen {
    pub fn new(theme: &Theme) -> Self {
        let mut search_field =
            SearchField::new_without_callback("Buscar carta (PT ou EN)…", theme);
        search_field.focus();
        Self {
            drop_confirm: None,
            wishes: Vec::new(),
            search: String::new(),
            search_field,
            filter_bar: FilterBar::new(theme, true),
            visible: Vec::new(),
            add_btn: Button::new("+ Adicionar carta"),
            add_card_modal: AddCardModal::new(theme),
            add_card_open: false,
            card_modal: None,
            modal_group: None,
            hover: None,
            debounce: 0.0,
            search_dirty: false,
            loading: true,
            acquiring: None,
            removing: None,
            last_content: Rect::new(0.0, 0.0, 800.0, 600.0),
            empty: EmptyState::new(
                "Wishlist vazia",
                "Adicione cartas que você quer adquirir.",
            )
            .icon("heart"),
            loading_empty: EmptyState::new(
                "Carregando wishlist…",
                "Lendo seus desejos do banco local.",
            )
            .icon("heart"),
            filter_empty: EmptyState::new(
                "Nada corresponde aos filtros",
                "Ajuste ou limpe os filtros para ver mais desejos.",
            )
            .icon("heart"),
            tx: None,
        }
    }

    pub fn on_enter(&mut self, _route: Route, ctx: &mut ScreenCtx) {
        self.loading = true;
        self.wishes.clear();
        self.tx = Some(ctx.tx.clone());
        ctx.send(Command::ListWishlist {
            q: self.search.clone(),
        });
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = false;
        if let Some(modal) = &mut self.card_modal {
            changed |= modal.on_event(event, ctx);
        }
        changed |= self.add_card_modal.on_event(event, ctx);

        match event {
            Event::WishlistListed(list) => {
                self.wishes = list.clone();
                self.loading = false;
                self.recompute_visible();
                return true;
            }
            Event::WishlistAcquired(result) => {
                if self.acquiring.take().is_some() {
                    match result {
                        Ok(_) => {
                            ctx.toast(
                                "Carta movida para a coleção.",
                                Intent::Constructive,
                            );
                            // The JS reloaded the wishlist right after
                            // acquiring; do the same so counts update.
                            if let Some(tx) = &self.tx {
                                let _ = tx.send(Command::ListWishlist {
                                    q: self.search.clone(),
                                });
                            }
                        }
                        Err(e) => ctx.toast(e.detail().to_string(), Intent::Destructive),
                    }
                    return true;
                }
            }
            Event::WishlistDeleted(result) => {
                if let Some(idx) = self.removing.take() {
                    match result {
                        Ok(removal) => {
                            if removal.remaining == 0 && idx < self.wishes.len() {
                                self.wishes.remove(idx);
                                self.recompute_visible();
                            } else if let Some(group) = self.wishes.get_mut(idx) {
                                group.total_quantity = removal.remaining;
                                // One unit left the first stored entry.
                                if let Some(first) = group.entries.first_mut() {
                                    first.quantity = (first.quantity - 1).max(0);
                                }
                                self.recompute_visible();
                            }
                        }
                        Err(e) => ctx.toast(e.detail().to_string(), Intent::Destructive),
                    }
                    return true;
                }
            }
            _ => {}
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
        if self.drop_confirm.take().is_some() {
            return true;
        }
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
                ctx.send(Command::ListWishlist {
                    q: self.search.clone(),
                });
            }
        }
        animating
    }

    pub fn overlay_open(&self) -> bool {
        self.add_card_open
            || self.card_modal.is_some()
            || self.filter_bar.is_open()
            || self.drop_confirm.is_some()
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
                    ctx.toast("Carta adicionada à wishlist.", Intent::Constructive);
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
                self.modal_group = None;
                self.reload();
            }
            return result;
        }
        // The drop-confirm dialog owns the window while open.
        if let Some((name, id)) = self.drop_confirm.clone() {
            if let WidgetEvent::MouseDown { x, y } = *event {
                let (yes, no) = crate::view::deck_detail::pub_confirm_buttons(window);
                if yes.contains(x, y) {
                    self.drop_confirm = None;
                    if !self.wishlist_busy()
                        && let Some(tx) = &self.tx
                    {
                        self.removing = Some(self.wishes.iter().position(|g| g.card_name == name).unwrap_or(gi_index_of(&self.wishes, id)));
                        let _ = tx.send(Command::DeleteWishlist { entry_id: id });
                    }
                    return EventResult::clicked();
                }
                if no.contains(x, y) || !window.contains(x, y) {
                    self.drop_confirm = None;
                    return EventResult::changed();
                }
            }
            return EventResult::IGNORED;
        }
        // Open filter menu floats over the grid and eats the event first.
        let toggle = self.filter_toggle_rect(self.last_content);
        let result = self.filter_bar.handle_event(event, toggle, window);
        if result.clicked {
            self.recompute_visible();
        }
        result
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
        } else if let Some((name, _)) = self.drop_confirm.clone() {
            crate::view::components::confirm::render_confirm_dialog(
                c,
                layer,
                window,
                theme,
                "Tirar da wishlist?",
                &format!("{name} sai da sua lista de compras."),
                ("Remover", true),
            );
        } else {
            // The open filter menu floats over content, outside the scroll clip.
            let toggle = self.filter_toggle_rect(self.last_content);
            let _ = art;
            self.filter_bar.render(c, toggle, layer, theme);
        }
    }

    // -- Layout ---------------------------------------------------------------

    fn search_rect(&self, content: Rect) -> Rect {
        Rect::new(content.x, content.y, SEARCH_W.min(content.w), SEARCH_H)
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

    fn grid_y(&self, content: Rect) -> f32 {
        content.y + SEARCH_H + 24.0
    }

    fn tile_rects(&self, content: Rect) -> Vec<Rect> {
        let (cols, col_w) = grid_columns(content.w, TILE_MIN_W, TILE_MAX_W, GRID_GAP);
        let tile_h = col_w * CARD_ASPECT + BODY_H;
        let y0 = self.grid_y(content);
        self.visible
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

    /// The filter toggle sits between the search field and the add button.
    fn filter_toggle_rect(&self, content: Rect) -> Rect {
        let search = self.search_rect(content);
        Rect::new(
            search.x + search.w + CHIP_GAP,
            content.y + (SEARCH_H - FILTER_TOGGLE_H) / 2.0,
            FILTER_TOGGLE_W,
            FILTER_TOGGLE_H,
        )
    }

    fn hit_at(&self, x: f32, y: f32, content: Rect) -> Option<Hit> {
        if self.add_btn_rect(content).contains(x, y) {
            return Some(Hit::AddButton);
        }
        for (i, rect) in self.tile_rects(content).iter().enumerate() {
            if !rect.contains(x, y) {
                continue;
            }
            // Buttons sit at the bottom of the body; check them first.
            let (acq, rem) = self.action_rects(*rect);
            if acq.contains(x, y) {
                return Some(Hit::Acquire(i));
            }
            if rem.contains(x, y) {
                return Some(Hit::Remove(i));
            }
            return Some(Hit::Card(i));
        }
        None
    }

    /// The two action buttons on a tile body, in layout order.
    fn action_rects(&self, rect: Rect) -> (Rect, Rect) {
        let bw = (rect.w - 24.0) / 2.0;
        let by = rect.y + rect.h - 36.0;
        (
            Rect::new(rect.x + 8.0, by, bw, 28.0),
            Rect::new(rect.x + 16.0 + bw, by, bw, 28.0),
        )
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        let controls_bottom = self.grid_y(content);
        if self.visible.is_empty() {
            return (controls_bottom + 240.0 - content.y).max(content.h);
        }
        let rects = self.tile_rects(content);
        let bottom = rects.last().map(|r| r.y + r.h).unwrap_or(controls_bottom);
        (bottom + 24.0 - content.y).max(content.h)
    }

    // -- Events ---------------------------------------------------------------

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
                let toggle = self.filter_toggle_rect(content);
                if toggle.contains(x, y) {
                    let r = self.filter_bar.handle_event(event, toggle, content);
                    self.recompute_visible();
                    return r;
                }
                match self.hit_at(x, y, content) {
                    Some(Hit::AddButton) => {
                        self.add_card_open = true;
                        self.add_card_modal.open(ctx);
                        EventResult::clicked()
                    }
                    Some(Hit::Card(i)) => {
                        if let Some(&gi) = self.visible.get(i) {
                            let name = self.wishes[gi].card_name.clone();
                            self.modal_group = Some(gi);
                            self.card_modal = Some(CardModal::open(&name, ctx));
                        }
                        EventResult::clicked()
                    }
                    Some(Hit::Acquire(i)) => {
                        if let Some(&gi) = self.visible.get(i)
                            && let Some(group) = self.wishes.get(gi)
                            && let Some(entry) = group.entries.first()
                            && !self.wishlist_busy()
                        {
                            self.acquiring = Some(gi);
                            ctx.send(Command::AcquireWishlist {
                                entry_id: entry.id,
                            });
                        }
                        EventResult::clicked()
                    }
                    Some(Hit::Remove(i)) => {
                        // confirm.js: "Tirar da wishlist?" antes de remover.
                        if !self.wishlist_busy()
                            && let Some(&gi) = self.visible.get(i)
                            && let Some(group) = self.wishes.get(gi)
                            && let Some(entry) = group.entries.first()
                        {
                            self.drop_confirm = Some((group.card_name.clone(), entry.id));
                        }
                        EventResult::clicked()
                    }
                    None => EventResult::IGNORED,
                }
            }
            _ => EventResult::IGNORED,
        }
    }

    /// Filtered view indices — the chip filters run client-side over the
    /// loaded list, exactly as the JS wishlist did.
    fn recompute_visible(&mut self) {
        self.visible = self
            .wishes
            .iter()
            .enumerate()
            .filter(|(_, g)| matches_filters(*g, &self.filter_bar.state))
            .map(|(i, _)| i)
            .collect();
    }

    fn wishlist_busy(&self) -> bool {
        self.acquiring.is_some() || self.removing.is_some()
    }

    fn reload(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Command::ListWishlist {
                q: self.search.clone(),
            });
        }
    }

    // -- Render ---------------------------------------------------------------

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

        let toggle_rect = self.filter_toggle_rect(content);
        self.filter_bar.render(c, toggle_rect, _layer, theme);

        let add_rect = self.add_btn_rect(content);
        self.add_btn.render(c, add_rect, theme);

        // Summary line: total copies · names · price, the JS's `wl-summary`.
        if !self.wishes.is_empty() {
            let units: i64 = self.wishes.iter().map(|g| g.total_quantity).sum();
            let usd: f64 = self
                .wishes
                .iter()
                .filter_map(|g| g.price_usd.as_deref()?.parse::<f64>().ok().map(|p| p * g.total_quantity as f64))
                .sum();
            let summary = format!(
                "{} carta(s) · {} nome(s) · ~${:.2}",
                units,
                self.wishes.len(),
                usd
            );
            text(
                c,
                &summary,
                12.0,
                500,
                add_rect.x - summary.len() as f32 * 6.4 - 16.0,
                content.y + (SEARCH_H - 12.0) / 2.0,
                theme.colors.text_dim.0,
            );
        }

        if self.visible.is_empty() {
            let empty_rect = Rect::new(content.x, self.grid_y(content), content.w, 200.0);
            if self.loading && self.wishes.is_empty() {
                self.loading_empty.render(c, empty_rect, theme);
            } else if !self.wishes.is_empty() {
                self.filter_empty.render(c, empty_rect, theme);
            } else {
                self.empty.render(c, empty_rect, theme);
            }
            return;
        }

        let rects = self.tile_rects(content);
        for (i, rect) in rects.iter().enumerate() {
            if let Some(&gi) = self.visible.get(i)
                && let Some(group) = self.wishes.get(gi)
            {
                let hovered = matches!(
                    self.hover,
                    Some(Hit::Card(j)) | Some(Hit::Acquire(j)) | Some(Hit::Remove(j))
                    if j == i
                );
                self.render_tile(c, *rect, group, hovered, art, theme);
            }
        }
    }

    fn render_tile(
        &self,
        c: &mut Compositor,
        rect: Rect,
        group: &WishlistGroup,
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
        let rel = group
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
                    &group.card_name,
                    12.0,
                    500,
                    rect.x + 10.0,
                    rect.y + art_h / 2.0 - 8.0,
                    theme.colors.text_dim.0,
                );
            }
        }

        // Quantity badge, top-right of the art.
        let qty_label = format!("{}x", group.total_quantity);
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

        // Price badge, top-left, when the index knows one.
        if let Some(price) = group.price_usd.as_deref().filter(|p| !p.is_empty()) {
            let label = format!("US$ {price}");
            let pw = label.len() as f32 * 6.4 + 12.0;
            c.push(rounded_rect(
                rect.x + 8.0,
                badge_y,
                pw,
                20.0,
                10.0,
                with_alpha(theme.colors.surface.0, 0.92),
            ));
            text(
                c,
                &label,
                11.0,
                600,
                rect.x + 14.0,
                badge_y + 4.0,
                theme.colors.success.0,
            );
        }

        // Name and meta under the art.
        let mut by = rect.y + art_h + 10.0;
        text(c, &group.card_name, 13.0, 600, rect.x + 8.0, by, theme.colors.text.0);
        by += 20.0;
        if let Some(type_line) = group.type_line.as_deref().filter(|t| !t.is_empty()) {
            text(
                c,
                type_line,
                10.0,
                400,
                rect.x + 8.0,
                by,
                theme.colors.text_dim.0,
            );
        }

        // Actions: "Comprei" (moves to collection) and the trash remove, the
        // JS tile's two buttons.
        let (acq, rem) = self.action_rects(rect);
        c.push(rounded_rect(
            acq.x,
            acq.y,
            acq.w,
            acq.h,
            theme.radius.md,
            theme.colors.success.0,
        ));
        text(
            c,
            "Comprei",
            11.0,
            600,
            acq.x + 8.0,
            acq.y + 7.0,
            [0.05, 0.05, 0.05, 1.0],
        );
        c.push(rounded_rect(
            rem.x,
            rem.y,
            rem.w,
            rem.h,
            theme.radius.md,
            theme.glass.surface_active.0,
        ));
        c.push(rounded_rect_stroke(
            rem.x,
            rem.y,
            rem.w,
            rem.h,
            theme.radius.md,
            theme.glass.edge.0,
            1.0,
        ));
        if let Some(node) = icons::icon_at("trash-2", 14.0, theme.colors.danger.0, rem.x + 10.0, rem.y + 7.0) {
            c.push(node);
        }
        text(
            c,
            "Remover",
            11.0,
            600,
            rem.x + 30.0,
            rem.y + 7.0,
            theme.colors.danger.0,
        );
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

    fn test_screen() -> WishlistScreen {
        let theme = Theme::hoff();
        WishlistScreen::new(&theme)
    }

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    fn sample_group() -> WishlistGroup {
        WishlistGroup {
            card_name: "Sol Ring".into(),
            total_quantity: 2,
            entries: vec![spellbook_core::ops::wishlist::WishlistEntry {
                id: 7,
                quantity: 2,
                lang: "en".into(),
                set_code: None,
                artist: None,
                notes: None,
            }],
            ..WishlistGroup::default()
        }
    }

    #[test]
    fn new_screen_has_no_overlay_open() {
        let screen = test_screen();
        assert!(!screen.overlay_open());
    }

    #[test]
    fn on_enter_lists_wishlist() {
        let mut screen = test_screen();
        let (mut ctx, rx) = test_ctx();
        screen.on_enter(Route::Wishlist, &mut ctx);
        let cmd = rx.try_recv().expect("expected a command");
        match cmd {
            Command::ListWishlist { q } => assert_eq!(q, ""),
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn wishlist_listed_populates_grid() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        let changed = screen.on_event(&Event::WishlistListed(vec![sample_group()]), &mut ctx);
        assert!(changed);
        assert_eq!(screen.wishes.len(), 1);
        assert!(!screen.loading);
    }

    #[test]
    fn acquire_sends_entry_id_of_first_stored_row() {
        let mut screen = test_screen();
        let (mut ctx, rx) = test_ctx();
        screen.wishes = vec![sample_group()];
        screen.recompute_visible();
        let content = Rect::new(300.0, 120.0, 800.0, 600.0);
        let rects = screen.tile_rects(content);
        let tile = rects[0];
        let (acq, _) = screen.action_rects(tile);
        let result = screen.handle_event(
            &WidgetEvent::MouseDown {
                x: acq.x + 5.0,
                y: acq.y + 5.0,
            },
            content,
            &mut ctx,
        );
        assert!(result.clicked);
        let cmd = rx.try_recv().expect("expected AcquireWishlist");
        match cmd {
            Command::AcquireWishlist { entry_id } => assert_eq!(entry_id, 7),
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn remove_sends_delete_and_swallows_group() {
        let mut screen = test_screen();
        let (mut ctx, rx) = test_ctx();
        screen.wishes = vec![sample_group()];
        screen.recompute_visible();
        screen.on_event(&Event::WishlistDeleted(Ok(
            spellbook_core::ops::wishlist::WishlistRemoval {
                card_name: "Sol Ring".into(),
                remaining: 0,
            },
        )), &mut ctx);
        // The optimistic path needs the index set by the click handler; drive
        // one through it directly.
        screen.removing = Some(0);
        screen.on_event(&Event::WishlistDeleted(Ok(
            spellbook_core::ops::wishlist::WishlistRemoval {
                card_name: "Sol Ring".into(),
                remaining: 0,
            },
        )), &mut ctx);
        assert!(screen.wishes.is_empty());
        let _ = rx;
    }

    #[test]
    fn add_button_opens_modal() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        let content = Rect::new(300.0, 120.0, 800.0, 600.0);
        let rect = screen.add_btn_rect(content);
        let result = screen.handle_event(
            &WidgetEvent::MouseDown {
                x: rect.x + 5.0,
                y: rect.y + 5.0,
            },
            content,
            &mut ctx,
        );
        assert!(result.clicked);
        assert!(screen.add_card_open);
        assert!(screen.overlay_open());
    }

    #[test]
    fn clicking_card_opens_card_modal() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        screen.wishes = vec![sample_group()];
        screen.recompute_visible();
        let content = Rect::new(300.0, 120.0, 800.0, 600.0);
        let tile = screen.tile_rects(content)[0];
        // Upper half of the tile: art, not the action buttons.
        let result = screen.handle_event(
            &WidgetEvent::MouseDown {
                x: tile.x + 10.0,
                y: tile.y + 10.0,
            },
            content,
            &mut ctx,
        );
        assert!(result.clicked);
        assert!(screen.card_modal.is_some());
        assert_eq!(screen.modal_group, Some(0));
    }
}
/// The wishlist index that holds the entry with the given id (fallback 0).
fn gi_index_of(wishes: &[WishlistGroup], entry_id: i64) -> usize {
    wishes
        .iter()
        .position(|g| g.entries.iter().any(|e| e.id == entry_id))
        .unwrap_or(0)
}

//! Dashboard: collection totals, the deck grid and the activity history.
//!
//! This is the reference screen for the app's data pattern:
//!
//! - `on_enter` queues one command (`LoadHome`); the screen draws its empty
//!   state until the answer arrives.
//! - `on_event` picks out `HomeLoaded`, stores the payload and reports
//!   `changed` so the shell invalidates. Errors become a toast, never a
//!   broken screen.
//! - Hit-testing runs against the same pure layout functions `render` uses,
//!   so the rect a click lands in is the rect that was drawn - there is no
//!   second, drifting copy of the geometry.

use engine::compositor::{Compositor, LayerId};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{
    Button, EmptyState, EventResult, Rect, WidgetEvent, rounded_rect,
};
use spellbook_core::client::{Command, Event, HomeData};

use crate::view::components::add_card::{AddCardAnswer, AddCardModal};
use crate::view::components::new_deck::{NewDeckAnswer, NewDeckModal};

use super::{EditKey, Route, ScreenCtx, deck_tile, grid_columns, group_label, panel, text};
use crate::art::ArtCache;

const STAT_H: f32 = 92.0;
const STAT_GAP: f32 = 12.0;
const DECK_GAP: f32 = 16.0;
const ACTIVITY_ROW_H: f32 = 34.0;

/// What the pointer is over. Rects come from the layout functions below, so
/// hover and click never disagree with the pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    /// The five stat cards, in layout order. Each navigates somewhere.
    Stat(usize),
    Deck(i64),
    NewDeckTile,
    AddCardBtn,
    NewDeckBtn,
}

pub struct HomeScreen {
    data: Option<Box<HomeData>>,
    hover: Option<Hit>,
    loading: EmptyState,
    /// Header buttons (the `page-header` btn row of home.js).
    add_btn: Button,
    new_deck_btn: Button,
    add_card_modal: AddCardModal,
    add_card_open: bool,
    new_deck_modal: NewDeckModal,
}

impl HomeScreen {
    pub fn new() -> Self {
        Self {
            data: None,
            hover: None,
            loading: EmptyState::new(
                "Carregando o laboratório",
                "Lendo a coleção, os decks e o histórico do banco local.",
            )
            .icon("house"),
            add_btn: Button::new("+ Adicionar Carta"),
            new_deck_btn: Button::new("Novo Deck").variant(engine::ui::widgets::ButtonVariant::Outline),
            add_card_modal: AddCardModal::new(&Theme::hoff()),
            add_card_open: false,
            new_deck_modal: NewDeckModal::new(&Theme::hoff()),
        }
    }

    pub fn on_enter(&mut self, _route: Route, ctx: &mut ScreenCtx) {
        ctx.send(Command::LoadHome);
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        // Modais primeiro: DeckAdded/CardsFound etc. pertencem a eles.
        let mut changed = self.on_modal_event(event, ctx);

        let Event::HomeLoaded(result) = event else {
            return changed;
        };

        match result {
            Ok(data) => {
                // The shell re-enters the route on navigation, which reloads;
                // here we only store what arrived.
                self.data = Some(data.clone());
                changed = true;
            }
            Err(e) => {
                ctx.toast(e.detail().to_string(), Intent::Destructive);
                changed = true;
            }
        }
        changed
    }

    pub fn handle_text(&mut self, s: &str, _ctx: &mut ScreenCtx) -> bool {
        if self.add_card_open {
            return self.add_card_modal.handle_text(s);
        }
        if self.new_deck_modal.is_open() {
            return self.new_deck_modal.handle_text(s);
        }
        false
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> bool {
        if self.add_card_open {
            return self.add_card_modal.handle_edit_key(key, ctx).changed;
        }
        if self.new_deck_modal.is_open() {
            return self.new_deck_modal.handle_edit_key(key, ctx).changed;
        }
        false
    }

    pub fn handle_escape(&mut self) -> bool {
        if self.add_card_open {
            if self.add_card_modal.handle_escape() {
                return true;
            }
            self.add_card_open = false;
            return true;
        }
        if self.new_deck_modal.is_open() {
            if self.new_deck_modal.handle_escape() {
                return true;
            }
            self.new_deck_modal.close();
            return true;
        }
        false
    }

    pub fn tick(&mut self, _dt: f32, ctx: &mut ScreenCtx) -> bool {
        if self.add_card_open {
            return self.add_card_modal.tick(_dt, ctx);
        }
        if self.new_deck_modal.is_open() {
            return self.new_deck_modal.tick(_dt, ctx);
        }
        false
    }

    // -- Layout ---------------------------------------------------------------

    /// The five stat cards across the top, navigable like the old
    /// `data-stat-nav` tiles. Starts below the header buttons.
    fn stat_rects(&self, content: Rect) -> Vec<Rect> {
        let (cols, col_w) = grid_columns(content.w, 170.0, 280.0, STAT_GAP);
        let y0 = content.y + 44.0 + 12.0; // header buttons + gap
        (0..5)
            .map(|i| {
                let (row, col) = (i / cols, i % cols);
                Rect::new(
                    content.x + col as f32 * (col_w + STAT_GAP),
                    y0 + row as f32 * (STAT_H + STAT_GAP),
                    col_w,
                    STAT_H,
                )
            })
            .collect()
    }

    fn stat_rows(&self, content: Rect) -> usize {
        let (cols, _) = grid_columns(content.w, 170.0, 280.0, STAT_GAP);
        5usize.div_ceil(cols)
    }

    /// Y where the deck section starts.
    fn decks_y(&self, content: Rect) -> f32 {
        content.y + 44.0 + 12.0 + self.stat_rows(content) as f32 * (STAT_H + STAT_GAP) + 40.0
    }

    /// One rect per deck tile, plus the "Novo Deck" tile at the end.
    fn deck_rects(&self, content: Rect) -> Vec<(Hit, Rect)> {
        let Some(data) = &self.data else {
            return Vec::new();
        };
        let (cols, col_w) = grid_columns(content.w, 240.0, 340.0, DECK_GAP);
        let tile_h = deck_tile::tile_height(col_w);
        let y0 = self.decks_y(content) + 26.0;
        let mut out: Vec<(Hit, Rect)> = data
            .decks
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let (row, col) = (i / cols, i % cols);
                (
                    Hit::Deck(d.id),
                    Rect::new(
                        content.x + col as f32 * (col_w + DECK_GAP),
                        y0 + row as f32 * (tile_h + DECK_GAP),
                        col_w,
                        tile_h,
                    ),
                )
            })
            .collect();
        let i = data.decks.len();
        let (row, col) = (i / cols, i % cols);
        out.push((
            Hit::NewDeckTile,
            Rect::new(
                content.x + col as f32 * (col_w + DECK_GAP),
                y0 + row as f32 * (tile_h + DECK_GAP),
                col_w,
                tile_h,
            ),
        ));
        out
    }

    fn activity_y(&self, content: Rect) -> f32 {
        let Some(data) = &self.data else {
            return self.decks_y(content) + 26.0;
        };
        let (cols, col_w) = grid_columns(content.w, 240.0, 340.0, DECK_GAP);
        let tile_h = deck_tile::tile_height(col_w);
        let tiles = data.decks.len() + 1;
        let rows = tiles.div_ceil(cols);
        self.decks_y(content) + 26.0 + rows as f32 * (tile_h + DECK_GAP) + 16.0
    }

    fn activity_rects(&self, content: Rect) -> Vec<Rect> {
        let Some(data) = &self.data else {
            return Vec::new();
        };
        let y0 = self.activity_y(content) + 26.0;
        data.activity
            .iter()
            .enumerate()
            .map(|(i, _)| {
                Rect::new(
                    content.x,
                    y0 + i as f32 * (ACTIVITY_ROW_H + 4.0),
                    content.w,
                    ACTIVITY_ROW_H,
                )
            })
            .collect()
    }

    /// Whether a modal or menu is open over the page. While `true` the shell
    /// routes pointer events here first via `handle_overlay_event`.
    pub fn overlay_open(&self) -> bool {
        self.add_card_open || self.new_deck_modal.is_open()
    }

    /// Pointer event while an overlay is open.
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
                    ctx.toast("Carta adicionada à coleção.", Intent::Constructive);
                    ctx.send(Command::LoadHome);
                }
                Some(AddCardAnswer::Cancelled) => self.add_card_open = false,
                None => {}
            }
            return result;
        }
        if self.new_deck_modal.is_open() {
            let (answer, result) = self.new_deck_modal.handle_event(event, window, ctx);
            match answer {
                Some(NewDeckAnswer::Created(id)) => {
                    self.new_deck_modal.close();
                    ctx.navigate(Route::Deck(id));
                    ctx.send(Command::LoadHome);
                }
                Some(NewDeckAnswer::Cancelled) => self.new_deck_modal.close(),
                None => {}
            }
            return result;
        }
        EventResult::IGNORED
    }

    /// Modals and menus, drawn over the whole window after the content clip.
    pub fn render_overlay(
        &mut self,
        c: &mut Compositor,
        layer: LayerId,
        window: Rect,
        theme: &Theme,
        _art: &mut ArtCache,
    ) {
        if self.add_card_open {
            self.add_card_modal.render(c, layer, window, theme);
        } else if self.new_deck_modal.is_open() {
            self.new_deck_modal.render(c, layer, window, theme);
        }
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        let Some(data) = &self.data else {
            return content.h;
        };
        let bottom = self.activity_y(content)
            + 26.0
            + data.activity.len().max(1) as f32 * (ACTIVITY_ROW_H + 4.0);
        (bottom - content.y).max(content.h)
    }

    /// The two header buttons, top-right (the page-header btn row).
    /// Positioned inside the content rect, above the stats, so they scroll
    /// with the page and are not clipped by the header band.
    fn header_btn_rects(&self, content: Rect) -> (Rect, Rect) {
        let (aw, ah) = self.add_btn.preferred_size();
        let (nw, nh) = self.new_deck_btn.preferred_size();
        let y = content.y;
        let add = Rect::new(content.x + content.w - aw - nw - 10.0, y, aw, ah);
        let new_deck = Rect::new(content.x + content.w - nw, y, nw, nh);
        (add, new_deck)
    }

    fn hit_at(&self, x: f32, y: f32, content: Rect) -> Option<Hit> {
        let (add, new_deck) = self.header_btn_rects(content);
        if add.contains(x, y) {
            return Some(Hit::AddCardBtn);
        }
        if new_deck.contains(x, y) {
            return Some(Hit::NewDeckBtn);
        }
        for (i, rect) in self.stat_rects(content).iter().enumerate() {
            if rect.contains(x, y) {
                return Some(Hit::Stat(i));
            }
        }
        for (hit, rect) in self.deck_rects(content) {
            if rect.contains(x, y) {
                return Some(hit);
            }
        }
        None
    }

    // -- Events ---------------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        content: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        if self.data.is_none() {
            return self.loading.handle_event(event, content);
        }
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
            WidgetEvent::MouseDown { x, y } => match self.hit_at(x, y, content) {
                Some(Hit::Stat(i)) => {
                    // The old data-stat-nav mapping: decks, collection,
                    // collection, wishlist, games.
                    let route = [
                        Route::Decks,
                        Route::Collection,
                        Route::Collection,
                        Route::Wishlist,
                        Route::Games,
                    ][i];
                    ctx.navigate(route);
                    EventResult::clicked()
                }
                Some(Hit::Deck(id)) => {
                    ctx.navigate(Route::Deck(id));
                    EventResult::clicked()
                }
                Some(Hit::NewDeckTile) => {
                    self.new_deck_modal.open(ctx);
                    EventResult::clicked()
                }
                Some(Hit::AddCardBtn) => {
                    self.add_card_open = true;
                    self.add_card_modal.open(ctx);
                    EventResult::clicked()
                }
                Some(Hit::NewDeckBtn) => {
                    self.new_deck_modal.open(ctx);
                    EventResult::clicked()
                }
                None => EventResult::IGNORED,
            },
            _ => EventResult::IGNORED,
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
        let Some(data) = &self.data else {
            self.loading.render(c, content, theme);
            return;
        };

        // Header buttons (home.js's page-header btn row).
        let (add_rect, new_deck_rect) = self.header_btn_rects(content);
        self.add_btn.render(c, add_rect, theme);
        self.new_deck_btn.render(c, new_deck_rect, theme);

        // Stat cards.
        let total_cards: i64 = data.decks.iter().map(|d| d.total_cards).sum();
        let total_games: i64 = data.decks.iter().map(|d| d.wins + d.losses).sum();
        let total_wins: i64 = data.decks.iter().map(|d| d.wins).sum();
        let stats: [(String, String, String); 5] = [
            (
                "DECKS MONTADOS".into(),
                format!("{}", data.decks.len()),
                format!("{total_cards} cartas ao todo"),
            ),
            (
                "CARTAS NA COLEÇÃO".into(),
                format!("{}", data.totals.total_units),
                format!(
                    "{} nomes distintos, com repetidas",
                    data.totals.distinct_cards
                ),
            ),
            (
                "CARTAS LIVRES".into(),
                format!("{}", data.totals.free_units),
                format!("{} em decks", data.totals.allocated_units),
            ),
            (
                "WISHLIST".into(),
                format!("{}", data.wishlist.total_units),
                format!(
                    "{} nomes · ~${:.2}",
                    data.wishlist.distinct_cards, data.wishlist.price_usd
                ),
            ),
            (
                "PARTIDAS REGISTRADAS".into(),
                format!("{total_games}"),
                format!("{total_wins} vitórias"),
            ),
        ];
        for (i, rect) in self.stat_rects(content).iter().enumerate() {
            let hovered = self.hover == Some(Hit::Stat(i));
            panel(c, *rect, theme);
            if hovered {
                c.push(rounded_rect(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    theme.radius.lg,
                    theme.glass.surface_hover.0,
                ));
            }
            let (label, value, sub) = &stats[i];
            text(
                c,
                label,
                10.0,
                600,
                rect.x + 14.0,
                rect.y + 14.0,
                theme.glass.text_placeholder.0,
            );
            text(
                c,
                value,
                26.0,
                600,
                rect.x + 14.0,
                rect.y + 30.0,
                theme.colors.text.0,
            );
            text(
                c,
                sub,
                11.0,
                400,
                rect.x + 14.0,
                rect.y + 68.0,
                theme.colors.text_dim.0,
            );
        }

        // Decks.
        group_label(c, "SEUS DECKS", content.x, self.decks_y(content), theme);
        for (hit, rect) in self.deck_rects(content) {
            match hit {
                Hit::Deck(id) => {
                    let deck = data
                        .decks
                        .iter()
                        .find(|d| d.id == id)
                        .expect("layout match");
                    deck_tile::render(c, rect, deck, self.hover == Some(hit), art, theme);
                }
                Hit::NewDeckTile => {
                    let hovered = self.hover == Some(Hit::NewDeckTile);
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
                    text(
                        c,
                        "+",
                        28.0,
                        300,
                        rect.x + rect.w / 2.0 - 8.0,
                        rect.y + rect.h / 2.0 - 34.0,
                        theme.colors.text_dim.0,
                    );
                    text(
                        c,
                        "Novo Deck",
                        13.0,
                        600,
                        rect.x + rect.w / 2.0 - 30.0,
                        rect.y + rect.h / 2.0 + 2.0,
                        theme.colors.text_dim.0,
                    );
                }
                Hit::Stat(_) => unreachable!("deck layout never yields stat hits"),
                Hit::AddCardBtn | Hit::NewDeckBtn => unreachable!("header buttons are not tiles"),
            }
        }

        // Activity.
        group_label(
            c,
            "HISTÓRICO DE ATIVIDADES",
            content.x,
            self.activity_y(content),
            theme,
        );
        let rows = self.activity_rects(content);
        if rows.is_empty() {
            text(
                c,
                "Nenhuma atividade ainda - mexa num deck ou na coleção pra ver o histórico aqui.",
                12.0,
                400,
                content.x,
                self.activity_y(content) + 26.0,
                theme.colors.text_dim.0,
            );
        }
        for (rect, row) in rows.iter().zip(data.activity.iter()) {
            c.push(rounded_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                theme.radius.md,
                theme.glass.surface.0,
            ));
            text(
                c,
                &row.description,
                12.0,
                500,
                rect.x + 12.0,
                rect.y + 9.0,
                theme.colors.text.0,
            );
            let ts = format_ts(&row.ts);
            text(
                c,
                &ts,
                11.0,
                400,
                rect.x + rect.w - ts.len() as f32 * 6.2 - 12.0,
                rect.y + 10.0,
                theme.colors.text_dim.0,
            );
        }
    }
}

/// "2026-08-30 16:04:11" (UTC, SQLite) -> "30/08 16:04", like formatTs did.
fn format_ts(ts: &str) -> String {
    let (date, time) = ts.split_once(' ').unwrap_or((ts, ""));
    let mut parts = date.split('-');
    let (Some(_y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return ts.to_string();
    };
    let hm: String = time.chars().take(5).collect();
    format!("{d}/{m} {hm}")
}

impl HomeScreen {
    /// Worker answers the two modals need (decks for allocation, search
    /// results for autocomplete, the save answers).
    pub fn on_modal_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = false;
        if self.add_card_open {
            changed |= self.add_card_modal.on_event(event, ctx);
        }
        if self.new_deck_modal.is_open() {
            changed |= self.new_deck_modal.on_event(event, ctx);
        }
        changed
    }
}

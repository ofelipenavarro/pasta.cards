//! Wishlist: cards the user wants and does not own yet.
//!
//! Port of `views/wishlist.js`. Shows a grid of wished cards with
//! filter toolbar and actions to acquire (move to collection) or remove.

use engine::compositor::Compositor;
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{Button, ButtonVariant, EmptyState, EventResult, Rect, WidgetEvent, rounded_rect};

use spellbook_core::client::{Command, Event};
use spellbook_core::ops::collection::CollectionIn;

use super::{EditKey, LabeledField, Route, ScreenCtx, deck_tile, text};
use crate::art::ArtCache;

const WIDTH: f32 = 480.0;
const TILE_MIN_W: f32 = 200.0;
const TILE_MAX_W: f32 = 300.0;
const GAP: f32 = 12.0;

/// What the pointer is over. Layout functions provide the rects, so hover and
/// click never disagree with the pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    WishlistCard(i64),
    AddCardTile,
}

pub struct WishlistScreen {
    wishes: Vec<spellbook_core::ops::collection::WishlistEntry>,
    hover: Option<Hit>,
    loading: bool,
    empty: EmptyState,
    loading_empty: EmptyState,
    filter_state: spellbook::components::FilterState,
    filter_bar: spellbook::components::FilterBar,
}

impl WishlistScreen {
    pub fn new() -> Self {
        let theme = Theme::hoff();
        let filter_bar = spellbook::components::FilterBar::new(theme, |_state| {});

        Self {
            wishes: Vec::new(),
            hover: None,
            loading: true,
            empty: EmptyState::new(
                "Wishlist vazia",
                "Adicione cartas que deseja adquirir",
            )
            .icon("heart"),
            loading_empty: EmptyState::new("Carregando wishlist…", "Lendo sua wishlist do banco local.")
                .icon("heart"),
            filter_state: spellbook::components::FilterState::default(),
            filter_bar,
        }
    }

    pub fn on_enter(&mut self, _route: Route, ctx: &mut ScreenCtx) {
        self.loading = true;
        ctx.send(Command::ListWishlist);
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = self.filter_bar.handle_event(event, content_rect());

        if let Event::WishlistListed(wishes) = event {
            self.wishes = wishes;
            self.loading = false;
            changed = true;
        }

        // Handle filter answer
        if changed {
            ctx.send(Command::FilterWishlist(self.filter_state.clone()));
        }

        changed
    }

    pub fn handle_text(&mut self, _s: &str, _ctx: &mut ScreenCtx) -> bool {
        false
    }

    pub fn handle_edit_key(&mut self, _key: EditKey, _ctx: &mut ScreenCtx) -> bool {
        false
    }

    pub fn handle_escape(&mut self) -> bool {
        false
    }

    pub fn tick(&mut self, _dt: f32) -> bool {
        false
    }

    /// Natural height of the laid-out screen, for the page scrollbar.
    pub fn content_height(&self, content: Rect) -> f32 {
        let rects = self.deck_rects(content);
        let bottom = rects.last().map(|r| r.1.y + r.1.h).unwrap_or(content.y + 240.0);
        (bottom + 24.0 - content.y).max(content.h)
    }

    fn content_rect(&self) -> Rect {
        // Return the inner content area - for filter bar + grid
        Rect::new(0.0, 0.0, 600.0, 800.0) // placeholder - real size from shell
    }

    fn deck_rects(&self, content: Rect) -> Vec<(Hit, Rect)> {
        if self.wishes.is_empty() && self.loading {
            return Vec::new();
        }
        let (cols, col_w) = grid_columns(content.w - 20.0, TILE_MIN_W, TILE_MAX_W, GAP);
        let tile_h = deck_tile::tile_height(col_w);
        let y0 = 80.0;
        let mut out: Vec<(Hit, Rect)> = self
            .wishes
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let (row, col) = (i / cols, i % cols);
                (
                    Hit::WishlistCard(w.id),
                    Rect::new(
                        content.x + 10.0 + col as f32 * (col_w + GAP),
                        y0 + row as f32 * (tile_h + GAP),
                        col_w,
                        tile_h,
                    ),
                )
            })
            .collect();
        let i = self.wishes.len();
        let (row, col) = (i / cols, i % cols);
        out.push((
            Hit::AddCardTile,
            Rect::new(
                content.x + 10.0 + col as f32 * (col_w + GAP),
                y0 + row as f32 * (tile_h + GAP),
                col_w,
                tile_h,
            ),
        ));
        out
    }

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        content: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        if self.loading && self.wishes.is_empty() {
            return self.loading_empty.handle_event(event, content);
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
                Some(Hit::WishlistCard(id)) => {
                    ctx.navigate(Route::WishlistCard(id));
                    EventResult::clicked()
                }
                Some(Hit::AddCardTile) => {
                    ctx.navigate(Route::Collection);
                    EventResult::clicked()
                }
                None => EventResult::IGNORED,
            },
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
        group_label(c, "MINHA WISHLIST", content.x, content.y, theme);

        if self.wishes.is_empty() && !self.loading {
            self.empty.render(c, content, theme);
            return;
        }

        let rects = self.deck_rects(content);
        for (hit, rect) in rects {
            match hit {
                Hit::WishlistCard(id) => {
                    if let Some(wish) = self.wishes.iter().find(|w| w.id == id) {
                        deck_tile::render(c, rect, wish, self.hover == Some(hit) || false, art, theme);
                        // Add acquire button overlay
                        let btn = Button::new("Adquirir")
                            .variant(ButtonVariant::Constructive)
                            .size(80.0, 36.0);
                        btn.render(c, rect.x + rect.w - 90.0, rect.y + rect.h - 44.0, theme);
                    }
                }
                Hit::AddCardTile => {
                    let hovered = self.hover == Some(Hit::AddCardTile);
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
                        "Adicionar Carta",
                        13.0,
                        600,
                        rect.x + rect.w / 2.0 - 50.0,
                        rect.y + rect.h / 2.0 + 2.0,
                        theme.colors.text_dim.0,
                    );
                }
            }
        }
    }
}
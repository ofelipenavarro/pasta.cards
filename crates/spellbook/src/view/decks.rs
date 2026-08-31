//! The deck grid: every deck, plus the modals that create, edit, delete and
//! import into them.
//!
//! Port of `desktop/ui/js/views/decks.js` minus the deck detail view (that is
//! `Route::Deck(id)`, reached by clicking a tile).

use std::sync::mpsc::Sender;

use engine::compositor::{Compositor, LayerId};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{
    ContextMenu, EmptyState, EventResult, MenuEntry, Rect, WidgetEvent, rounded_rect,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::DeckSummary;

use super::{
    EditKey, Route, ScreenCtx, deck_tile, grid_columns, group_label, text, with_alpha,
};
use crate::art::ArtCache;
use crate::view::components::delete_deck::{DeleteDeckAnswer, DeleteDeckModal};
use crate::view::components::edit_deck::{EditDeckAnswer, EditDeckModal};
use crate::view::components::import_deck::{ImportDeckAnswer, ImportDeckModal};
use crate::view::components::new_deck::{NewDeckAnswer, NewDeckModal};

const DECK_GAP: f32 = 16.0;
const TILE_MIN_W: f32 = 240.0;
const TILE_MAX_W: f32 = 340.0;
const MENU_BTN_SIZE: f32 = 28.0;

/// What the pointer is over. Layout functions provide the rects, so hover and
/// click never disagree with the pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Deck(i64),
    NewDeckTile,
    DeckMenu(usize),
}

pub struct DecksScreen {
    decks: Vec<DeckSummary>,
    hover: Option<Hit>,
    loading: bool,
    empty: EmptyState,
    loading_empty: EmptyState,

    new_deck_modal: NewDeckModal,
    edit_deck_modal: EditDeckModal,
    delete_deck_modal: DeleteDeckModal,
    import_deck_modal: ImportDeckModal,

    context_menu: Option<ContextMenuState>,

    /// Command sender captured on enter so `reload` can be called from answers.
    tx: Option<Sender<Command>>,
}

struct ContextMenuState {
    menu: ContextMenu,
    /// Screen-space origin of the menu.
    origin: (f32, f32),
    /// Index into `decks` that the menu belongs to.
    deck_index: usize,
}

impl DecksScreen {
    pub fn new() -> Self {
        let theme = Theme::hoff();
        Self {
            decks: Vec::new(),
            hover: None,
            loading: true,
            empty: EmptyState::new(
                "Nenhum deck ainda",
                "Monte seu primeiro deck, ou deixe o autobuild montar um por você.",
            )
            .icon("layers"),
            loading_empty: EmptyState::new("Carregando decks…", "Lendo seus decks do banco local.")
                .icon("layers"),
            new_deck_modal: NewDeckModal::new(&theme),
            edit_deck_modal: EditDeckModal::new(&theme),
            delete_deck_modal: DeleteDeckModal::new(&theme),
            import_deck_modal: ImportDeckModal::new(&theme),
            context_menu: None,
            tx: None,
        }
    }

    pub fn on_enter(&mut self, _route: Route, ctx: &mut ScreenCtx) {
        self.loading = true;
        self.decks.clear();
        self.tx = Some(ctx.tx.clone());
        ctx.send(Command::ListDecks);
    }

    fn reload(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Command::ListDecks);
        }
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = false;

        changed |= self.new_deck_modal.on_event(event, ctx);
        changed |= self.edit_deck_modal.on_event(event, ctx);
        changed |= self.delete_deck_modal.on_event(event, ctx);
        changed |= self.import_deck_modal.on_event(event, ctx);

        if let Event::DecksListed(decks) = event {
            self.decks = decks.clone();
            self.loading = false;
            return true;
        }

        changed
    }

    pub fn handle_text(&mut self, s: &str, _ctx: &mut ScreenCtx) -> bool {
        if self.new_deck_modal.is_open() {
            return self.new_deck_modal.handle_text(s);
        }
        if self.edit_deck_modal.is_open() {
            return self.edit_deck_modal.handle_text(s);
        }
        if self.import_deck_modal.is_open() {
            return self.import_deck_modal.handle_text(s);
        }
        false
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> bool {
        if self.new_deck_modal.is_open() {
            return self.new_deck_modal.handle_edit_key(key, ctx).changed;
        }
        if self.edit_deck_modal.is_open() {
            return self.edit_deck_modal.handle_edit_key(key, ctx).changed;
        }
        if self.import_deck_modal.is_open() {
            return self.import_deck_modal.handle_edit_key(key, ctx).changed;
        }
        false
    }

    pub fn handle_escape(&mut self) -> bool {
        if self.context_menu.take().is_some() {
            return true;
        }
        if self.new_deck_modal.is_open() {
            if self.new_deck_modal.handle_escape() {
                return true;
            }
            self.new_deck_modal.close();
            return true;
        }
        if self.edit_deck_modal.is_open() {
            if self.edit_deck_modal.handle_escape() {
                return true;
            }
            self.edit_deck_modal.close();
            return true;
        }
        if self.delete_deck_modal.is_open() {
            if self.delete_deck_modal.handle_escape() {
                return true;
            }
            self.delete_deck_modal.close();
            return true;
        }
        if self.import_deck_modal.is_open() {
            if self.import_deck_modal.handle_escape() {
                return true;
            }
            self.import_deck_modal.close();
            return true;
        }
        false
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let mut animating = false;
        if let Some(tx) = &self.tx {
            let mut dummy_actions = Vec::new();
            let mut ctx = ScreenCtx { tx, actions: &mut dummy_actions };
            if self.new_deck_modal.is_open() {
                animating |= self.new_deck_modal.tick(dt, &mut ctx);
            }
            if self.edit_deck_modal.is_open() {
                animating |= self.edit_deck_modal.tick(dt, &mut ctx);
            }
            if self.import_deck_modal.is_open() {
                animating |= self.import_deck_modal.tick(dt, &mut ctx);
            }
        }
        animating
    }

    // -- Overlay --------------------------------------------------------------

    pub fn overlay_open(&self) -> bool {
        self.context_menu.is_some()
            || self.new_deck_modal.is_open()
            || self.edit_deck_modal.is_open()
            || self.delete_deck_modal.is_open()
            || self.import_deck_modal.is_open()
    }

    pub fn handle_overlay_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        // Context menu is top-most and owns the whole window while open.
        if let Some(menu_state) = &mut self.context_menu {
            let (result, clicked_id) = menu_state.menu.handle_event(event, menu_state.origin.0, menu_state.origin.1);
            if let Some(id) = clicked_id {
                let idx = menu_state.deck_index;
                self.context_menu = None;
                if let Some(deck) = self.decks.get(idx).cloned() {
                    match id {
                        0 => self.edit_deck_modal.open(&deck, ctx),
                        1 => self.import_deck_modal.open(deck.id, ctx),
                        2 => self.delete_deck_modal.open(&deck, ctx),
                        _ => {}
                    }
                }
                return EventResult::clicked();
            }
            // Click outside the menu closes it, but only on MouseDown: the
            // MouseUp that follows the opening click must not close it.
            if let WidgetEvent::MouseDown { x, y } = *event {
                let (w, h) = menu_state.menu.size();
                let r = Rect::new(menu_state.origin.0, menu_state.origin.1, w, h);
                if !r.contains(x, y) {
                    self.context_menu = None;
                    return EventResult::changed();
                }
            }
            return result;
        }

        if self.new_deck_modal.is_open() {
            let (answer, result) = self.new_deck_modal.handle_event(event, window, ctx);
            match answer {
                Some(NewDeckAnswer::Created(id)) => {
                    self.new_deck_modal.close();
                    ctx.navigate(Route::Deck(id));
                    self.reload();
                }
                Some(NewDeckAnswer::Cancelled) => self.new_deck_modal.close(),
                None => {}
            }
            return result;
        }

        if self.edit_deck_modal.is_open() {
            let (answer, result) = self.edit_deck_modal.handle_event(event, window, ctx);
            match answer {
                Some(EditDeckAnswer::Saved) => {
                    self.edit_deck_modal.close();
                    ctx.toast("Deck atualizado.", Intent::Constructive);
                    self.reload();
                }
                Some(EditDeckAnswer::Cancelled) => self.edit_deck_modal.close(),
                None => {}
            }
            return result;
        }

        if self.delete_deck_modal.is_open() {
            let (answer, result) = self.delete_deck_modal.handle_event(event, window, ctx);
            match answer {
                Some(DeleteDeckAnswer::Deleted) => {
                    self.delete_deck_modal.close();
                    ctx.toast("Deck excluído.", Intent::Constructive);
                    self.reload();
                }
                Some(DeleteDeckAnswer::Cancelled) => self.delete_deck_modal.close(),
                None => {}
            }
            return result;
        }

        if self.import_deck_modal.is_open() {
            let (answer, result) = self.import_deck_modal.handle_event(event, window, ctx);
            match answer {
                Some(ImportDeckAnswer::Imported) => {
                    self.import_deck_modal.close();
                    ctx.toast("Deck importado.", Intent::Constructive);
                    self.reload();
                }
                Some(ImportDeckAnswer::Cancelled) => self.import_deck_modal.close(),
                None => {}
            }
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
        if let Some(menu_state) = &self.context_menu {
            menu_state.menu.render(c, layer, theme, menu_state.origin.0, menu_state.origin.1);
        }

        if self.new_deck_modal.is_open() {
            self.new_deck_modal.render(c, layer, window, theme);
        } else if self.edit_deck_modal.is_open() {
            self.edit_deck_modal.render(c, layer, window, theme);
        } else if self.delete_deck_modal.is_open() {
            self.delete_deck_modal.render(c, layer, window, theme);
        } else if self.import_deck_modal.is_open() {
            self.import_deck_modal.render(c, layer, window, theme);
        }

        // Make sure art requested by modals (commander images) is queued.
        let _ = art;
    }

    // -- Layout ---------------------------------------------------------------

    fn deck_rects(&self, content: Rect) -> Vec<(Hit, Rect)> {
        if self.decks.is_empty() && self.loading {
            return Vec::new();
        }
        let (cols, col_w) = grid_columns(content.w, TILE_MIN_W, TILE_MAX_W, DECK_GAP);
        let tile_h = deck_tile::tile_height(col_w);
        let y0 = content.y + 26.0;
        let mut out: Vec<(Hit, Rect)> = self
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
        let i = self.decks.len();
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

    fn menu_btn_rect(&self, tile: Rect) -> Rect {
        Rect::new(
            tile.x + tile.w - MENU_BTN_SIZE - 8.0,
            tile.y + 8.0,
            MENU_BTN_SIZE,
            MENU_BTN_SIZE,
        )
    }

    fn hit_at(&self, x: f32, y: f32, content: Rect) -> Option<Hit> {
        for (hit, rect) in self.deck_rects(content) {
            if let Hit::Deck(_) = hit {
                let menu = self.menu_btn_rect(rect);
                if menu.contains(x, y) {
                    // Find the deck index from the hit to open the right menu.
                    if let Some(idx) = self.decks.iter().position(|d| Hit::Deck(d.id) == hit) {
                        return Some(Hit::DeckMenu(idx));
                    }
                }
            }
            if rect.contains(x, y) {
                return Some(hit);
            }
        }
        None
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        let rects = self.deck_rects(content);
        let bottom = rects.last().map(|r| r.1.y + r.1.h).unwrap_or(content.y + 240.0);
        (bottom + 24.0 - content.y).max(content.h)
    }

    // -- Events ---------------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        content: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        if self.loading && self.decks.is_empty() {
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
                Some(Hit::Deck(id)) => {
                    ctx.navigate(Route::Deck(id));
                    EventResult::clicked()
                }
                Some(Hit::NewDeckTile) => {
                    self.new_deck_modal.open(ctx);
                    EventResult::clicked()
                }
                Some(Hit::DeckMenu(idx)) => {
                    if let Some(deck) = self.decks.get(idx) {
                        let rects = self.deck_rects(content);
                        if let Some((_, rect)) = rects.iter().find(|(h, _)| matches!(h, Hit::Deck(id) if *id == deck.id)) {
                            let menu = ContextMenu::new(vec![
                                MenuEntry::item(0, "Editar").icon("pencil"),
                                MenuEntry::item(1, "Importar decklist").icon("download"),
                                MenuEntry::item(2, "Excluir").icon("trash-2").intent(Intent::Destructive),
                            ]);
                            let btn = self.menu_btn_rect(*rect);
                            self.context_menu = Some(ContextMenuState {
                                menu,
                                origin: (btn.x, btn.y + btn.h + 4.0),
                                deck_index: idx,
                            });
                            return EventResult::clicked();
                        }
                    }
                    EventResult::IGNORED
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
        group_label(c, "SEUS DECKS", content.x, content.y, theme);

        if self.decks.is_empty() && !self.loading {
            self.empty.render(c, content, theme);
            return;
        }

        let rects = self.deck_rects(content);
        for (hit, rect) in rects {
            match hit {
                Hit::Deck(id) => {
                    let deck = self
                        .decks
                        .iter()
                        .find(|d| d.id == id)
                        .expect("layout match");
                    let hovered = self.hover == Some(hit) || self.hover == Some(Hit::DeckMenu(self.decks.iter().position(|d| d.id == id).unwrap_or(usize::MAX)));
                    deck_tile::render(c, rect, deck, hovered, art, theme);
                    self.render_menu_btn(c, rect, theme);
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
                Hit::DeckMenu(_) => unreachable!(),
            }
        }
    }

    fn render_menu_btn(&self, c: &mut Compositor, tile: Rect, theme: &Theme) {
        let r = self.menu_btn_rect(tile);
        let hovered = self.hover.is_some_and(|h| {
            if let Hit::DeckMenu(idx) = h {
                let deck_id = self.decks.get(idx).map(|d| d.id);
                self.decks.iter().position(|d| d.id == deck_id.unwrap_or(-1)) == Some(idx)
            } else {
                false
            }
        });
        c.push(rounded_rect(
            r.x,
            r.y,
            r.w,
            r.h,
            theme.radius.md,
            if hovered {
                theme.glass.surface_active.0
            } else {
                with_alpha(theme.colors.surface.0, 0.7)
            },
        ));
        text(c, "⋯", 14.0, 700, r.x + 8.0, r.y + 6.0, theme.colors.text.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ScreenAction;
    use spellbook_core::client::Command;

    fn sample_decks() -> Vec<DeckSummary> {
        vec![DeckSummary {
            id: 1,
            name: "Gishath".into(),
            commander_name: "Gishath, Sun's Avatar".into(),
            commander_name_2: None,
            philosophy: None,
            tags: None,
            created_at: None,
            total_cards: 65,
            wins: 2,
            losses: 1,
            commander_image: None,
            commander_image_2: None,
            color_identity: Some("RGW".into()),
        }]
    }

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    #[test]
    fn on_enter_lists_decks() {
        let mut screen = DecksScreen::new();
        let (mut ctx, rx) = test_ctx();
        screen.on_enter(Route::Decks, &mut ctx);
        assert!(screen.loading);
        let cmd = rx.try_recv().expect("expected ListDecks");
        assert!(matches!(cmd, Command::ListDecks));
    }

    #[test]
    fn decks_listed_populates_grid() {
        let mut screen = DecksScreen::new();
        let (mut ctx, _rx) = test_ctx();
        let changed = screen.on_event(&Event::DecksListed(sample_decks()), &mut ctx);
        assert!(changed);
        assert_eq!(screen.decks.len(), 1);
        assert!(!screen.loading);
    }

    #[test]
    fn clicking_deck_navigates() {
        let mut screen = DecksScreen::new();
        let (mut ctx, _rx) = test_ctx();
        screen.decks = sample_decks();
        let content = Rect::new(300.0, 120.0, 800.0, 600.0);
        let result = screen.handle_event(
            &WidgetEvent::MouseDown { x: 320.0, y: 160.0 },
            content,
            &mut ctx,
        );
        assert!(result.clicked);
        assert!(ctx.actions.iter().any(|a| matches!(a, ScreenAction::Navigate(Route::Deck(1)))));
    }

    #[test]
    fn new_deck_tile_opens_modal() {
        let mut screen = DecksScreen::new();
        let (mut ctx, _rx) = test_ctx();
        screen.decks = sample_decks();
        let content = Rect::new(300.0, 120.0, 800.0, 600.0);
        let rects = screen.deck_rects(content);
        let (_, new_tile) = rects.last().copied().unwrap();
        let result = screen.handle_event(
            &WidgetEvent::MouseDown {
                x: new_tile.x + 10.0,
                y: new_tile.y + 10.0,
            },
            content,
            &mut ctx,
        );
        assert!(result.clicked);
        assert!(screen.new_deck_modal.is_open());
    }

    #[test]
    fn overlay_open_with_modal() {
        let mut screen = DecksScreen::new();
        let (mut ctx, _rx) = test_ctx();
        screen.new_deck_modal.open(&mut ctx);
        assert!(screen.overlay_open());
    }
}

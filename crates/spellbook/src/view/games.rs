//! Games: recorded matches, each deck's win rate, and the form that
//! registers one.
//!
//! Port of `desktop/ui/js/views/games.js`. The header is a row of stat
//! cards (total, W/L/D, win rate), the "Destaques" panel lists the cards
//! that carried the most games, and the history lists every match with
//! its result pill, deck, opponents and highlights. "+ Registrar partida"
//! opens [`AddGameModal`].

use std::sync::mpsc::Sender;

use engine::compositor::{Compositor, LayerId};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{
    Button, EmptyState, EventResult, Rect, WidgetEvent, rounded_rect, rounded_rect_stroke,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::games::{Game, GamesStats};

use super::{EditKey, Route, ScreenCtx, group_label, panel, text};
use crate::art::ArtCache;
use crate::view::components::add_game::{AddGameAnswer, AddGameModal};

const BTN_ROW_H: f32 = 44.0;
const STAT_H: f32 = 92.0;
const STAT_GAP: f32 = 12.0;
const HIGHLIGHT_ROW_H: f32 = 26.0;
const ROW_H: f32 = 76.0;
const ROW_GAP: f32 = 8.0;

pub struct GamesScreen {
    games: Vec<Game>,
    stats: Option<Box<GamesStats>>,
    loading: bool,
    add_btn: Button,
    add_game_modal: AddGameModal,
    hover_add: bool,
    empty: EmptyState,
    loading_empty: EmptyState,
    /// Command sender captured on enter so the answer handler can reload.
    tx: Option<Sender<Command>>,
}

impl GamesScreen {
    pub fn new(theme: &Theme) -> Self {
        Self {
            games: Vec::new(),
            stats: None,
            loading: true,
            add_btn: Button::new("+ Registrar partida"),
            add_game_modal: AddGameModal::new(theme),
            hover_add: false,
            empty: EmptyState::new(
                "Nenhuma partida registrada",
                "Registre vitórias, derrotas e os destaques de cada jogo.",
            )
            .icon("layout-grid"),
            loading_empty: EmptyState::new(
                "Carregando partidas…",
                "Lendo o histórico do banco local.",
            )
            .icon("layout-grid"),
            tx: None,
        }
    }

    pub fn on_enter(&mut self, _route: Route, ctx: &mut ScreenCtx) {
        self.loading = true;
        self.games.clear();
        self.tx = Some(ctx.tx.clone());
        ctx.send(Command::ListGames);
        ctx.send(Command::GamesStats);
    }

    fn reload(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Command::ListGames);
            let _ = tx.send(Command::GamesStats);
        }
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = self.add_game_modal.on_event(event, ctx);

        match event {
            Event::GamesListed(games) => {
                self.games = games.clone();
                self.loading = false;
                return true;
            }
            Event::GamesStatsLoaded(stats) => {
                self.stats = Some(stats.clone());
                return true;
            }
            Event::GameAdded(result)
                if self.add_game_modal.claims_game_added() => {
                    changed = true;
                    match result {
                        Ok(_) => {
                            self.add_game_modal.close();
                            ctx.toast("Partida registrada.", Intent::Constructive);
                            self.reload();
                        }
                        Err(e) => self.add_game_modal.set_error(e.detail().to_string()),
                    }
                }
            _ => {}
        }
        changed
    }

    pub fn handle_text(&mut self, s: &str, ctx: &mut ScreenCtx) -> bool {
        if self.add_game_modal.is_open() {
            return self.add_game_modal.handle_text(s, ctx);
        }
        false
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> bool {
        if self.add_game_modal.is_open() {
            return self.add_game_modal.handle_edit_key(key, ctx).changed;
        }
        false
    }

    pub fn handle_escape(&mut self) -> bool {
        if self.add_game_modal.is_open() {
            if self.add_game_modal.handle_escape() {
                return true;
            }
            self.add_game_modal.close();
            return true;
        }
        false
    }

    pub fn tick(&mut self, _dt: f32) -> bool {
        false
    }

    pub fn overlay_open(&self) -> bool {
        self.add_game_modal.is_open()
    }

    pub fn handle_overlay_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        let (answer, result) = self.add_game_modal.handle_event(event, window, ctx);
        match answer {
            Some(AddGameAnswer::Saved) => {
                self.add_game_modal.close();
                ctx.toast("Partida registrada.", Intent::Constructive);
                self.reload();
            }
            Some(AddGameAnswer::Cancelled) => self.add_game_modal.close(),
            None => {}
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
        let _ = art;
        if self.add_game_modal.is_open() {
            self.add_game_modal.render(c, layer, window, theme);
        }
    }

    // -- Layout ---------------------------------------------------------------

    fn add_btn_rect(&self, content: Rect) -> Rect {
        let (w, h) = self.add_btn.preferred_size();
        let x = (content.x + content.w - w).max(content.x);
        Rect::new(x, content.y + (BTN_ROW_H - h).max(0.0) / 2.0, w, h.max(BTN_ROW_H))
    }

    /// The three stat cards, in the JS's order: win rate, wins, losses.
    fn stat_rects(&self, content: Rect) -> Vec<Rect> {
        let (cols, col_w) = super::grid_columns(content.w, 170.0, 280.0, STAT_GAP);
        (0..3)
            .map(|i| {
                let (row, col) = (i / cols, i % cols);
                Rect::new(
                    content.x + col as f32 * (col_w + STAT_GAP),
                    content.y + BTN_ROW_H + 12.0 + row as f32 * (STAT_H + STAT_GAP),
                    col_w,
                    STAT_H,
                )
            })
            .collect()
    }

    /// Row-wise Y position a chip row needs — highlights are inline chips
    /// that wrap; this returns the height consumed for `n` chips.
    fn chips_height(&self, content: Rect, n: usize) -> f32 {
        if n == 0 {
            return 0.0;
        }
        const CHIP_W: f32 = 130.0;
        const CHIP_ROW_H: f32 = 30.0;
        let per_row = ((content.w / (CHIP_W + 8.0)).floor() as usize).max(1);
        n.div_ceil(per_row) as f32 * CHIP_ROW_H
    }

    /// Y of the "Destaques" group label.
    fn highlights_y(&self, content: Rect) -> f32 {
        content.y + BTN_ROW_H + 12.0 + STAT_H + 30.0
    }

    /// Y where the highlight chip rows start.
    fn highlight_rows_y(&self, content: Rect) -> f32 {
        self.highlights_y(content) + 26.0
    }

    /// Y of the "Histórico" group label.
    fn history_y(&self, content: Rect) -> f32 {
        let n = self
            .stats
            .as_ref()
            .map(|s| s.top_highlight_cards.len().min(10))
            .unwrap_or(0);
        self.highlight_rows_y(content) + self.chips_height(content, n) + 24.0
    }

    fn row_rects(&self, content: Rect) -> Vec<Rect> {
        let y0 = self.history_y(content) + 26.0;
        self.games
            .iter()
            .enumerate()
            .map(|(i, _)| {
                Rect::new(content.x, y0 + i as f32 * (ROW_H + ROW_GAP), content.w, ROW_H)
            })
            .collect()
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        let rects = self.row_rects(content);
        let bottom = rects
            .last()
            .map(|r| r.y + r.h)
            .unwrap_or(self.history_y(content));
        (bottom + 24.0 - content.y).max(content.h)
    }

    fn hit_at(&self, x: f32, y: f32, content: Rect) -> bool {
        self.add_btn_rect(content).contains(x, y)
    }

    // -- Events ---------------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        content: Rect,
        ctx: &mut ScreenCtx,
    ) -> EventResult {
        if self.loading && self.games.is_empty() && self.stats.is_none() {
            return self.loading_empty.handle_event(event, content);
        }
        match *event {
            WidgetEvent::MouseMove { x, y } => {
                let hovered = self.hit_at(x, y, content);
                if hovered != self.hover_add {
                    self.hover_add = hovered;
                    EventResult::changed()
                } else {
                    EventResult::IGNORED
                }
            }
            WidgetEvent::MouseDown { x, y } => {
                if self.hit_at(x, y, content) {
                    self.add_game_modal.open(ctx);
                    EventResult::clicked()
                } else {
                    EventResult::IGNORED
                }
            }
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
        let _ = art;
        let add_rect = self.add_btn_rect(content);
        self.add_btn.render(c, add_rect, theme);

        let Some(stats) = &self.stats else {
            if self.loading {
                self.loading_empty.render(c, content, theme);
            } else {
                self.empty.render(c, content, theme);
            }
            return;
        };

        // Stat cards, the JS's three: win rate, wins, losses.
        let win_rate = stats
            .win_rate
            .map(|r| format!("{r:.1}%"))
            .unwrap_or_else(|| "—".into());
        let card_data: [(&str, String, String); 3] = [
            (
                "TAXA DE VITÓRIA",
                win_rate,
                format!("{} partidas", stats.total_games),
            ),
            ("VITÓRIAS", format!("{}", stats.wins), String::new()),
            ("DERROTAS", format!("{}", stats.losses), String::new()),
        ];
        for (i, rect) in self.stat_rects(content).iter().enumerate() {
            let (label, value, sub) = &card_data[i];
            panel(c, *rect, theme);
            text(
                c,
                label,
                10.0,
                600,
                rect.x + 14.0,
                rect.y + 14.0,
                theme.glass.text_placeholder.0,
            );
            let value_color = match i {
                1 => theme.colors.success.0,
                2 => theme.colors.danger.0,
                _ => theme.colors.text.0,
            };
            text(
                c,
                value,
                26.0,
                600,
                rect.x + 14.0,
                rect.y + 30.0,
                value_color,
            );
            if !sub.is_empty() {
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
        }

        // Highlights: inline chips ("Nome · Nx"), like the JS.
        group_label(c, "CARTAS QUE MAIS SE DESTACARAM", content.x, self.highlights_y(content), theme);
        if stats.top_highlight_cards.is_empty() {
            text(
                c,
                "Nenhum destaque registrado ainda.",
                12.0,
                400,
                content.x,
                self.highlight_rows_y(content),
                theme.colors.text_dim.0,
            );
        } else {
            let mut hx = content.x;
            let mut hy = self.highlight_rows_y(content);
            for hl in stats.top_highlight_cards.iter().take(10) {
                let label = format!("{} · {}x", hl.card_name, hl.n);
                let cw = label.len() as f32 * 6.4 + 20.0;
                if hx + cw > content.x + content.w {
                    hx = content.x;
                    hy += HIGHLIGHT_ROW_H + 6.0;
                }
                c.push(rounded_rect(
                    hx,
                    hy,
                    cw,
                    24.0,
                    12.0,
                    theme.glass.surface_active.0,
                ));
                c.push(rounded_rect_stroke(
                    hx,
                    hy,
                    cw,
                    24.0,
                    12.0,
                    theme.glass.edge_soft.0,
                    1.0,
                ));
                text(c, &label, 11.0, 500, hx + 10.0, hy + 6.0, theme.colors.text.0);
                hx += cw + 8.0;
            }
        }

        // History.
        group_label(c, "HISTÓRICO", content.x, self.history_y(content), theme);
        let rects = self.row_rects(content);
        if rects.is_empty() {
            text(
                c,
                "Nenhuma partida registrada — clique em \"+ Registrar partida\".",
                12.0,
                400,
                content.x,
                self.history_y(content) + 26.0,
                theme.colors.text_dim.0,
            );
        }
        for (rect, game) in rects.iter().zip(self.games.iter()) {
            self.render_row(c, *rect, game, theme);
        }
    }

    fn render_row(&self, c: &mut Compositor, rect: Rect, game: &Game, theme: &Theme) {
        panel(c, rect, theme);

        // Result pill.
        let (label, color) = match game.result.as_str() {
            "vitoria" => ("VITÓRIA", theme.colors.success.0),
            "empate" => ("EMPATE", theme.colors.warning.0),
            _ => ("DERROTA", theme.colors.danger.0),
        };
        let pill_w = label.len() as f32 * 7.0 + 20.0;
        c.push(rounded_rect(
            rect.x + 12.0,
            rect.y + (rect.h - 24.0) / 2.0,
            pill_w,
            24.0,
            12.0,
            color,
        ));
        text(
            c,
            label,
            10.0,
            700,
            rect.x + 22.0,
            rect.y + (rect.h - 24.0) / 2.0 + 5.0,
            [0.05, 0.05, 0.05, 1.0],
        );

        // Deck + opponents.
        text(
            c,
            &game.deck_name,
            13.0,
            600,
            rect.x + pill_w + 24.0,
            rect.y + 12.0,
            theme.colors.text.0,
        );
        if let Some(opponents) = game.opponents.as_deref().filter(|o| !o.is_empty()) {
            text(
                c,
                opponents,
                11.0,
                400,
                rect.x + pill_w + 24.0,
                rect.y + 32.0,
                theme.colors.text_dim.0,
            );
        }
        if let Some(turns) = game.turns {
            text(
                c,
                &format!("{turns} turnos"),
                11.0,
                400,
                rect.x + pill_w + 24.0,
                rect.y + 48.0,
                theme.glass.text_placeholder.0,
            );
        }

        // Highlights inline after the deck info.
        if !game.highlights.is_empty() {
            let joined = game.highlights.join(", ");
            text(
                c,
                &joined,
                10.0,
                400,
                rect.x + rect.w * 0.55,
                rect.y + 12.0,
                theme.glass.text_placeholder.0,
            );
        }

        // Notes, second row under deck/opponents.
        if let Some(notes) = game.notes.as_deref().filter(|n| !n.is_empty()) {
            let ny = if rect.h > 56.0 && game.turns.is_some() {
                rect.y + 54.0
            } else {
                rect.y + 50.0
            };
            text(c, notes, 11.0, 400, rect.x + pill_w + 24.0, ny, theme.glass.text_placeholder.0);
        }

        // Timestamp, top-right.
        let ts = format_game_ts(&game.played_at);
        text(
            c,
            &ts,
            11.0,
            400,
            rect.x + rect.w - ts.len() as f32 * 6.2 - 12.0,
            rect.y + 10.0,
            theme.glass.text_placeholder.0,
        );
    }
}

/// "2026-08-30 16:04:11" (UTC, SQLite) -> "30/08 16:04", like formatTs did.
/// Falls back to the raw string when the shape is unexpected.
fn format_game_ts(ts: &str) -> String {
    let (date, time) = ts.split_once(' ').unwrap_or((ts, ""));
    let mut parts = date.split('-');
    let (Some(_y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return ts.to_string();
    };
    let hm: String = time.chars().take(5).collect();
    format!("{d}/{m} {hm}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellbook_core::client::Command;

    fn test_screen() -> GamesScreen {
        let theme = Theme::hoff();
        GamesScreen::new(&theme)
    }

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    #[test]
    fn on_enter_requests_games_and_stats() {
        let mut screen = test_screen();
        let (mut ctx, rx) = test_ctx();
        screen.on_enter(Route::Games, &mut ctx);
        let first = rx.try_recv().expect("ListGames");
        let second = rx.try_recv().expect("GamesStats");
        assert!(matches!(first, Command::ListGames));
        assert!(matches!(second, Command::GamesStats));
    }

    #[test]
    fn games_listed_populates_rows() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        let changed = screen.on_event(&Event::GamesListed(vec![Game::default()]), &mut ctx);
        assert!(changed);
        assert_eq!(screen.games.len(), 1);
        assert!(!screen.loading);
    }

    #[test]
    fn stats_loaded_populates_header() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        let changed = screen.on_event(
            &Event::GamesStatsLoaded(Box::default()),
            &mut ctx,
        );
        assert!(changed);
        assert!(screen.stats.is_some());
    }

    #[test]
    fn add_button_opens_modal() {
        let mut screen = test_screen();
        let (mut ctx, _rx) = test_ctx();
        screen.on_event(&Event::GamesListed(vec![]), &mut ctx);
        screen.on_event(&Event::GamesStatsLoaded(Box::default()), &mut ctx);
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
        assert!(screen.add_game_modal.is_open());
        assert!(screen.overlay_open());
    }
}
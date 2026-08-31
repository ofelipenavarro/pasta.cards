//! "Registrar partida" modal. Port of `openGameModal` in
//! `desktop/ui/js/views/games.js`.
//!
//! Deck comes from a select fed by `ListDecks`; result is a segmented
//! strip (vitória/derrota/empate); highlights is a comma-separated name
//! list, exactly the shape `game_highlights` stores one row per name of.
//! Saving sends one `AddGame`; the screen reads the `GameAdded` answer
//! (via [`Self::claims_game_added`]) and closes with
//! [`AddGameAnswer::Saved`].

use engine::compositor::Compositor;
use engine::theme::Theme;
use engine::ui::widgets::{
    Button, ButtonVariant, EventResult, Rect, Select, Tabs, WidgetEvent,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::DeckSummary;
use spellbook_core::ops::games::GameIn;

use super::super::{EditKey, ScreenCtx, text};
use super::field::{LabeledField, FIELD_H};
use super::modal::ModalFrame;

const WIDTH: f32 = 460.0;
const TABS_H: f32 = 44.0;
const BTN_H: f32 = 44.0;
/// Played-at defaults to today, the JS's `new Date()` input value. Stored
/// as the local date; the schema keeps it TEXT.
const MAX_HIGHLIGHTS: usize = 6;

/// What the modal tells the screen when it closes for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddGameAnswer {
    /// The game was recorded; the screen should reload.
    Saved,
    /// Dismissed without saving.
    Cancelled,
}

#[derive(Default)]
enum Save {
    #[default]
    Idle,
    /// Awaiting `GameAdded`.
    Saving,
}

pub struct AddGameModal {
    frame: ModalFrame,
    decks: Vec<DeckSummary>,
    deck: Select,
    result_tabs: Tabs,
    result: usize,
    played_at: LabeledField,
    opponents: LabeledField,
    turns: LabeledField,
    notes: LabeledField,
    highlights: LabeledField,

    saving: Save,
    /// One save in flight that this modal owns; the screen checks it so a
    /// stale `GameAdded` from elsewhere is ignored.
    save_generation: u64,
    generation_seen: u64,
    error: Option<String>,
    open: bool,

    save: Button,
    cancel: Button,
}

#[derive(Clone, Copy, Debug, Default)]
struct Layout {
    deck_label: Rect,
    deck: Rect,
    result: Rect,
    played_at: Rect,
    opponents: Rect,
    turns: Rect,
    notes: Rect,
    highlights: Rect,
    error: Rect,
    save: Rect,
    cancel: Rect,
    height: f32,
}

impl AddGameModal {
    pub fn new(theme: &Theme) -> Self {
        let mut played_at = LabeledField::new("Data da partida", "2026-08-30 16:04", theme);
        played_at.set_value(&today_default());
        Self {
            frame: ModalFrame::new(),
            decks: Vec::new(),
            deck: Select::new(["Carregando decks…"], 0),
            result_tabs: Tabs::new(["Vitória", "Derrota", "Empate"]),
            result: 0,
            played_at,
            opponents: LabeledField::new("Oponentes", "Nomes separados por vírgula", theme),
            turns: LabeledField::new("Turnos", "Ex: 12", theme),
            notes: LabeledField::new("Notas", "Como o jogo foi", theme),
            highlights: LabeledField::new(
                "Cartas destaque",
                "Sol Ring, Cyclonic Rift…",
                theme,
            ),
            saving: Save::Idle,
            save_generation: 0,
            generation_seen: 0,
            error: None,
            open: false,
            save: Button::new("Registrar"),
            cancel: Button::new("Cancelar").variant(ButtonVariant::Outline),
        }
    }

    pub fn open(&mut self, ctx: &mut ScreenCtx) {
        ctx.send(Command::ListDecks);
        self.open = true;
        self.error = None;
        self.saving = Save::Idle;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
        self.saving = Save::Idle;
    }

    /// Whether the next `GameAdded` belongs to this modal's save.
    pub fn claims_game_added(&self) -> bool {
        self.save_generation != self.generation_seen
    }

    /// Surface a save error inside the modal (screen-side handler path).
    pub fn set_error(&mut self, message: String) {
        self.error = Some(message);
        self.saving = Save::Idle;
        self.generation_seen = self.save_generation;
    }

    fn result_code(&self) -> &'static str {
        ["vitoria", "derrota", "empate"][self.result.min(2)]
    }

    fn save(&mut self, ctx: &mut ScreenCtx) {
        if self.open_saving() {
            return;
        }
        self.error = None;
        let Some(deck) = self.decks.get(self.deck.selected) else {
            self.error = Some("Nenhum deck selecionado.".into());
            return;
        };
        let deck_id = deck.id;
        let played_at = self.played_at.value().trim().to_string();
        if played_at.is_empty() {
            self.error = Some("Preencha a data da partida.".into());
            return;
        }
        let turns = self.turns.value().trim().parse::<i64>().ok();
        let highlights: Vec<String> = self
            .highlights
            .value()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .take(MAX_HIGHLIGHTS)
            .map(str::to_string)
            .collect();
        self.save_generation += 1;
        self.saving = Save::Saving;
        ctx.send(Command::AddGame(Box::new(GameIn {
            deck_id,
            played_at,
            result: self.result_code().into(),
            opponents: self.opponents.value_opt(),
            turns,
            notes: self.notes.value_opt(),
            highlights,
        })));
    }

    fn open_saving(&self) -> bool {
        matches!(self.saving, Save::Saving)
    }

    /// Worker answers. Returns `true` when the frame changed.
    pub fn on_event(&mut self, event: &Event, _ctx: &mut ScreenCtx) -> bool {
        match event {
            Event::DecksListed(decks) => {
                self.decks = decks.clone();
                let options: Vec<String> = decks.iter().map(|d| d.name.clone()).collect();
                if options.is_empty() {
                    self.deck.options = vec!["Nenhum deck cadastrado".into()];
                } else {
                    self.deck.options = options;
                    self.deck.selected = self.deck.selected.min(decks.len() - 1);
                }
                true
            }
            // The save answer is read by the screen through
            // `claims_game_added`; nothing to do here.
            _ => false,
        }
    }

    // -- Layout ---------------------------------------------------------------

    fn layout(&self, content: Rect) -> Layout {
        let mut l = Layout::default();
        let w = content.w;
        let mut y = content.y;

        l.deck_label = Rect::new(content.x, y, w, 16.0);
        y += 18.0;
        l.deck = Rect::new(content.x, y, w, FIELD_H);
        y += FIELD_H + 12.0;

        l.result = Rect::new(content.x, y, w, TABS_H);
        y += TABS_H + 12.0;

        l.played_at = Rect::new(content.x, y, w, LabeledField::height());
        y += l.played_at.h + 10.0;
        l.opponents = Rect::new(content.x, y, w, LabeledField::height());
        y += l.opponents.h + 10.0;
        l.turns = Rect::new(content.x, y, w, LabeledField::height());
        y += l.turns.h + 10.0;
        l.notes = Rect::new(content.x, y, w, LabeledField::height());
        y += l.notes.h + 10.0;
        l.highlights = Rect::new(content.x, y, w, LabeledField::height());
        y += l.highlights.h + 8.0;

        if self.error.is_some() {
            l.error = Rect::new(content.x, y, w, 18.0);
            y += 24.0;
        }

        let (save_w, _) = self.save.preferred_size();
        let (cancel_w, _) = self.cancel.preferred_size();
        l.save = Rect::new(content.x + w - save_w.max(110.0), y, save_w.max(110.0), BTN_H);
        l.cancel = Rect::new(
            l.save.x - 8.0 - cancel_w.max(100.0),
            y,
            cancel_w.max(100.0),
            BTN_H,
        );
        l.height = y - content.y + BTN_H;
        l
    }

    fn geometry(&mut self, window: Rect) -> (Rect, Layout) {
        let inner_w = WIDTH - super::modal::PAD * 2.0;
        let probe = self.layout(Rect::new(0.0, 0.0, inner_w, 0.0));
        let panel = self.frame.rect(window, WIDTH, probe.height);
        let content = self.frame.content_rect(panel);
        (panel, self.layout(content))
    }

    // -- Pointer input --------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> (Option<AddGameAnswer>, EventResult) {
        let (panel, l) = self.geometry(window);
        let (close, mut result) = self.frame.handle_event(event, panel);
        if close {
            return (Some(AddGameAnswer::Cancelled), result);
        }

        if self.deck.is_open() {
            return (None, result.merge(self.deck.handle_event(event, l.deck)));
        }

        // Result strip.
        let tabs_r = self.result_tabs.handle_event(event, l.result);
        if tabs_r.clicked {
            self.result = self.result_tabs.active;
        }
        result = result.merge(tabs_r);

        if let WidgetEvent::MouseDown { x, y } = *event {
            for rect in [
                l.played_at,
                l.opponents,
                l.turns,
                l.notes,
                l.highlights,
            ] {
                if rect.contains(x, y) {
                    result = result.merge(EventResult::changed());
                }
            }
        }

        let cancel_r = self.cancel.handle_event(event, l.cancel);
        if cancel_r.clicked {
            return (Some(AddGameAnswer::Cancelled), EventResult::clicked());
        }
        result = result.merge(cancel_r);

        let save_r = self.save.handle_event(event, l.save);
        if save_r.clicked {
            self.save(ctx);
            result = result.merge(EventResult::clicked());
        } else {
            result = result.merge(save_r);
        }

        (None, result)
    }

    pub fn handle_text(&mut self, s: &str, _ctx: &mut ScreenCtx) -> bool {
        self.played_at.handle_text(s)
            | self.opponents.handle_text(s)
            | self.turns.handle_text(s)
            | self.notes.handle_text(s)
            | self.highlights.handle_text(s)
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> EventResult {
        match key {
            EditKey::Enter => {
                if !self.open_saving() {
                    self.save(ctx);
                }
                EventResult::clicked()
            }
            _ => EventResult::IGNORED,
        }
    }

    /// Escape while a save is in flight must not close under it. `false`
    /// tells the screen to close the modal itself.
    pub fn handle_escape(&mut self) -> bool {
        self.open_saving()
    }

    pub fn tick(&mut self, _dt: f32, _ctx: &mut ScreenCtx) -> bool {
        false
    }

    // -- Render ---------------------------------------------------------------

    pub fn render(&mut self, c: &mut Compositor, layer: engine::compositor::LayerId, window: Rect, theme: &Theme) {
        let (panel, l) = self.geometry(window);
        self.frame.render(c, layer, window, panel, "Registrar partida", theme);

        text(
            c,
            "Deck",
            12.0,
            400,
            l.deck_label.x,
            l.deck_label.y,
            theme.colors.text_dim.0,
        );
        self.deck.render(c, l.deck, theme);
        self.result_tabs.render(c, l.result, theme);
        self.played_at.render(c, l.played_at, theme);
        self.opponents.render(c, l.opponents, theme);
        self.turns.render(c, l.turns, theme);
        self.notes.render(c, l.notes, theme);
        self.highlights.render(c, l.highlights, theme);

        if let Some(error) = &self.error {
            text(c, error, 12.0, 400, l.error.x, l.error.y, theme.colors.danger.0);
        }

        self.cancel.render(c, l.cancel, theme);
        self.save.render(c, l.save, theme);
        self.deck.render_dropdown(c, layer, l.deck, theme);
    }
}

/// "2026-08-30 16:04" - the JS prefilled `new Date()`; here, UTC now
/// formatted the only way the schema stores timestamps.
fn today_default() -> String {
    // No chrono in this crate: the schema's DEFAULT CURRENT_TIMESTAMP is
    // what real saves use, so the prefill is display sugar. Keep it simple
    // and stable: the field starts empty and the DB fills the timestamp.
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    fn modal_with_deck() -> (
        AddGameModal,
        ScreenCtx<'static>,
        std::sync::mpsc::Receiver<Command>,
    ) {
        let theme = Theme::hoff();
        let mut modal = AddGameModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&mut ctx);
        // Answer the ListDecks the open path sent.
        modal.on_event(
            &spellbook_core::client::Event::DecksListed(vec![DeckSummary {
                id: 3,
                name: "Gishath".into(),
                commander_name: "Gishath, Sun's Avatar".into(),
                commander_name_2: None,
                philosophy: None,
                tags: None,
                created_at: None,
                total_cards: 100,
                wins: 0,
                losses: 0,
                commander_image: None,
                commander_image_2: None,
                color_identity: None,
            }]),
            &mut ctx,
        );
        (modal, ctx, rx)
    }

    #[test]
    fn open_requests_decks() {
        let theme = Theme::hoff();
        let mut modal = AddGameModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&mut ctx);
        let cmd = rx.try_recv().expect("ListDecks");
        assert!(matches!(cmd, Command::ListDecks));
        assert!(modal.is_open());
    }

    #[test]
    fn save_sends_add_game_with_parsed_fields() {
        let (mut modal, mut ctx, rx) = modal_with_deck();
        modal.turns.set_value("12");
        modal.opponents.set_value("Ana, Beto");
        modal.played_at.set_value("2026-08-30 16:04");
        modal
            .highlights
            .set_value("Sol Ring, Cyclonic Rift, , Avenger");

        modal.save(&mut ctx);
        // Drain the ListDecks the open path queued; only the AddGame
        // belongs to save.
        let _ = rx.try_recv();
        let cmd = rx.try_recv().expect("AddGame");
        match cmd {
            Command::AddGame(payload) => {
                assert_eq!(payload.deck_id, 3);
                assert_eq!(payload.result, "vitoria");
                assert_eq!(payload.played_at, "2026-08-30 16:04");
                assert_eq!(payload.turns, Some(12));
                assert_eq!(payload.opponents.as_deref(), Some("Ana, Beto"));
                assert_eq!(
                    payload.highlights,
                    vec!["Sol Ring".to_string(), "Cyclonic Rift".to_string(), "Avenger".to_string()]
                );
            }
            other => panic!("unexpected command: {:?}", other),
        }
        assert!(modal.claims_game_added());
    }

    #[test]
    fn save_without_decks_sets_error() {
        let theme = Theme::hoff();
        let mut modal = AddGameModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&mut ctx);
        modal.save(&mut ctx);
        assert_eq!(modal.error.as_deref(), Some("Nenhum deck selecionado."));
    }

    #[test]
    fn result_tab_selection_maps_to_code() {
        let theme = Theme::hoff();
        let mut modal = AddGameModal::new(&theme);
        modal.result_tabs.active = 2;
        modal.result = 2;
        assert_eq!(modal.result_code(), "empate");
        modal.result_tabs.active = 1;
        modal.result = 1;
        assert_eq!(modal.result_code(), "derrota");
    }
}
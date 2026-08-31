//! "Excluir deck" modal. Port of `openDeleteDeckModal` in
//! `desktop/ui/js/views/decks.js`.
//!
//! Asks whether allocated collection copies should be returned to the free
//! pool (`DeleteMode::Free`) or removed from the collection (`DeleteMode::Remove`).

use engine::compositor::{Compositor, LayerId};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{
    Button, ButtonVariant, Checkbox, EventResult, Rect, WidgetEvent,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::{DeckDetail, DeckSummary, DeleteMode};

use super::super::{EditKey, ScreenCtx, text};
use super::modal::{ModalFrame, PAD as MODAL_PAD};

const WIDTH: f32 = 440.0;
const BTN_H: f32 = 44.0;

/// What the modal tells the screen when it closes for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteDeckAnswer {
    /// The deletion was confirmed and succeeded; the screen should reload.
    Deleted,
    /// Dismissed without deleting.
    Cancelled,
}

enum Phase {
    Idle,
    Loading,
    Confirming,
    Deleting,
}

pub struct DeleteDeckModal {
    frame: ModalFrame,
    deck_id: Option<i64>,
    deck_name: String,
    detail: Option<Box<DeckDetail>>,
    phase: Phase,

    free: Checkbox,
    remove: Checkbox,
    delete: Button,
    cancel: Button,

    error: Option<String>,
    open: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Layout {
    body: Rect,
    free: Rect,
    remove: Rect,
    error: Rect,
    delete: Rect,
    cancel: Rect,
    height: f32,
}

impl DeleteDeckModal {
    pub fn new(_theme: &Theme) -> Self {
        Self {
            frame: ModalFrame::new(),
            deck_id: None,
            deck_name: String::new(),
            detail: None,
            phase: Phase::Idle,
            free: Checkbox::new(true).label("Devolver para cartas livres"),
            remove: Checkbox::new(false).label("Remover também da coleção"),
            delete: Button::new("Excluir deck").intent(Intent::Destructive),
            cancel: Button::new("Cancelar").variant(ButtonVariant::Outline),
            error: None,
            open: false,
        }
    }

    pub fn open(&mut self, deck: &DeckSummary, ctx: &mut ScreenCtx) {
        self.deck_id = Some(deck.id);
        self.deck_name = deck.name.clone();
        self.detail = None;
        self.phase = Phase::Loading;
        self.error = None;
        self.free.checked = true;
        self.remove.checked = false;
        ctx.send(Command::GetDeck { deck_id: deck.id });
        self.open = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
        self.deck_id = None;
        self.detail = None;
        self.phase = Phase::Idle;
    }

    pub fn on_event(&mut self, event: &Event, _ctx: &mut ScreenCtx) -> bool {
        match event {
            Event::DeckLoaded { deck_id, result } => {
                if self.deck_id != Some(*deck_id) {
                    return false;
                }
                match result {
                    Ok(detail) => {
                        self.detail = Some(detail.clone());
                        self.phase = Phase::Confirming;
                        true
                    }
                    Err(e) => {
                        self.error = Some(e.detail().to_string());
                        self.phase = Phase::Confirming;
                        true
                    }
                }
            }
            Event::DeckDeleted(result) => {
                if !matches!(self.phase, Phase::Deleting) {
                    return false;
                }
                self.phase = Phase::Idle;
                match result {
                    Ok(_) => {
                        self.error = None;
                        true
                    }
                    Err(e) => {
                        self.error = Some(e.detail().to_string());
                        true
                    }
                }
            }
            _ => false,
        }
    }

    pub fn just_deleted(&self) -> bool {
        self.deck_id.is_some()
            && matches!(self.phase, Phase::Idle)
            && self.error.is_none()
    }

    fn card_count(&self) -> i64 {
        let Some(detail) = &self.detail else {
            return 0;
        };
        detail
            .by_type
            .iter()
            .filter(|(cat, _)| *cat != "Comandante")
            .map(|(_, cards)| cards.iter().map(|c| c.quantity).sum::<i64>())
            .sum()
    }

    fn body_text(&self) -> String {
        let count = self.card_count();
        if count > 0 {
            format!(
                "Este deck tem {count} carta(s) não-comandante na lista. O que fazer com as cópias alocadas à coleção?"
            )
        } else {
            "Nenhuma carta não-comandante está neste deck. O comandante será removido junto com o deck.".into()
        }
    }

    fn layout(&self, content: Rect) -> Layout {
        let mut l = Layout::default();
        let w = content.w;
        let mut y = content.y;

        l.body = Rect::new(content.x, y, w, 44.0);
        y += l.body.h + 16.0;

        l.free = Rect::new(content.x, y, w, 42.0);
        y += l.free.h + 8.0;
        l.remove = Rect::new(content.x, y, w, 42.0);
        y += l.remove.h + 12.0;

        if self.error.is_some() {
            l.error = Rect::new(content.x, y, w, 18.0);
            y += 24.0;
        }

        let (delete_w, _) = self.delete.preferred_size();
        let (cancel_w, _) = self.cancel.preferred_size();
        l.delete = Rect::new(
            content.x + w - delete_w.max(110.0),
            y,
            delete_w.max(110.0),
            BTN_H,
        );
        l.cancel = Rect::new(
            l.delete.x - 8.0 - cancel_w.max(100.0),
            y,
            cancel_w.max(100.0),
            BTN_H,
        );
        l.height = y - content.y + BTN_H;
        l
    }

    fn geometry(&mut self, window: Rect) -> (Rect, Layout) {
        let inner_w = WIDTH - MODAL_PAD * 2.0;
        let probe = self.layout(Rect::new(0.0, 0.0, inner_w, 0.0));
        let panel = self.frame.rect(window, WIDTH, probe.height);
        let content = self.frame.content_rect(panel);
        (panel, self.layout(content))
    }

    fn confirm(&mut self, ctx: &mut ScreenCtx) {
        let Some(deck_id) = self.deck_id else {
            self.error = Some("Nenhum deck selecionado.".into());
            return;
        };
        let mode = if self.remove.checked {
            DeleteMode::Remove
        } else {
            DeleteMode::Free
        };
        self.phase = Phase::Deleting;
        ctx.send(Command::DeleteDeck { deck_id, mode });
    }

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> (Option<DeleteDeckAnswer>, EventResult) {
        let (panel, l) = self.geometry(window);
        let (close, mut result) = self.frame.handle_event(event, panel);
        if close {
            return (Some(DeleteDeckAnswer::Cancelled), result);
        }

        if matches!(self.phase, Phase::Loading) {
            // Swallow all interaction until the deck detail arrives.
            return (None, result);
        }

        let free_r = self.free.handle_event(event, l.free);
        if free_r.clicked {
            self.free.checked = true;
            self.remove.checked = false;
        }
        result = result.merge(free_r);

        let remove_r = self.remove.handle_event(event, l.remove);
        if remove_r.clicked {
            self.remove.checked = true;
            self.free.checked = false;
        }
        result = result.merge(remove_r);

        let cancel_r = self.cancel.handle_event(event, l.cancel);
        if cancel_r.clicked {
            return (Some(DeleteDeckAnswer::Cancelled), EventResult::clicked());
        }
        result = result.merge(cancel_r);

        let delete_r = self.delete.handle_event(event, l.delete);
        if delete_r.clicked && !matches!(self.phase, Phase::Deleting) {
            self.confirm(ctx);
            result = result.merge(EventResult::clicked());
        } else {
            result = result.merge(delete_r);
        }

        if self.just_deleted() {
            return (Some(DeleteDeckAnswer::Deleted), result);
        }

        (None, result)
    }

    pub fn handle_edit_key(&mut self, key: EditKey, _ctx: &mut ScreenCtx) -> EventResult {
        if key == EditKey::Enter {
            _ctx.send(Command::DeleteDeck {
                deck_id: self.deck_id.unwrap_or(0),
                mode: if self.remove.checked {
                    DeleteMode::Remove
                } else {
                    DeleteMode::Free
                },
            });
            self.phase = Phase::Deleting;
            return EventResult::clicked();
        }
        EventResult::IGNORED
    }

    pub fn handle_escape(&mut self) -> bool {
        !matches!(self.phase, Phase::Deleting)
    }

    pub fn render(&mut self, c: &mut Compositor, layer: LayerId, window: Rect, theme: &Theme) {
        let (panel, l) = self.geometry(window);
        let title = format!("Excluir \"{}\"", self.deck_name);
        self.frame.render(c, layer, window, panel, &title, theme);

        let style = engine::theme::TypographyScale::hoff().base_2r();
        let text_color = theme.colors.text.0;
        let body = self.body_text();
        c.push(engine::compositor::SceneNode::Text {
            key: engine::compositor::TextNodeKey::from_style(&body, &style, Some(l.body.w)),
            x: l.body.x,
            y: l.body.y,
            color: text_color,
        });

        if matches!(self.phase, Phase::Loading) {
            text(c, "Carregando detalhes…", 12.0, 400, l.free.x, l.free.y, theme.colors.text_dim.0);
        } else {
            self.free.render(c, l.free, theme);
            text(
                c,
                "Você tem essas cartas fisicamente — o deck foi desmontado.",
                11.0,
                400,
                l.free.x + 28.0,
                l.free.y + l.free.h - 12.0,
                theme.glass.text_faint.0,
            );
            self.remove.render(c, l.remove, theme);
            text(
                c,
                "Era uma lista planejada; some as cópias da coleção junto com o deck.",
                11.0,
                400,
                l.remove.x + 28.0,
                l.remove.y + l.remove.h - 12.0,
                theme.glass.text_faint.0,
            );
        }

        if let Some(error) = &self.error {
            text(c, error, 12.0, 400, l.error.x, l.error.y, theme.colors.danger.0);
        }

        self.cancel.render(c, l.cancel, theme);
        self.delete.render(c, l.delete, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellbook_core::client::Command;
    use std::collections::HashMap;

    fn sample_deck() -> DeckSummary {
        DeckSummary {
            id: 3,
            name: "Esper Control".into(),
            commander_name: "Oloro, Ageless Ascetic".into(),
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
        }
    }

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    #[test]
    fn open_requests_deck_detail() {
        let theme = Theme::hoff();
        let mut modal = DeleteDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);

        let cmd = rx.try_recv().expect("expected GetDeck");
        assert!(matches!(cmd, Command::GetDeck { deck_id: 3 }));
    }

    #[test]
    fn confirm_sends_delete_deck_with_mode() {
        let theme = Theme::hoff();
        let mut modal = DeleteDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);
        modal.remove.checked = true;
        modal.phase = Phase::Confirming;

        modal.confirm(&mut ctx);
        // Drain the GetDeck the open path queued; only the DeleteDeck
        // belongs to confirm.
        let _ = rx.try_recv();
        let cmd = rx.try_recv().expect("expected DeleteDeck");
        match cmd {
            Command::DeleteDeck { deck_id, mode } => {
                assert_eq!(deck_id, 3);
                assert!(matches!(mode, DeleteMode::Remove));
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn deck_deleted_signals_deleted() {
        let theme = Theme::hoff();
        let mut modal = DeleteDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);
        modal.phase = Phase::Deleting;

        let changed = modal.on_event(&Event::DeckDeleted(Ok(5)), &mut ctx);
        assert!(changed);
        assert!(modal.just_deleted());
    }

    #[test]
    fn card_count_excludes_commanders() {
        let theme = Theme::hoff();
        let mut modal = DeleteDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);

        let mut by_type = HashMap::new();
        by_type.insert(
            "Comandante".into(),
            vec![spellbook_core::ops::decks::DeckCard {
                id: 1,
                card_name: "Oloro".into(),
                quantity: 1,
                oracle_id: None,
                mana_cost: None,
                type_line: "Legendary Creature".into(),
                image: None,
                image_back: None,
                cmc: None,
                price_usd: None,
                edhrec_rank: None,
                colors: None,
                color_identity: None,
                rarity: None,
                shared_with: vec![],
            }],
        );
        by_type.insert(
            "Creature".into(),
            vec![spellbook_core::ops::decks::DeckCard {
                id: 2,
                card_name: "Sol Ring".into(),
                quantity: 2,
                oracle_id: None,
                mana_cost: None,
                type_line: "Artifact".into(),
                image: None,
                image_back: None,
                cmc: None,
                price_usd: None,
                edhrec_rank: None,
                colors: None,
                color_identity: None,
                rarity: None,
                shared_with: vec![],
            }],
        );

        modal.detail = Some(Box::new(DeckDetail {
            id: 3,
            name: "Esper Control".into(),
            commander_name: "Oloro".into(),
            commander_name_2: None,
            philosophy: None,
            tags: None,
            created_at: None,
            total_cards: 3,
            is_valid_100: false,
            by_type,
            mana_curve: HashMap::new(),
            ownership: HashMap::new(),
        }));

        assert_eq!(modal.card_count(), 2);
    }
}

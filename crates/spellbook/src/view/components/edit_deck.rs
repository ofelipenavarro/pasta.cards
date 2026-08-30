//! "Editar deck" modal. Port of `openEditDeckModal` in `desktop/ui/js/views/decks.js`.
//!
//! Edits commander, optional partner commander, name, philosophy and tags.
//! Changing a commander reconciles the commander rows in `spellbook_core`.

use engine::compositor::{Compositor, LayerId, SceneNode, TextNodeKey};
use engine::theme::{Theme, TypographyScale};
use engine::ui::widgets::{Button, ButtonVariant, EventResult, Rect, WidgetEvent, rounded_rect};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::{DeckIn, DeckSummary};
use spellbook_core::types::Card;

use super::super::{EditKey, ScreenCtx, text, with_alpha};
use super::field::LabeledField;
use super::modal::{ModalFrame, PAD as MODAL_PAD};

const WIDTH: f32 = 480.0;
const BTN_H: f32 = 44.0;
const SUGGEST_H: f32 = 30.0;
const SUGGEST_ROWS: usize = 6;
const NAME_DEBOUNCE: f32 = 0.250;

/// What the modal tells the screen when it closes for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditDeckAnswer {
    /// The update succeeded; the owning screen should reload the grid.
    Saved,
    /// Dismissed without saving.
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Commander,
    Commander2,
    Name,
    Philosophy,
    Tags,
}

pub struct EditDeckModal {
    frame: ModalFrame,

    commander: LabeledField,
    commander2: LabeledField,
    name: LabeledField,
    philosophy: LabeledField,
    tags: LabeledField,

    save: Button,
    cancel: Button,

    deck_id: Option<i64>,
    suggestions: Vec<Card>,
    suggestions2: Vec<Card>,
    name_in_flight: bool,
    name_dirty: bool,
    since_name_edit: f32,
    name2_in_flight: bool,
    name2_dirty: bool,
    since_name2_edit: f32,

    focus: Option<Slot>,
    hover_suggest: Option<usize>,
    hover_suggest2: Option<usize>,

    error: Option<String>,
    saving: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Layout {
    commander: Rect,
    commander2: Rect,
    name: Rect,
    philosophy: Rect,
    tags: Rect,
    hint: Rect,
    error: Rect,
    save: Rect,
    cancel: Rect,
    height: f32,
}

impl EditDeckModal {
    pub fn new(theme: &Theme) -> Self {
        Self {
            frame: ModalFrame::new(),
            commander: LabeledField::new("Comandante *", "Nome (PT ou EN)…", theme),
            commander2: LabeledField::new(
                "Comandante parceiro (opcional)",
                "Deixe em branco para remover o parceiro",
                theme,
            ),
            name: LabeledField::new("Nome do deck *", "Nome do deck", theme),
            philosophy: LabeledField::new("Estratégia / filosofia", "Descreva a estratégia…", theme),
            tags: LabeledField::new("Tags (separadas por vírgula)", "Ex: Competitivo, budget", theme),
            save: Button::new("Salvar"),
            cancel: Button::new("Cancelar").variant(ButtonVariant::Outline),
            deck_id: None,
            suggestions: Vec::new(),
            suggestions2: Vec::new(),
            name_in_flight: false,
            name_dirty: false,
            since_name_edit: 0.0,
            name2_in_flight: false,
            name2_dirty: false,
            since_name2_edit: 0.0,
            focus: None,
            hover_suggest: None,
            hover_suggest2: None,
            error: None,
            saving: false,
        }
    }

    pub fn open(&mut self, deck: &DeckSummary, ctx: &mut ScreenCtx) {
        self.reset();
        self.deck_id = Some(deck.id);
        self.commander.set_value(&deck.commander_name);
        if let Some(c2) = &deck.commander_name_2 {
            self.commander2.set_value(c2);
        }
        self.name.set_value(&deck.name);
        if let Some(p) = &deck.philosophy {
            self.philosophy.set_value(p);
        }
        if let Some(t) = &deck.tags {
            self.tags.set_value(t);
        }
        self.set_focus(Some(Slot::Commander), ctx);
    }

    fn reset(&mut self) {
        let theme = Theme::hoff();
        *self = Self::new(&theme);
    }

    fn set_focus(&mut self, slot: Option<Slot>, _ctx: &mut ScreenCtx) {
        self.focus = slot;
        for s in [Slot::Commander, Slot::Commander2, Slot::Name, Slot::Philosophy, Slot::Tags] {
            let field = self.field_mut(s);
            if slot == Some(s) {
                field.focus();
            } else {
                field.unfocus();
            }
        }
    }

    fn field_mut(&mut self, slot: Slot) -> &mut LabeledField {
        match slot {
            Slot::Commander => &mut self.commander,
            Slot::Commander2 => &mut self.commander2,
            Slot::Name => &mut self.name,
            Slot::Philosophy => &mut self.philosophy,
            Slot::Tags => &mut self.tags,
        }
    }

    fn field_ref(&self, slot: Slot) -> &LabeledField {
        match slot {
            Slot::Commander => &self.commander,
            Slot::Commander2 => &self.commander2,
            Slot::Name => &self.name,
            Slot::Philosophy => &self.philosophy,
            Slot::Tags => &self.tags,
        }
    }

    fn active_suggestions(&self) -> (&Vec<Card>, Option<usize>) {
        match self.focus {
            Some(Slot::Commander2) => (&self.suggestions2, self.hover_suggest2),
            _ => (&self.suggestions, self.hover_suggest),
        }
    }

    fn active_suggestions_mut(&mut self) -> (&mut Vec<Card>, &mut Option<usize>, &mut bool) {
        match self.focus {
            Some(Slot::Commander2) => (
                &mut self.suggestions2,
                &mut self.hover_suggest2,
                &mut self.name2_in_flight,
            ),
            _ => (
                &mut self.suggestions,
                &mut self.hover_suggest,
                &mut self.name_in_flight,
            ),
        }
    }

    // -- Layout ---------------------------------------------------------------

    fn layout(&self, content: Rect) -> Layout {
        let mut l = Layout::default();
        let w = content.w;
        let mut y = content.y;

        l.commander = Rect::new(content.x, y, w, LabeledField::height());
        y += l.commander.h + 12.0;

        l.commander2 = Rect::new(content.x, y, w, LabeledField::height());
        y += l.commander2.h + 12.0;

        l.name = Rect::new(content.x, y, w, LabeledField::height());
        y += l.name.h + 12.0;

        l.philosophy = Rect::new(content.x, y, w, LabeledField::height());
        y += l.philosophy.h + 12.0;

        l.tags = Rect::new(content.x, y, w, LabeledField::height());
        y += l.tags.h + 12.0;

        l.hint = Rect::new(content.x, y, w, 28.0);
        y += l.hint.h + 8.0;

        if self.error.is_some() {
            l.error = Rect::new(content.x, y, w, 18.0);
            y += 24.0;
        }

        let (save_w, _) = self.save.preferred_size();
        let (cancel_w, _) = self.cancel.preferred_size();
        l.save = Rect::new(content.x + w - save_w.max(90.0), y, save_w.max(90.0), BTN_H);
        l.cancel = Rect::new(l.save.x - 8.0 - cancel_w.max(100.0), y, cancel_w.max(100.0), BTN_H);
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

    fn suggest_rect(&self, field: Rect) -> Rect {
        let (suggestions, _) = self.active_suggestions();
        let rows = suggestions.len().min(SUGGEST_ROWS) as f32;
        Rect::new(
            field.x,
            field.y + field.h + 4.0,
            field.w,
            rows * (SUGGEST_H + 4.0) + 8.0,
        )
    }

    // -- Data events ----------------------------------------------------------

    pub fn on_event(&mut self, event: &Event, _ctx: &mut ScreenCtx) -> bool {
        match event {
            Event::CardsFound(cards) => {
                let (suggestions, hover, in_flight) = self.active_suggestions_mut();
                if !*in_flight {
                    return false;
                }
                *in_flight = false;
                if !self.focus.is_some_and(|s| self.field_ref(s).is_focused()) {
                    return false;
                }
                *suggestions = cards.iter().take(SUGGEST_ROWS).cloned().collect();
                *hover = None;
                true
            }
            Event::DeckUpdated(result) => {
                if !self.saving {
                    return false;
                }
                self.saving = false;
                match result {
                    Ok(()) => {
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

    /// Returns true when the last update succeeded and the modal should close.
    pub fn just_saved(&self) -> bool {
        self.deck_id.is_some() && !self.saving && self.error.is_none()
    }

    // -- Save -----------------------------------------------------------------

    fn save(&mut self, ctx: &mut ScreenCtx) {
        if self.saving {
            return;
        }
        self.error = None;

        let commander_name = self.commander.value().trim().to_string();
        let name = self.name.value().trim().to_string();
        if commander_name.is_empty() || name.is_empty() {
            self.error = Some("Preencha comandante e nome do deck.".into());
            return;
        }

        let Some(deck_id) = self.deck_id else {
            self.error = Some("Nenhum deck selecionado.".into());
            return;
        };

        self.saving = true;
        ctx.send(Command::UpdateDeck {
            deck_id,
            patch: Box::new(DeckIn {
                name,
                commander_name,
                commander_name_2: self.commander2.value_opt(),
                philosophy: self.philosophy.value_opt(),
                tags: self.tags.value_opt(),
            }),
        });
    }

    // -- Pointer input --------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> (Option<EditDeckAnswer>, EventResult) {
        let (panel, l) = self.geometry(window);
        let (close, mut result) = self.frame.handle_event(event, panel);
        if close {
            return (Some(EditDeckAnswer::Cancelled), result);
        }

        if let Some(focus) = self.focus {
            if matches!(focus, Slot::Commander | Slot::Commander2) {
                result = result.merge(self.suggestion_pointer(event, &l, ctx));
            }
        }

        match *event {
            WidgetEvent::MouseDown { x, y } => {
                for (slot, rect) in [
                    (Slot::Commander, l.commander),
                    (Slot::Commander2, l.commander2),
                    (Slot::Name, l.name),
                    (Slot::Philosophy, l.philosophy),
                    (Slot::Tags, l.tags),
                ] {
                    let fr = self.field_mut(slot).field_rect(rect);
                    if fr.contains(x, y) {
                        self.set_focus(Some(slot), ctx);
                        self.field_mut(slot).click(x - fr.x);
                        return (None, EventResult::changed());
                    }
                }
            }
            _ => {}
        }

        let cancel_r = self.cancel.handle_event(event, l.cancel);
        if cancel_r.clicked {
            return (Some(EditDeckAnswer::Cancelled), EventResult::clicked());
        }
        result = result.merge(cancel_r);

        let save_r = self.save.handle_event(event, l.save);
        if save_r.clicked {
            self.save(ctx);
            result = result.merge(EventResult::clicked());
        } else {
            result = result.merge(save_r);
        }

        if self.just_saved() {
            return (Some(EditDeckAnswer::Saved), result);
        }

        (None, result)
    }

    fn suggestion_pointer(
        &mut self,
        event: &WidgetEvent,
        l: &Layout,
        _ctx: &mut ScreenCtx,
    ) -> EventResult {
        let slot = self.focus.unwrap();
        let field_rect = match slot {
            Slot::Commander => self.commander.field_rect(l.commander),
            Slot::Commander2 => self.commander2.field_rect(l.commander2),
            _ => return EventResult::IGNORED,
        };
        let suggest = self.suggest_rect(field_rect);
        let (suggestions, hover) = self.active_suggestions();

        match *event {
            WidgetEvent::MouseMove { x, y } => {
                let hovered = (!suggestions.is_empty())
                    .then(|| suggest.contains(x, y))
                    .unwrap_or(false)
                    .then(|| ((y - suggest.y - 4.0) / (SUGGEST_H + 4.0)).floor() as usize)
                    .filter(|i| *i < suggestions.len());
                if hovered != *hover {
                    *hover = hovered;
                    EventResult::changed()
                } else {
                    EventResult::IGNORED
                }
            }
            WidgetEvent::MouseDown { x, y }
                if !suggestions.is_empty() && suggest.contains(x, y) =>
            {
                let i = ((y - suggest.y - 4.0) / (SUGGEST_H + 4.0)).floor() as usize;
                if let Some(card) = suggestions.get(i).cloned() {
                    self.field_mut(slot).set_value(&card.name);
                    suggestions.clear();
                    return EventResult::clicked();
                }
                EventResult::IGNORED
            }
            _ => EventResult::IGNORED,
        }
    }

    // -- Text input -----------------------------------------------------------

    pub fn handle_text(&mut self, s: &str) -> bool {
        let slot = match self.focus {
            Some(s) => s,
            None => return false,
        };
        let changed = self.field_mut(slot).handle_text(s);
        if !changed {
            return false;
        }
        if matches!(slot, Slot::Commander | Slot::Commander2) {
            let (suggestions, _hover, in_flight) = self.active_suggestions_mut();
            suggestions.clear();
            *in_flight = false;
            match slot {
                Slot::Commander => {
                    self.name_dirty = true;
                    self.since_name_edit = 0.0;
                }
                Slot::Commander2 => {
                    self.name2_dirty = true;
                    self.since_name2_edit = 0.0;
                }
                _ => {}
            }
        }
        true
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> EventResult {
        match key {
            EditKey::Tab => {
                let order = [Slot::Commander, Slot::Commander2, Slot::Name, Slot::Philosophy, Slot::Tags];
                let next = match self.focus {
                    None => order.first().copied(),
                    Some(slot) => {
                        let i = order.iter().position(|s| *s == slot).unwrap_or(0);
                        order.get(i + 1).copied().or(Some(order[0]))
                    }
                };
                self.set_focus(next, ctx);
                EventResult::changed()
            }
            EditKey::Enter => {
                self.save(ctx);
                EventResult::clicked()
            }
            _ => {
                let slot = match self.focus {
                    Some(s) => s,
                    None => return EventResult::IGNORED,
                };
                let (consumed, changed) = self.field_mut(slot).handle_edit_key(key);
                if !consumed {
                    EventResult::IGNORED
                } else if changed {
                    EventResult::changed()
                } else {
                    EventResult {
                        handled: true,
                        ..EventResult::IGNORED
                    }
                }
            }
        }
    }

    pub fn handle_escape(&mut self) -> bool {
        let (suggestions, _hover, in_flight) = self.active_suggestions_mut();
        if !suggestions.is_empty() {
            suggestions.clear();
            *in_flight = false;
            return true;
        }
        false
    }

    pub fn tick(&mut self, dt: f32, _ctx: &mut ScreenCtx) -> bool {
        self.since_name_edit += dt;
        self.since_name2_edit += dt;

        if self.name_dirty && !self.name_in_flight && self.since_name_edit >= NAME_DEBOUNCE {
            self.name_dirty = false;
            let q = self.commander.value().trim().to_string();
            if q.len() >= 2 {
                self.name_in_flight = true;
                _ctx.send(Command::SearchCards { q, limit: 6 });
            }
        }
        if self.name2_dirty && !self.name2_in_flight && self.since_name2_edit >= NAME_DEBOUNCE {
            self.name2_dirty = false;
            let q = self.commander2.value().trim().to_string();
            if q.len() >= 2 {
                self.name2_in_flight = true;
                _ctx.send(Command::SearchCards { q, limit: 6 });
            }
        }

        self.focus.is_some()
    }

    // -- Render ---------------------------------------------------------------

    pub fn render(&mut self, c: &mut Compositor, layer: LayerId, window: Rect, theme: &Theme) {
        let (panel, l) = self.geometry(window);
        self.frame.render(c, layer, window, panel, "Editar deck", theme);

        self.commander.render(c, l.commander, theme);
        self.render_suggestions(c, l.commander, theme);
        self.commander2.render(c, l.commander2, theme);
        if self.focus == Some(Slot::Commander2) {
            self.render_suggestions(c, l.commander2, theme);
        }
        self.name.render(c, l.name, theme);
        self.philosophy.render(c, l.philosophy, theme);
        self.tags.render(c, l.tags, theme);

        text(
            c,
            "Trocar um comandante remove a carta antiga do deck e adiciona a nova como comandante.",
            11.0,
            400,
            l.hint.x,
            l.hint.y,
            theme.glass.text_faint.0,
        );

        if let Some(error) = &self.error {
            text(c, error, 12.0, 400, l.error.x, l.error.y, theme.colors.danger.0);
        }

        self.cancel.render(c, l.cancel, theme);
        self.save.render(c, l.save, theme);
    }

    fn render_suggestions(&self, c: &mut Compositor, field_block: Rect, theme: &Theme) {
        let slot = match self.focus {
            Some(s) if matches!(s, Slot::Commander | Slot::Commander2) => s,
            _ => return,
        };
        let field = self.field_ref(slot).field_rect(field_block);
        let suggest = self.suggest_rect(field);
        let (suggestions, hover) = self.active_suggestions();
        if suggestions.is_empty() {
            return;
        }

        let radius = theme.radius.lg;
        c.push(engine::ui::widgets::menu_shadow(suggest, radius));
        for node in engine::ui::widgets::glass_pill(
            suggest,
            radius,
            theme.glass.edge_soft.0,
            1.5,
            theme.glass.popover.0,
        ) {
            c.push(node);
        }

        let style = TypographyScale::hoff().base_2sm();
        for (i, card) in suggestions.iter().enumerate() {
            let row = Rect::new(
                suggest.x + 4.0,
                suggest.y + 4.0 + i as f32 * (SUGGEST_H + 4.0),
                suggest.w - 8.0,
                SUGGEST_H,
            );
            if hover == Some(i) {
                c.push(rounded_rect(
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    theme.radius.md,
                    theme.glass.surface_hover.0,
                ));
            }
            c.push(SceneNode::Text {
                key: TextNodeKey::from_style(&card.name, &style, Some(row.w - 20.0)),
                x: row.x + 10.0,
                y: row.y + (SUGGEST_H - style.line_height) / 2.0,
                color: with_alpha(theme.colors.text.0, theme.colors.text.0[3] * 0.8),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellbook_core::client::Command;

    fn sample_deck() -> DeckSummary {
        DeckSummary {
            id: 3,
            name: "Esper Control".into(),
            commander_name: "Oloro, Ageless Ascetic".into(),
            commander_name_2: None,
            philosophy: None,
            tags: Some("control".into()),
            created_at: None,
            total_cards: 100,
            wins: 0,
            losses: 0,
            commander_image: None,
            commander_image_2: None,
            color_identity: Some("WUB".into()),
        }
    }

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    #[test]
    fn open_loads_deck_fields() {
        let theme = Theme::hoff();
        let mut modal = EditDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);

        assert_eq!(modal.deck_id, Some(3));
        assert_eq!(modal.name.value(), "Esper Control");
        assert_eq!(modal.commander.value(), "Oloro, Ageless Ascetic");
        assert_eq!(modal.tags.value(), "control");
    }

    #[test]
    fn save_sends_update_deck() {
        let theme = Theme::hoff();
        let mut modal = EditDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);
        modal.name.set_value("Esper Stax");

        modal.save(&mut ctx);
        let cmd = rx.try_recv().expect("expected UpdateDeck");
        match cmd {
            Command::UpdateDeck { deck_id, patch } => {
                assert_eq!(deck_id, 3);
                assert_eq!(patch.name, "Esper Stax");
                assert_eq!(patch.commander_name, "Oloro, Ageless Ascetic");
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn deck_updated_signals_saved() {
        let theme = Theme::hoff();
        let mut modal = EditDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&sample_deck(), &mut ctx);
        modal.saving = true;

        let changed = modal.on_event(&Event::DeckUpdated(Ok(())), &mut ctx);
        assert!(changed);
        assert!(modal.just_saved());
    }
}

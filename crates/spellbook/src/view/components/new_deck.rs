//! "Novo deck" modal. Port of the first half of `desktop/ui/js/views/decks.js`.
//!
//! Fields for commander (with autocomplete), optional partner commander, deck
//! name, philosophy and tags, plus an optional auto-build step that fills the
//! 99 non-commander cards after the deck shell is created.

use engine::compositor::{Compositor, LayerId, SceneNode, TextNodeKey};
use engine::theme::{Intent, Theme, TypographyScale};
use engine::ui::widgets::{
    Button, ButtonVariant, Checkbox, EventResult, Rect, Select, WidgetEvent, rounded_rect,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::{AutoBuildStatus, BuildState, DeckIn};
use spellbook_core::types::Card;
use spellbook_core::wizard::AutoBuildIn;

use super::super::{EditKey, ScreenCtx, text, with_alpha};
use super::field::{FIELD_FONT, LabeledField};
use super::modal::{ModalFrame, PAD as MODAL_PAD};

const WIDTH: f32 = 480.0;
const BTN_H: f32 = 44.0;
const SUGGEST_H: f32 = 30.0;
const SUGGEST_ROWS: usize = 6;
const NAME_DEBOUNCE: f32 = 0.250;
const STATUS_POLL_INTERVAL: f32 = 0.2;

/// What the modal tells the screen when it closes for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewDeckAnswer {
    /// A deck was created (with or without auto-build). The screen reloads
    /// the grid and, for auto-build, navigates into the new deck.
    Created(i64),
    /// Dismissed without saving.
    Cancelled,
}

/// One slot of the focus chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Commander,
    Commander2,
    Name,
    Philosophy,
    Tags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BuildPhase {
    Idle,
    Creating,
    Building { deck_id: i64 },
    Polling { deck_id: i64 },
}

pub struct NewDeckModal {
    frame: ModalFrame,

    commander: LabeledField,
    commander2: LabeledField,
    name: LabeledField,
    philosophy: LabeledField,
    tags: LabeledField,

    auto_build: Checkbox,
    bracket: Select,
    mode: Select,

    save: Button,
    cancel: Button,

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
    build_phase: BuildPhase,
    status_poll_timer: f32,
    open: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Layout {
    commander: Rect,
    commander2: Rect,
    name: Rect,
    auto: Rect,
    bracket_label: Rect,
    bracket: Rect,
    mode_label: Rect,
    mode: Rect,
    mode_hint: Rect,
    philosophy: Rect,
    tags: Rect,
    status: Rect,
    error: Rect,
    save: Rect,
    cancel: Rect,
    height: f32,
}

impl NewDeckModal {
    pub fn new(theme: &Theme) -> Self {
        Self {
            frame: ModalFrame::new(),
            commander: LabeledField::new("Comandante *", "Nome (PT ou EN)…", theme),
            commander2: LabeledField::new(
                "Comandante parceiro (opcional)",
                "Ex: Partner, Background…",
                theme,
            ),
            name: LabeledField::new("Nome do deck *", "Ex: Syr Konrad Aristocratas", theme),
            philosophy: LabeledField::new(
                "Estratégia / filosofia (opcional)",
                "Descreva a estratégia do deck…",
                theme,
            ),
            tags: LabeledField::new(
                "Tags (opcional, separadas por vírgula)",
                "Ex: Competitivo, Orçamento baixo, cEDH",
                theme,
            ),
            auto_build: Checkbox::new(false).label("Montar automaticamente"),
            bracket: Select::new(
                ["1 — Casual", "2 — Baixo", "3 — Médio", "4 — Alto", "5 — cEDH"],
                2,
            ),
            mode: Select::new(["Sugerir as melhores", "Só o que tenho na coleção"], 0),
            save: Button::new("Criar deck"),
            cancel: Button::new("Cancelar").variant(ButtonVariant::Outline),
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
            build_phase: BuildPhase::Idle,
            status_poll_timer: 0.0,
        }
    }

    pub fn open(&mut self, ctx: &mut ScreenCtx) {
        self.reset();
        self.set_focus(Some(Slot::Commander), ctx);
    }

    fn reset(&mut self) {
        *self = Self::new(&Theme::hoff());
    }

    fn is_working(&self) -> bool {
        !matches!(self.build_phase, BuildPhase::Idle)
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

    fn focused_field(&self) -> Option<&LabeledField> {
        self.focus.map(|s| self.field_ref(s))
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
        y += l.name.h + 14.0;

        l.auto = Rect::new(content.x, y, w, 24.0);
        y += l.auto.h + 12.0;

        if self.auto_build.checked {
            l.bracket_label = Rect::new(content.x, y, w, 16.0);
            l.bracket = Rect::new(content.x, y + 18.0, w, BTN_H);
            y += BTN_H + 34.0;

            l.mode_label = Rect::new(content.x, y, w, 16.0);
            l.mode = Rect::new(content.x, y + 18.0, w, BTN_H);
            l.mode_hint = Rect::new(content.x, y + BTN_H + 22.0, w, 32.0);
            y += BTN_H + 58.0;
        }

        l.philosophy = Rect::new(content.x, y, w, LabeledField::height());
        y += l.philosophy.h + 12.0;

        l.tags = Rect::new(content.x, y, w, LabeledField::height());
        y += l.tags.h + 8.0;

        if self.is_working() {
            l.status = Rect::new(content.x, y, w, 18.0);
            y += 24.0;
        }

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

    fn status_text(&self) -> String {
        match &self.build_phase {
            BuildPhase::Idle => String::new(),
            BuildPhase::Creating => "Criando deck…".into(),
            BuildPhase::Building { .. } => "Montando as 99 cartas…".into(),
            BuildPhase::Polling { .. } => "Finalizando…".into(),
        }
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
                if !self.focused_field().is_some_and(|f| f.is_focused()) {
                    return false;
                }
                *suggestions = cards.iter().take(SUGGEST_ROWS).cloned().collect();
                *hover = None;
                true
            }
            Event::DeckCreated(result) => {
                if !matches!(self.build_phase, BuildPhase::Creating) {
                    return false;
                }
                match result {
                    Ok(id) => {
                        if self.auto_build.checked {
                            self.build_phase = BuildPhase::Building { deck_id: *id };
                            true
                        } else {
                            // The screen will take the Created answer on the
                            // next handle_event call and close the modal.
                            self.build_phase = BuildPhase::Idle;
                            true
                        }
                    }
                    Err(e) => {
                        self.error = Some(e.detail().to_string());
                        self.build_phase = BuildPhase::Idle;
                        true
                    }
                }
            }
            Event::AutoBuildFinished(result) => {
                if let BuildPhase::Building { deck_id } = self.build_phase {
                    match result {
                        Ok(()) => {
                            self.build_phase = BuildPhase::Polling { deck_id };
                            self.status_poll_timer = 0.0;
                            true
                        }
                        Err(e) => {
                            self.error = Some(e.detail().to_string());
                            self.build_phase = BuildPhase::Idle;
                            true
                        }
                    }
                } else {
                    false
                }
            }
            Event::AutoBuildStatus(status) => {
                if !matches!(self.build_phase, BuildPhase::Polling { .. }) {
                    return false;
                }
                self.handle_status(status)
            }
            _ => false,
        }
    }

    fn handle_status(&mut self, status: &AutoBuildStatus) -> bool {
        match status.state {
            BuildState::Done => {
                if let Some(result) = &status.result {
                    self.build_phase = BuildPhase::Idle;
                    // Keep the final id visible until the screen reacts.
                    self.build_phase = BuildPhase::Polling { deck_id: result.deck_id };
                }
                true
            }
            BuildState::Error => {
                self.error = status.error.clone().or(Some("Falha na montagem automática.".into()));
                self.build_phase = BuildPhase::Idle;
                true
            }
            BuildState::Idle => {
                // Still waiting for the worker to report progress.
                true
            }
        }
    }

    /// The id the auto-build finished with, if it is done.
    pub fn finished_deck_id(&self) -> Option<i64> {
        if let BuildPhase::Polling { deck_id } = self.build_phase {
            Some(deck_id)
        } else {
            None
        }
    }

    // -- Save -----------------------------------------------------------------

    fn save(&mut self, ctx: &mut ScreenCtx) {
        if self.is_working() {
            return;
        }
        self.error = None;

        let commander_name = self.commander.value().trim().to_string();
        let name = self.name.value().trim().to_string();
        if commander_name.is_empty() || name.is_empty() {
            self.error = Some("Preencha comandante e nome do deck.".into());
            self.set_focus(
                if commander_name.is_empty() {
                    Some(Slot::Commander)
                } else {
                    Some(Slot::Name)
                },
                ctx,
            );
            return;
        }

        let commander_name_2 = self.commander2.value_opt();
        let philosophy = self.philosophy.value_opt();
        let tags = self.tags.value_opt();

        self.build_phase = BuildPhase::Creating;
        ctx.send(Command::CreateDeck(Box::new(DeckIn {
            name,
            commander_name,
            commander_name_2,
            philosophy,
            tags,
        })));
    }

    fn send_autobuild(&mut self, deck_id: i64, ctx: &mut ScreenCtx) {
        let bracket = (self.bracket.selected as i64) + 1;
        let mode = if self.mode.selected == 0 {
            Some("suggest".into())
        } else {
            Some("owned".into())
        };
        ctx.send(Command::AutoBuild(Box::new(AutoBuildIn {
            name: self.name.value().trim().to_string(),
            commander_name: self.commander.value().trim().to_string(),
            bracket,
            philosophy: self.philosophy.value_opt(),
            mode,
        })));
        self.build_phase = BuildPhase::Building { deck_id };
    }

    // -- Pointer input --------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> (Option<NewDeckAnswer>, EventResult) {
        let (panel, l) = self.geometry(window);
        let (close, mut result) = self.frame.handle_event(event, panel);
        if close {
            return (Some(NewDeckAnswer::Cancelled), result);
        }

        // Open selects eat the event before the form does.
        if self.bracket.is_open() {
            return (None, result.merge(self.bracket.handle_event(event, l.bracket)));
        }
        if self.mode.is_open() {
            return (None, result.merge(self.mode.handle_event(event, l.mode)));
        }

        // Suggestion list for the focused commander field.
        if let Some(focus) = self.focus {
            if matches!(focus, Slot::Commander | Slot::Commander2) {
                result = result.merge(self.suggestion_pointer(event, &l, ctx));
            }
        }

        match *event {
            WidgetEvent::MouseDown { x, y } => {
                // Field clicks.
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

                // Auto-build checkbox.
                if l.auto.contains(x, y) {
                    self.auto_build.checked = !self.auto_build.checked;
                    self.save.label = if self.auto_build.checked {
                        "Montar deck"
                    } else {
                        "Criar deck"
                    };
                    return (None, EventResult::clicked());
                }

                // Selects (only when auto-build is on).
                if self.auto_build.checked {
                    if l.bracket.contains(x, y) {
                        self.set_focus(None, ctx);
                        return (None, result.merge(self.bracket.handle_event(event, l.bracket)));
                    }
                    if l.mode.contains(x, y) {
                        self.set_focus(None, ctx);
                        return (None, result.merge(self.mode.handle_event(event, l.mode)));
                    }
                }
            }
            _ => {}
        }

        // Buttons.
        let cancel_r = self.cancel.handle_event(event, l.cancel);
        if cancel_r.clicked {
            return (Some(NewDeckAnswer::Cancelled), EventResult::clicked());
        }
        result = result.merge(cancel_r);

        let save_r = self.save.handle_event(event, l.save);
        if save_r.clicked {
            self.save(ctx);
            result = result.merge(EventResult::clicked());
        } else {
            result = result.merge(save_r);
        }

        // If the build just finished, report it now so the screen can navigate.
        if let Some(id) = self.finished_deck_id() {
            return (Some(NewDeckAnswer::Created(id)), result);
        }

        (None, result)
    }

    fn suggestion_pointer(
        &mut self,
        event: &WidgetEvent,
        l: &Layout,
        ctx: &mut ScreenCtx,
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
                    if slot == Slot::Commander && self.name.is_empty() {
                        self.name.set_value(&card.name);
                    }
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

    /// Escape unwinds one layer at a time.
    pub fn handle_escape(&mut self) -> bool {
        let (suggestions, _hover, in_flight) = self.active_suggestions_mut();
        if !suggestions.is_empty() {
            suggestions.clear();
            *in_flight = false;
            return true;
        }
        if self.bracket.is_open() {
            self.bracket.close();
            return true;
        }
        if self.mode.is_open() {
            self.mode.close();
            return true;
        }
        false
    }

    /// Blink + debounced searches + status polling.
    pub fn tick(&mut self, dt: f32, ctx: &mut ScreenCtx) -> bool {
        self.since_name_edit += dt;
        self.since_name2_edit += dt;

        if self.name_dirty && !self.name_in_flight && self.since_name_edit >= NAME_DEBOUNCE {
            self.name_dirty = false;
            let q = self.commander.value().trim().to_string();
            if q.len() >= 2 {
                self.name_in_flight = true;
                ctx.send(Command::SearchCards { q, limit: 6 });
            }
        }
        if self.name2_dirty && !self.name2_in_flight && self.since_name2_edit >= NAME_DEBOUNCE {
            self.name2_dirty = false;
            let q = self.commander2.value().trim().to_string();
            if q.len() >= 2 {
                self.name2_in_flight = true;
                ctx.send(Command::SearchCards { q, limit: 6 });
            }
        }

        // The build command is synchronous, but the status event may arrive
        // before we ask for it. Poll until we see Done/Error.
        if let BuildPhase::Polling { .. } = self.build_phase {
            self.status_poll_timer -= dt;
            if self.status_poll_timer <= 0.0 {
                self.status_poll_timer = STATUS_POLL_INTERVAL;
                ctx.send(Command::AutoBuildStatus);
            }
        }

        // If CreateDeck succeeded and auto-build is on, send AutoBuild now.
        if let BuildPhase::Building { deck_id } = self.build_phase {
            self.send_autobuild(deck_id, ctx);
        }

        self.focus.is_some()
    }

    // -- Render ---------------------------------------------------------------

    pub fn render(&mut self, c: &mut Compositor, layer: LayerId, window: Rect, theme: &Theme) {
        let (panel, l) = self.geometry(window);
        self.frame.render(c, layer, window, panel, "Novo deck", theme);

        self.commander.render(c, l.commander, theme);
        self.render_suggestions(c, l.commander, theme);
        self.commander2.render(c, l.commander2, theme);
        if self.focus == Some(Slot::Commander2) {
            self.render_suggestions(c, l.commander2, theme);
        }
        self.name.render(c, l.name, theme);

        self.auto_build.render(c, l.auto, theme);

        if self.auto_build.checked {
            text(
                c,
                "Bracket alvo",
                12.0,
                400,
                l.bracket_label.x,
                l.bracket_label.y,
                theme.colors.text_dim.0,
            );
            self.bracket.render(c, l.bracket, theme);
            text(
                c,
                "Quais cartas usar",
                12.0,
                400,
                l.mode_label.x,
                l.mode_label.y,
                theme.colors.text_dim.0,
            );
            self.mode.render(c, l.mode, theme);
            let hint = if self.mode.selected == 0 {
                "Monta o melhor deck possível, mesmo com cartas que você não tem."
            } else {
                "Deck montável hoje com cartas da sua coleção."
            };
            text(c, hint, 11.0, 400, l.mode_hint.x, l.mode_hint.y, theme.glass.text_placeholder.0);
        }

        self.philosophy.render(c, l.philosophy, theme);
        self.tags.render(c, l.tags, theme);

        if self.is_working() {
            text(
                c,
                &self.status_text(),
                12.0,
                400,
                l.status.x,
                l.status.y,
                theme.colors.text_dim.0,
            );
        }

        if let Some(error) = &self.error {
            text(c, error, 12.0, 400, l.error.x, l.error.y, theme.colors.danger.0);
        }

        self.cancel.render(c, l.cancel, theme);
        self.save.render(c, l.save, theme);

        // Dropdown overlays above everything.
        self.bracket.render_dropdown(c, layer, l.bracket, theme);
        self.mode.render_dropdown(c, layer, l.mode, theme);
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

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    #[test]
    fn opens_with_focus_on_commander() {
        let theme = Theme::hoff();
        let mut modal = NewDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&mut ctx);
        assert!(modal.commander.is_focused());
    }

    #[test]
    fn validation_requires_commander_and_name() {
        let theme = Theme::hoff();
        let mut modal = NewDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&mut ctx);

        modal.save(&mut ctx);
        assert!(modal.error.is_some());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn save_sends_create_deck_without_autobuild() {
        let theme = Theme::hoff();
        let mut modal = NewDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&mut ctx);
        modal.commander.set_value("Kess, Dissident Mage");
        modal.name.set_value("Kess Spells");
        modal.tags.set_value("combo, budget");

        modal.save(&mut ctx);
        let cmd = rx.try_recv().expect("expected CreateDeck");
        match cmd {
            Command::CreateDeck(deck_in) => {
                assert_eq!(deck_in.commander_name, "Kess, Dissident Mage");
                assert_eq!(deck_in.name, "Kess Spells");
                assert_eq!(deck_in.tags, Some("combo, budget".into()));
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn deck_created_closes_modal_when_not_autobuilding() {
        let theme = Theme::hoff();
        let mut modal = NewDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(&mut ctx);
        modal.build_phase = BuildPhase::Creating;

        let changed = modal.on_event(&Event::DeckCreated(Ok(7)), &mut ctx);
        assert!(changed);
        assert!(matches!(modal.build_phase, BuildPhase::Idle));
    }

    #[test]
    fn deck_created_starts_autobuild_when_toggle_on() {
        let theme = Theme::hoff();
        let mut modal = NewDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(&mut ctx);
        modal.auto_build.checked = true;
        modal.build_phase = BuildPhase::Creating;

        let changed = modal.on_event(&Event::DeckCreated(Ok(7)), &mut ctx);
        assert!(changed);
        assert!(matches!(modal.build_phase, BuildPhase::Building { deck_id: 7 }));

        // tick sends the AutoBuild command.
        modal.tick(0.1, &mut ctx);
        let cmd = rx.try_recv().expect("expected AutoBuild");
        assert!(matches!(cmd, Command::AutoBuild(_)));
    }
}

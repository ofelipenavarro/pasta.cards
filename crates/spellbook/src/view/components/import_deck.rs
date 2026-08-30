//! "Importar decklist" modal. Port of `openImportDeckModal` in
//! `desktop/ui/js/views/decks.js`.
//!
//! Accepts pasted text, runs `ImportPreview`, shows matched/not-found rows,
//! and commits the import in either merge or replace mode.

use engine::compositor::{Compositor, LayerId, SceneNode, TextNodeKey};
use engine::theme::{Theme, TypographyScale};
use engine::ui::widgets::{
    Button, ButtonVariant, Chip, EventResult, Rect, WidgetEvent, rounded_rect,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::{ImportCard, ImportMode, ImportPreview};

use super::super::{EditKey, ScreenCtx, text};
use super::field::{FIELD_FONT, LabeledField};
use super::modal::{ModalFrame, PAD as MODAL_PAD};

const WIDTH: f32 = 560.0;
const BTN_H: f32 = 44.0;
const LIST_H: f32 = FIELD_FONT * 1.4 * 6.0 + 16.0;
const ROW_H: f32 = 28.0;

/// What the modal tells the screen when it closes for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportDeckAnswer {
    /// The import was committed; the screen should reload the deck.
    Imported,
    /// Dismissed without importing.
    Cancelled,
}

pub struct ImportDeckModal {
    frame: ModalFrame,
    deck_id: Option<i64>,

    list: LabeledField,
    preview: Option<Box<ImportPreview>>,
    preview_in_flight: bool,
    import_in_flight: bool,

    merge_chip: Chip,
    replace_chip: Chip,

    preview_btn: Button,
    confirm_btn: Button,
    cancel_btn: Button,

    error: Option<String>,
    focus: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Layout {
    hint: Rect,
    list: Rect,
    results: Rect,
    mode_label: Rect,
    merge_chip: Rect,
    replace_chip: Rect,
    error: Rect,
    preview: Rect,
    confirm: Rect,
    cancel: Rect,
    height: f32,
}

impl ImportDeckModal {
    pub fn new(theme: &Theme) -> Self {
        Self {
            frame: ModalFrame::new(),
            deck_id: None,
            list: LabeledField::new(
                "Cole a decklist",
                "1 Sol Ring\n1 Arcane Signet\n1 Command Tower",
                theme,
            ),
            preview: None,
            preview_in_flight: false,
            import_in_flight: false,
            merge_chip: Chip::new("Somar à lista atual").selected(true).interactive(true),
            replace_chip: Chip::new("Trocar o deck inteiro").interactive(true),
            preview_btn: Button::new("Importar"),
            confirm_btn: Button::new("Confirmar importação"),
            cancel_btn: Button::new("Cancelar").variant(ButtonVariant::Outline),
            error: None,
            focus: false,
        }
    }

    pub fn open(&mut self, deck_id: i64, _ctx: &mut ScreenCtx) {
        self.deck_id = Some(deck_id);
        self.list.set_value("");
        self.preview = None;
        self.preview_in_flight = false;
        self.import_in_flight = false;
        self.merge_chip.selected = true;
        self.replace_chip.selected = false;
        self.error = None;
        self.focus = true;
        self.list.focus();
    }

    fn reset_preview(&mut self) {
        self.preview = None;
        self.error = None;
    }

    fn mode(&self) -> ImportMode {
        if self.replace_chip.selected {
            ImportMode::Replace
        } else {
            ImportMode::Merge
        }
    }

    fn has_matches(&self) -> bool {
        self.preview.as_ref().is_some_and(|p| !p.matched.is_empty())
    }

    // -- Layout ---------------------------------------------------------------

    fn layout(&self, content: Rect) -> Layout {
        let mut l = Layout::default();
        let w = content.w;
        let mut y = content.y;

        l.hint = Rect::new(content.x, y, w, 40.0);
        y += l.hint.h + 8.0;

        l.list = Rect::new(content.x, y, w, LIST_H);
        y += l.list.h + 14.0;

        let results_h = if self.preview.is_some() {
            let rows = self
                .preview
                .as_ref()
                .map(|p| p.matched.len() + p.not_found.len())
                .unwrap_or(0)
                .max(2);
            (rows as f32 * ROW_H + 40.0).min(240.0)
        } else {
            0.0
        };
        if results_h > 0.0 {
            l.results = Rect::new(content.x, y, w, results_h);
            y += results_h + 16.0;
        }

        if self.has_matches() {
            l.mode_label = Rect::new(content.x, y, w, 16.0);
            y += 22.0;
            l.merge_chip = Rect::new(content.x, y, 180.0, 32.0);
            l.replace_chip = Rect::new(content.x + 196.0, y, 180.0, 32.0);
            y += 44.0;
        }

        if self.error.is_some() {
            l.error = Rect::new(content.x, y, w, 18.0);
            y += 24.0;
        }

        let (cancel_w, _) = self.cancel_btn.preferred_size();
        let (preview_w, _) = self.preview_btn.preferred_size();
        let (confirm_w, _) = self.confirm_btn.preferred_size();

        l.cancel = Rect::new(
            content.x + w - cancel_w.max(100.0),
            y,
            cancel_w.max(100.0),
            BTN_H,
        );
        if self.has_matches() {
            l.confirm = Rect::new(
                l.cancel.x - 8.0 - confirm_w.max(160.0),
                y,
                confirm_w.max(160.0),
                BTN_H,
            );
        } else {
            l.preview = Rect::new(
                l.cancel.x - 8.0 - preview_w.max(100.0),
                y,
                preview_w.max(100.0),
                BTN_H,
            );
        }
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

    // -- Data events ----------------------------------------------------------

    pub fn on_event(&mut self, event: &Event, _ctx: &mut ScreenCtx) -> bool {
        match event {
            Event::ImportPreviewed { deck_id, result } => {
                if self.deck_id != Some(*deck_id) {
                    return false;
                }
                self.preview_in_flight = false;
                match result {
                    Ok(preview) => {
                        self.preview = Some(preview.clone());
                        self.error = None;
                        true
                    }
                    Err(e) => {
                        self.preview = None;
                        self.error = Some(e.detail().to_string());
                        true
                    }
                }
            }
            Event::ImportCommitted { deck_id, result } => {
                if self.deck_id != Some(*deck_id) || !self.import_in_flight {
                    return false;
                }
                self.import_in_flight = false;
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

    pub fn just_imported(&self) -> bool {
        self.deck_id.is_some()
            && !self.import_in_flight
            && self.error.is_none()
            && self.preview.is_some()
    }

    // -- Actions --------------------------------------------------------------

    fn run_preview(&mut self, ctx: &mut ScreenCtx) {
        let Some(deck_id) = self.deck_id else {
            self.error = Some("Nenhum deck selecionado.".into());
            return;
        };
        let text = self.list.value().trim().to_string();
        if text.is_empty() {
            self.error = Some("Cole uma decklist primeiro.".into());
            return;
        }
        self.preview_in_flight = true;
        self.error = None;
        ctx.send(Command::ImportPreview { deck_id, text });
    }

    fn commit(&mut self, ctx: &mut ScreenCtx) {
        let Some(deck_id) = self.deck_id else {
            return;
        };
        let Some(preview) = &self.preview else {
            return;
        };
        let cards: Vec<ImportCard> = preview
            .matched
            .iter()
            .map(|m| ImportCard {
                card_name: m.name.clone(),
                quantity: m.quantity,
            })
            .collect();
        if cards.is_empty() {
            return;
        }
        self.import_in_flight = true;
        ctx.send(Command::ImportCommit {
            deck_id,
            cards,
            mode: self.mode(),
        });
    }

    // -- Pointer input --------------------------------------------------------

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
        ctx: &mut ScreenCtx,
    ) -> (Option<ImportDeckAnswer>, EventResult) {
        let (panel, l) = self.geometry(window);
        let (close, mut result) = self.frame.handle_event(event, panel);
        if close {
            return (Some(ImportDeckAnswer::Cancelled), result);
        }

        match *event {
            WidgetEvent::MouseDown { x, y } => {
                let fr = self.list.field_rect(l.list);
                if fr.contains(x, y) {
                    self.focus = true;
                    self.list.click(x - fr.x);
                    return (None, EventResult::changed());
                }
                if !self.import_in_flight {
                    if l.merge_chip.contains(x, y) {
                        self.merge_chip.selected = true;
                        self.replace_chip.selected = false;
                        return (None, EventResult::clicked());
                    }
                    if l.replace_chip.contains(x, y) {
                        self.replace_chip.selected = true;
                        self.merge_chip.selected = false;
                        return (None, EventResult::clicked());
                    }
                }
            }
            _ => {}
        }

        let cancel_r = self.cancel_btn.handle_event(event, l.cancel);
        if cancel_r.clicked {
            return (Some(ImportDeckAnswer::Cancelled), EventResult::clicked());
        }
        result = result.merge(cancel_r);

        if self.has_matches() {
            let confirm_r = self.confirm_btn.handle_event(event, l.confirm);
            if confirm_r.clicked && !self.import_in_flight {
                self.commit(ctx);
                result = result.merge(EventResult::clicked());
            } else {
                result = result.merge(confirm_r);
            }
        } else {
            let preview_r = self.preview_btn.handle_event(event, l.preview);
            if preview_r.clicked && !self.preview_in_flight {
                self.run_preview(ctx);
                result = result.merge(EventResult::clicked());
            } else {
                result = result.merge(preview_r);
            }
        }

        if self.just_imported() {
            return (Some(ImportDeckAnswer::Imported), result);
        }

        (None, result)
    }

    // -- Text input -----------------------------------------------------------

    pub fn handle_text(&mut self, s: &str) -> bool {
        if !self.focus {
            return false;
        }
        let changed = self.list.handle_text(s);
        if changed {
            self.reset_preview();
        }
        changed
    }

    pub fn handle_edit_key(&mut self, key: EditKey, _ctx: &mut ScreenCtx) -> EventResult {
        if !self.focus {
            return EventResult::IGNORED;
        }
        let (consumed, changed) = self.list.handle_edit_key(key);
        if changed {
            self.reset_preview();
        }
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

    pub fn handle_escape(&mut self) -> bool {
        self.focus = false;
        self.list.unfocus();
        false
    }

    pub fn tick(&mut self, _dt: f32, _ctx: &mut ScreenCtx) -> bool {
        false
    }

    // -- Render ---------------------------------------------------------------

    pub fn render(&mut self, c: &mut Compositor, layer: LayerId, window: Rect, theme: &Theme) {
        let (panel, l) = self.geometry(window);
        self.frame.render(c, layer, window, panel, "Importar decklist", theme);

        let style = TypographyScale::hoff().base_2r();
        let hint = "Cole uma lista (Moxfield, Archidekt ou texto simples). Nada é adicionado até você confirmar.";
        c.push(SceneNode::Text {
            key: TextNodeKey::from_style(hint, &style, Some(l.hint.w)),
            x: l.hint.x,
            y: l.hint.y,
            color: theme.colors.text_dim.0,
        });

        self.render_list_area(c, l.list, theme);

        if let Some(preview) = &self.preview {
            self.render_preview(c, l.results, preview, theme);
        }

        if self.has_matches() {
            text(
                c,
                "O deck já tem cartas. O que fazer?",
                12.0,
                400,
                l.mode_label.x,
                l.mode_label.y,
                theme.colors.text_dim.0,
            );
            self.merge_chip.render(c, l.merge_chip, theme);
            self.replace_chip.render(c, l.replace_chip, theme);
        }

        if let Some(error) = &self.error {
            text(c, error, 12.0, 400, l.error.x, l.error.y, theme.colors.danger.0);
        }

        self.cancel_btn.render(c, l.cancel, theme);
        if self.has_matches() {
            self.confirm_btn.render(c, l.confirm, theme);
        } else {
            self.preview_btn.render(c, l.preview, theme);
        }
    }

    fn render_list_area(&self, c: &mut Compositor, rect: Rect, theme: &Theme) {
        if self.focus {
            c.push(engine::ui::widgets::focus_ring(rect, theme.radius.sm, theme));
        }
        c.push(rounded_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            theme.radius.sm,
            theme.glass.field.0,
        ));
        let style = TypographyScale::hoff().base_2r();
        let value = self.list.value();
        let shown = if value.is_empty() && !self.focus {
            self.list.input.placeholder.as_str()
        } else {
            value
        };
        let color = if value.is_empty() && !self.focus {
            self.list.input.placeholder_color
        } else {
            self.list.input.text_color
        };
        c.push(SceneNode::Text {
            key: TextNodeKey::from_style(shown, &style, Some(rect.w - 16.0)),
            x: rect.x + 8.0,
            y: rect.y + 8.0,
            color,
        });
    }

    fn render_preview(&self, c: &mut Compositor, rect: Rect, preview: &ImportPreview, theme: &Theme) {
        c.push(rounded_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            theme.radius.md,
            theme.glass.surface.0,
        ));
        c.push(engine::ui::widgets::rounded_rect_stroke(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            theme.radius.md,
            theme.glass.edge_soft.0,
            1.0,
        ));

        let header = format!(
            "{} reconhecida(s) de {} linha(s)",
            preview.matched.len(),
            preview.total_lines
        );
        text(c, &header, 12.0, 600, rect.x + 12.0, rect.y + 10.0, theme.colors.text.0);

        let mut y = rect.y + 32.0;
        let style = TypographyScale::hoff().base_2sm();
        for m in &preview.matched {
            if y + ROW_H > rect.y + rect.h {
                break;
            }
            let label = format!("{}x {}", m.quantity, m.name);
            c.push(SceneNode::Text {
                key: TextNodeKey::from_style(&label, &style, Some(rect.w - 24.0)),
                x: rect.x + 12.0,
                y,
                color: theme.colors.text.0,
            });
            y += ROW_H;
        }
        for miss in &preview.not_found {
            if y + ROW_H > rect.y + rect.h {
                break;
            }
            let label = format!("{}x {}", miss.quantity, miss.requested_name);
            c.push(SceneNode::Text {
                key: TextNodeKey::from_style(&label, &style, Some(rect.w - 24.0)),
                x: rect.x + 12.0,
                y,
                color: theme.colors.danger.0,
            });
            y += ROW_H;
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
    fn run_preview_sends_import_preview() {
        let theme = Theme::hoff();
        let mut modal = ImportDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(5, &mut ctx);
        modal.list.set_value("1 Sol Ring\n1 Arcane Signet");

        modal.run_preview(&mut ctx);
        let cmd = rx.try_recv().expect("expected ImportPreview");
        match cmd {
            Command::ImportPreview { deck_id, text } => {
                assert_eq!(deck_id, 5);
                assert!(text.contains("Sol Ring"));
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn import_previewed_stores_preview() {
        let theme = Theme::hoff();
        let mut modal = ImportDeckModal::new(&theme);
        let (mut ctx, _rx) = test_ctx();
        modal.open(5, &mut ctx);

        let preview = ImportPreview {
            matched: vec![spellbook_core::ops::decks::ImportMatch {
                name: "Sol Ring".into(),
                quantity: 1,
                requested_name: "Sol Ring".into(),
                match_type: "exata".into(),
                mana_cost: None,
                type_line: "Artifact".into(),
                image: None,
                image_back: None,
            }],
            not_found: vec![],
            total_lines: 1,
        };
        let changed = modal.on_event(
            &Event::ImportPreviewed {
                deck_id: 5,
                result: Ok(Box::new(preview)),
            },
            &mut ctx,
        );
        assert!(changed);
        assert!(modal.has_matches());
    }

    #[test]
    fn commit_sends_import_commit_with_mode() {
        let theme = Theme::hoff();
        let mut modal = ImportDeckModal::new(&theme);
        let (mut ctx, rx) = test_ctx();
        modal.open(5, &mut ctx);
        modal.replace_chip.selected = true;

        let preview = ImportPreview {
            matched: vec![spellbook_core::ops::decks::ImportMatch {
                name: "Sol Ring".into(),
                quantity: 2,
                requested_name: "Sol Ring".into(),
                match_type: "exata".into(),
                mana_cost: None,
                type_line: "Artifact".into(),
                image: None,
                image_back: None,
            }],
            not_found: vec![],
            total_lines: 1,
        };
        modal.preview = Some(Box::new(preview));

        modal.commit(&mut ctx);
        let cmd = rx.try_recv().expect("expected ImportCommit");
        match cmd {
            Command::ImportCommit { deck_id, cards, mode } => {
                assert_eq!(deck_id, 5);
                assert_eq!(cards.len(), 1);
                assert_eq!(cards[0].quantity, 2);
                assert!(matches!(mode, ImportMode::Replace));
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }
}

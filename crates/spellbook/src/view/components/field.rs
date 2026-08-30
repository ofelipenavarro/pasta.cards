//! Labeled single-line text field wrapping `engine::text_input::TextInput`.
//!
//! Port of the web's labeled inputs. The label sits above the field; the
//! field itself is a `TextInput` with HOFF glass styling. Used directly by
//! [`AddCardModal`](super::add_card::AddCardModal) and
//! [`CardModal`](super::card_modal::CardModal).

use engine::compositor::Compositor;
use engine::text_input::TextInput;
use engine::theme::Theme;
use engine::ui::widgets::{EventResult, Rect, WidgetEvent, focus_ring, rounded_rect};

use super::super::text;

use super::EditKey;

/// Layout constants shared with the modal layouts.
pub const FIELD_FONT: f32 = 16.0;
pub const FIELD_H: f32 = FIELD_FONT * 2.0;
const LABEL_H: f32 = 20.0;
const TEXT_PAD: f32 = 8.0;

/// A labeled text field.
pub struct LabeledField {
    label: String,
    pub input: TextInput,
}

impl LabeledField {
    pub fn new(label: impl Into<String>, placeholder: impl Into<String>, theme: &Theme) -> Self {
        let accent = theme.colors.accent.0;
        let mut input = TextInput::new()
            .with_placeholder(&placeholder.into())
            .with_font_size(FIELD_FONT)
            .with_text_color(theme.colors.text.0)
            .with_bg_color(theme.glass.field.0);
        input.placeholder_color = theme.glass.text_placeholder.0;
        input.cursor_color = accent;
        input.selection_color = [accent[0], accent[1], accent[2], 0.25];
        Self {
            label: label.into(),
            input,
        }
    }

    /// Total height of the label + field block.
    pub fn height() -> f32 {
        LABEL_H + FIELD_H
    }

    /// The field rect inside a layout rect allocated for this block.
    pub fn field_rect(&self, block: Rect) -> Rect {
        Rect::new(block.x, block.y + LABEL_H, block.w, FIELD_H)
    }

    pub fn value(&self) -> &str {
        self.input.buffer.text()
    }

    pub fn value_opt(&self) -> Option<String> {
        let v = self.value().trim();
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    }

    pub fn set_value(&mut self, value: &str) {
        self.input.buffer.set_text(value);
    }

    pub fn is_empty(&self) -> bool {
        self.input.buffer.is_empty()
    }

    pub fn focus(&mut self) {
        self.input.focus();
    }

    pub fn unfocus(&mut self) {
        self.input.unfocus();
    }

    pub fn set_focused(&mut self, focused: bool) {
        if focused {
            self.input.focus();
        } else {
            self.input.unfocus();
        }
    }

    pub fn is_focused(&self) -> bool {
        self.input.focused
    }

    pub fn click(&mut self, local_x: f32) {
        self.input.focus();
        self.input.handle_click(local_x - TEXT_PAD);
    }

    pub fn handle_text(&mut self, s: &str) -> bool {
        if !self.input.focused {
            return false;
        }
        for c in s.chars() {
            self.input.handle_char(c);
        }
        true
    }

    /// Returns `(consumed, changed)`.
    pub fn handle_edit_key(&mut self, key: EditKey) -> (bool, bool) {
        if !self.input.focused {
            return (false, false);
        }
        match key {
            EditKey::Backspace => self.input.handle_backspace(),
            EditKey::Delete => self.input.handle_delete(),
            EditKey::Left => self.input.handle_left(),
            EditKey::Right => self.input.handle_right(),
            EditKey::Home => self.input.handle_home(),
            EditKey::End => self.input.handle_end(),
            EditKey::Up | EditKey::Down | EditKey::Tab | EditKey::Enter => return (false, false),
        };
        (true, true)
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if self.input.focused {
            self.input.tick(dt);
            true
        } else {
            false
        }
    }

    pub fn render(&self, c: &mut Compositor, block: Rect, theme: &Theme) {
        text(
            c,
            &self.label,
            12.0,
            600,
            block.x,
            block.y,
            theme.glass.text_placeholder.0,
        );
        let field = self.field_rect(block);
        if self.input.focused {
            c.push(focus_ring(field, theme.radius.sm, theme));
        }
        c.push(rounded_rect(
            field.x,
            field.y,
            field.w,
            field.h,
            theme.radius.sm,
            self.input.bg_color,
        ));
        for node in self.input.build_scene(field.x, field.y, field.w) {
            c.push(node);
        }
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, block: Rect) -> EventResult {
        let field = self.field_rect(block);
        match *event {
            WidgetEvent::MouseDown { x, y } => {
                if field.contains(x, y) {
                    self.click(x - field.x);
                    EventResult::changed()
                } else {
                    EventResult::IGNORED
                }
            }
            _ => EventResult::IGNORED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_creation() {
        let theme = engine::theme::Theme::hoff();
        let field = LabeledField::new("Nome", "Digite o nome...", &theme);
        assert_eq!(field.value(), "");
        assert!(!field.input.focused);
    }
}

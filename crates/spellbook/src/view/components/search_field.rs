//! Search input with a clear ("x") button and change notification.
//!
//! Port of `desktop/ui/js/ui/search-field.js`. Wraps a [`LabeledField`]
//! and adds a clear button that appears when there's text.

use engine::compositor::Compositor;
use engine::theme::Theme;
use engine::ui::widgets::Rect;

use super::{EditKey, LabeledField};
use super::super::text;

/// Search field with clear button. Emits `on_changed` callback when
/// the text changes (debounced externally by the screen if needed).
pub struct SearchField {
    field: LabeledField,
    clear_visible: bool,
    on_changed: Option<Box<dyn Fn(&str) + Send + Sync>>,
}

impl SearchField {
    pub fn new(
        placeholder: impl Into<String>,
        theme: &Theme,
        on_changed: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let field = LabeledField::new("", placeholder, theme);
        Self {
            field,
            clear_visible: false,
            on_changed: Some(Box::new(on_changed)),
        }
    }

    /// Create without an on_changed callback (for manual polling).
    pub fn new_without_callback(placeholder: impl Into<String>, theme: &Theme) -> Self {
        let field = LabeledField::new("", placeholder, theme);
        Self {
            field,
            clear_visible: false,
            on_changed: None,
        }
    }

    pub fn value(&self) -> &str {
        self.field.value()
    }

    pub fn set_value(&mut self, value: &str) {
        self.field.set_value(value);
        self.update_clear_visibility();
    }

    pub fn is_empty(&self) -> bool {
        self.field.is_empty()
    }

    pub fn focus(&mut self) {
        self.field.focus();
    }

    pub fn unfocus(&mut self) {
        self.field.unfocus();
    }

    pub fn is_focused(&self) -> bool {
        self.field.is_focused()
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.field.set_focused(focused);
    }

    pub fn render(&self, c: &mut Compositor, block: Rect, theme: &Theme) {
        self.field.render(c, block, theme);
        if self.clear_visible {
            let glyph = "×";
            let size = 14.0;
            let field = self.field.field_rect(block);
            text(
                c,
                glyph,
                size,
                600,
                field.x + field.w - 24.0,
                field.y + (field.h - size * 1.4) / 2.0,
                theme.colors.text_dim.0,
            );
        }
    }

    fn update_clear_visibility(&mut self) {
        self.clear_visible = !self.field.is_empty();
    }

    // Keyboard handling
    pub fn handle_text(&mut self, s: &str) -> bool {
        let consumed = self.field.handle_text(s);
        self.update_clear_visibility();
        if let Some(cb) = &self.on_changed {
            cb(self.field.value());
        }
        consumed
    }

    pub fn handle_edit_key(&mut self, key: EditKey) -> bool {
        let (consumed, changed) = self.field.handle_edit_key(key);
        if changed {
            self.update_clear_visibility();
            if let Some(cb) = &self.on_changed {
                cb(self.field.value());
            }
        }
        consumed
    }

    pub fn handle_click(&mut self, local_x: f32, field_rect: Rect) {
        let clear_btn_x = 32.0; // right padding area
        if self.clear_visible && local_x >= field_rect.w - clear_btn_x {
            // Clicked clear button
            self.field.set_value("");
            self.update_clear_visibility();
            if let Some(cb) = &self.on_changed {
                cb("");
            }
        } else {
            self.field.click(local_x);
        }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.field.tick(dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_field_clears() {
        let theme = Theme::hoff();
        let mut field = SearchField::new_without_callback("Buscar...", &theme);
        field.set_value("test");
        assert_eq!(field.value(), "test");
        assert!(field.clear_visible);
    }
}

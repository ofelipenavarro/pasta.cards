//! Search input with a clear ("x") button and change notification.
//!
//! Port of `desktop/ui/js/ui/search-field.js`. Wraps a `LabeledField`
//! and adds a clear button that appears when there's text.

use engine::compositor::Compositor;
use engine::theme::Theme;
use engine::ui::widgets::{EventResult, Rect, WidgetEvent};

use super::{EditKey, LabeledField, ScreenCtx};
use crate::art::ArtCache;

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

    pub fn handle_edit_key(&mut self, key: super::EditKey) -> bool {
        if key == super::EditKey::Escape && !self.field.is_empty() {
            // Escape clears the field
            self.field.set_value("");
            self.update_clear_visibility();
            if let Some(cb) = &self.on_changed {
                cb("");
            }
            true
        } else {
            self.field.handle_edit_key(key)
        }
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
            self.field.handle_click(local_x);
        }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.field.tick(dt)
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.field.set_focused(focused);
    }

    pub fn is_focused(&self) -> bool {
        self.field.is_focused()
    }
}

impl SearchField {
    /// Dummy ScreenCtx for internal event handling
    fn dummy() -> ScreenCtx<'static> {
        use std::sync::mpsc;
        let (tx, _rx) = mpsc::channel();
        ScreenCtx {
            tx: &tx,
            actions: &mut Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_field_clears() {
        let theme = crate::engine::theme::Theme::hoff();
        let mut field = SearchField::new_without_callback("Buscar...", &theme);
        field.set_value("test");
        assert_eq!(field.value(), "test");
        assert!(field.clear_visible);
    }
}
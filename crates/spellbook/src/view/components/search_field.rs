//! Search input with a clear ("x") button and change notification.
//!
//! Port of `desktop/ui/js/ui/search-field.js`. Wraps a `LabeledField`
//! and adds a clear button that appears when there's text.

use engine::compositor::{Compositor, Rect};
use engine::theme::Theme;
use engine::ui::widgets::{EventResult, WidgetEvent};

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

    pub fn render(
        &mut self,
        c: &mut Compositor,
        rect: Rect,
        theme: &Theme,
        _art: &mut ArtCache,
    ) {
        // We don't render a label, just the field with clear button
        let field_rect = Rect::new(rect.x, rect.y, rect.w, 36.0);

        // Field background
        c.push(engine::ui::widgets::rounded_rect(
            field_rect.x,
            field_rect.y,
            field_rect.w,
            field_rect.h,
            theme.radius.sm,
            self.field.input.bg_color,
        ));

        // Focus ring
        if self.field.focused {
            c.push(engine::ui::widgets::focus_ring(
                field_rect,
                theme.radius.sm,
                theme,
            ));
        }

        // Text input scene
        for node in self.field.input.build_scene(field_rect.x, field_rect.y, field_rect.w) {
            c.push(node);
        }

        // Clear button (x)
        if self.clear_visible {
            let btn_size = 24.0;
            let btn_x = field_rect.x + field_rect.w - 28.0;
            let btn_y = field_rect.y + 6.0;
            let btn_rect = Rect::new(btn_x, btn_y, btn_size, btn_size);

            // Button background
            c.push(engine::ui::widgets::rounded_rect(
                btn_rect.x,
                btn_rect.y,
                btn_rect.w,
                btn_rect.h,
                theme.radius.sm,
                theme.glass.surface_active.0,
            ));

            // X icon
            if let Some(node) = engine::ui::icons::icon_at(
                "x",
                14.0,
                theme.colors.text_dim.0,
                btn_rect.x + 5.0,
                btn_rect.y + 5.0,
            ) {
                c.push(node);
            }
        }

        // Also render the inner text input scene
        for node in self.field.input.build_scene(rect.x, rect.y, rect.w) {
            c.push(node);
        }
    }

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        rect: Rect,
        _ctx: &mut super::ScreenCtx,
    ) -> bool {
        let field_rect = Rect::new(rect.x, rect.y, rect.w, 36.0);

        match event {
            WidgetEvent::MouseDown { x, y } => {
                if self.clear_visible {
                    let btn_x = rect.x + rect.w - 28.0;
                    let btn_y = rect.y + 6.0;
                    if Rect::new(rect.x + rect.w - 28.0, rect.y + 6.0, 24.0, 24.0).contains(x, y) {
                        // Clear button clicked
                        self.field.set_value("");
                        self.clear_visible = false;
                        if let Some(cb) = &self.on_changed {
                            cb("");
                        }
                        return true;
                    }
                }
                // Delegate to field
                self.field.handle_event(event, rect, &mut super::ScreenCtx::dummy())
            }
            _ => self.field.handle_event(event, rect, &mut super::ScreenCtx::dummy()),
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
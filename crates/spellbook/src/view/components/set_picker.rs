//! Set/printing picker dropdown with search.
//!
//! Port of `desktop/ui/js/ui/set-picker.js`. An autocomplete field that
//! searches sets via the worker thread (API), scoped optionally to a
//! specific card's printings. Selected set populates a hidden code field.

use engine::compositor::{Compositor, Rect, SceneNode};
use engine::text::{TextMeasurer, TextStyle, TypographyScale};
use engine::theme::Theme;
use engine::ui::icons;
use engine::ui::widgets::{EventResult, Rect, WidgetEvent, rounded_rect, focus_ring, glass_pill};

use super::{EditKey, LabeledField, ScreenCtx};
use crate::art::ArtCache;
use crate::spellbook_core::client::{Command, Event};
use crate::spellbook_core::ops::cards::SetInfo;

/// An option in the set picker dropdown.
#[derive(Clone, Debug)]
struct SetOption {
    name: String,
    code: String,
    released_at: Option<String>,
}

/// The set picker component: a search field + dynamic dropdown.
/// The hidden `code` field is updated when a set is selected.
pub struct SetPicker {
    search: LabeledField,
    options: Vec<SetOption>,
    open: bool,
    hovered_idx: Option<usize>,
    focused: bool,
    hidden_code: Option<String>,
    card_name: Option<String>,
    on_changed: Option<Box<dyn Fn(&str) + Send + Sync>>,
    last_query: String,
    request_pending: bool,
}

impl SetPicker {
    pub fn new(
        placeholder: impl Into<String>,
        theme: &Theme,
        on_changed: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let search = LabeledField::new("", placeholder, theme);
        Self {
            search,
            options: Vec::new(),
            open: false,
            hovered_idx: None,
            focused: false,
            hidden_code: None,
            card_name: None,
            on_changed: Some(Box::new(on_changed)),
            last_query: String::new(),
            request_pending: false,
        }
    }

    pub fn new_without_callback(placeholder: impl Into<String>, theme: &Theme) -> Self {
        let search = LabeledField::new("", placeholder, theme);
        Self {
            search,
            options: Vec::new(),
            open: false,
            hovered_idx: None,
            focused: false,
            hidden_code: None,
            card_name: None,
            on_changed: None,
            last_query: String::new(),
            request_pending: false,
        }
    }

    /// Set the card name to scope the set search to that card's printings.
    pub fn set_card_name(&mut self, card_name: Option<String>) {
        self.card_name = card_name;
    }

    /// Get the selected set code (from the hidden field).
    pub fn get_code(&self) -> Option<String> {
        self.hidden_code.clone()
    }

    /// Check if the dropdown is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Focus/unfocus the search field.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.search.set_focused(focused);
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Called when the search text changes - triggers a debounced search.
    fn on_search_changed(&mut self, query: &str) {
        self.last_query = query.to_string();
        // The actual API call is triggered by the screen via handle_data
        // when it receives the SearchSets result.
    }

    /// Called by the screen when search results arrive.
    pub fn set_options(&mut self, options: Vec<SetInfo>) {
        self.options = options
            .into_iter()
            .map(|s| SetOption {
                name: s.name,
                code: s.code,
                released_at: s.released_at.map(|d| d.split('-').next().unwrap_or("").to_string()),
            })
            .collect();
        self.hovered_idx = None;
        self.open = true;
    }

    /// Get the currently hovered/selected option code.
    pub fn hovered_code(&self) -> Option<String> {
        self.hovered_idx.and_then(|i| self.options.get(i)).map(|o| o.code.clone())
    }

    /// Take the selected code (called when user confirms selection).
    pub fn take_selected_code(&mut self) -> Option<String> {
        if let Some(i) = self.hovered_idx {
            let code = self.options[i].code.clone();
            self.hidden_code = Some(code.clone());
            self.open = false;
            self.hovered_idx = None;
            Some(code)
        } else {
            None
        }
    }

    // Keyboard handling
    pub fn handle_text(&mut self, s: &str) -> bool {
        let consumed = self.search.handle_text(s);
        if let Some(cb) = &self.on_changed {
            cb(self.search.value());
        }
        consumed
    }

    pub fn handle_edit_key(&mut self, key: super::EditKey) -> bool {
        match key {
            super::EditKey::Enter => {
                if let Some(code) = self.take_selected_code() {
                    if let Some(cb) = &self.on_changed {
                        cb(&code);
                    }
                    true
                } else {
                    false
                }
            }
            super::EditKey::Escape => {
                if self.open {
                    self.open = false;
                    self.hovered_idx = None;
                    true
                } else {
                    self.search.unfocus();
                    false
                }
            }
            super::EditKey::ArrowDown => {
                if self.open && !self.options.is_empty() {
                    let next = match self.hovered_idx {
                        Some(i) => (i + 1).min(self.options.len() - 1),
                        None => 0,
                    };
                    self.hovered_idx = Some(next);
                    true
                } else {
                    false
                }
            }
            super::EditKey::ArrowUp => {
                if self.open && !self.options.is_empty() {
                    let next = match self.hovered_idx {
                        Some(i) => i.saturating_sub(1),
                        None => self.options.len().saturating_sub(1),
                    };
                    self.hovered_idx = Some(next);
                    true
                } else {
                    false
                }
            }
            _ => self.search.handle_edit_key(key),
        }
    }

    pub fn handle_click(&mut self, local_x: f32, field_rect: Rect) {
        // Delegate click to the search field (handles focus)
        self.search.handle_click(local_x);
    }

    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        rect: Rect,
        _ctx: &mut super::ScreenCtx,
    ) -> bool {
        // Delegate to search field for focus/click handling
        self.search.handle_event(event, rect, &mut super::ScreenCtx::dummy())
    }

    /// Called when search results arrive from the worker thread.
    pub fn on_search_results(&mut self, results: Vec<SetInfo>) {
        self.request_pending = false;
        self.set_options(results);
    }

    /// Trigger a search for the current query.
    pub fn trigger_search(&mut self, ctx: &mut ScreenCtx) {
        if self.last_query.is_empty() {
            self.options.clear();
            self.open = false;
            return;
        }
        self.request_pending = true;
        let card = self.card_name.as_deref().map(|s| s.to_string());
        ctx.send(Command::SearchSets {
            q: self.last_query.clone(),
            card,
            limit: 20,
        });
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.search.tick(dt)
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.search.set_focused(focused);
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn value(&self) -> &str {
        self.search.value()
    }

    pub fn set_value(&mut self, value: &str) {
        self.search.set_value(value);
    }

    pub fn is_empty(&self) -> bool {
        self.search.is_empty()
    }

    pub fn focus(&mut self) {
        self.search.focus();
    }

    pub fn unfocus(&mut self) {
        self.search.unfocus();
    }

    pub fn render(
        &mut self,
        c: &mut Compositor,
        rect: Rect,
        theme: &Theme,
        _art: &mut ArtCache,
    ) {
        // Render the search field
        let field_rect = Rect::new(rect.x, rect.y, rect.w, 36.0);
        self.search.render(c, field_rect, theme, &mut crate::art::ArtCache::new());

        // Render dropdown if open
        if self.open && !self.options.is_empty() {
            let dropdown_rect = Rect::new(
                rect.x,
                rect.y + 36.0 + 4.0,
                rect.w,
                (self.options.len() as f32 * 44.0).min(320.0),
            );
            self.render_dropdown(c, dropdown_rect, theme);
        }
    }

    fn render_dropdown(&self, c: &mut Compositor, bounds: Rect, theme: &Theme) {
        let glass = &theme.glass;
        let radius = theme.radius.lg;

        // Floating panel shadow + glass background
        c.push_to_layer(
            100, // overlay layer
            engine::ui::widgets::menu_shadow(bounds, radius),
        );
        for node in glass_pill(bounds, radius, glass.edge_soft.0, 1.5, glass.popover.0) {
            c.push_to_layer(100, node);
        }

        let style = {
            let mut s = TextStyle::new();
            s.font_size = 14.0;
            s.weight = 500;
            s.line_height = 44.0;
            s
        };
        let text = theme.colors.text;

        let pad_x = 16.0;
        let pad_y = 8.0;

        for (i, option) in self.options.iter().enumerate() {
            let oy = bounds.y + 8.0 + i as f32 * 44.0;
            if oy + 44.0 > bounds.y + bounds.h {
                break;
            }

            let is_hovered = self.hovered_idx == Some(i);

            if is_hovered {
                c.push_to_layer(
                    100,
                    engine::compositor::SceneNode::RoundedRect {
                        x: bounds.x + 4.0,
                        y: oy,
                        w: bounds.w - 8.0,
                        h: 44.0,
                        color: glass.surface_hover.0,
                        corner_radius: theme.radius.md,
                        border_width: 0.0,
                        border_color: [0.0; 4],
                    },
                );
            }

            let label_alpha = if self.hovered_idx == Some(i) { 0.8 } else { 0.59 };
            c.push_to_layer(
                100,
                SceneNode::Text {
                    key: TextNodeKey::from_style(
                        &format!("{} ({})", option.name, option.code.to_uppercase()),
                        &{
                            let mut s = TextStyle::new();
                            s.font_size = 14.0;
                            s.weight = 500;
                            s.line_height = 44.0;
                            s
                        },
                        None,
                    ),
                    x: bounds.x + 16.0,
                    y: oy + 22.0,
                    color: [text.0[0], text.0[1], text.0[2], text.0[3] * label_alpha],
                },
            );
        }
    }
}

impl SetPicker {
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
    fn set_picker_creation() {
        let theme = crate::engine::theme::Theme::hoff();
        let picker = SetPicker::new_without_callback("Nome ou código da edição…", &theme);
        assert!(picker.value().is_empty());
        assert!(!picker.is_open());
    }
}
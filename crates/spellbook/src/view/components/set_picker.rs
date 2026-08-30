//! Set/printing picker dropdown with search.
//!
//! Port of `desktop/ui/js/ui/set-picker.js`. An autocomplete field that
//! searches sets via the worker thread, scoped optionally to a specific
//! card's printings. Selected set populates a hidden code field exposed
//! through [`SetPicker::code`].

use engine::compositor::{Compositor, LayerId, SceneNode, TextNodeKey};
use engine::theme::{Theme, TypographyScale};
use engine::ui::widgets::{EventResult, Rect, WidgetEvent, glass_pill, menu_shadow, rounded_rect};

use spellbook_core::client::Command;
use spellbook_core::ops::cards::SetInfo;

use super::field::LabeledField;
use super::{EditKey, ScreenCtx};

const ROW_H: f32 = 34.0;
const MAX_ROWS: usize = 6;
const GAP: f32 = 4.0;

/// Set/printing picker: a labeled search field with a dynamic dropdown.
pub struct SetPicker {
    pub field: LabeledField,
    card_name: Option<String>,
    options: Vec<SetInfo>,
    open: bool,
    hovered: Option<usize>,
    selected_code: Option<String>,
    in_flight: bool,
    since_edit: f32,
    last_sent_query: Option<String>,
}

impl SetPicker {
    pub fn new(label: impl Into<String>, theme: &Theme, card_name: Option<String>) -> Self {
        Self {
            field: LabeledField::new(label, "Nome ou código da edição…", theme),
            card_name,
            options: Vec::new(),
            open: false,
            hovered: None,
            selected_code: None,
            in_flight: false,
            since_edit: 0.0,
            last_sent_query: None,
        }
    }

    pub fn set_card(&mut self, card_name: Option<String>) {
        self.card_name = card_name;
        self.options.clear();
        self.open = false;
        self.hovered = None;
        self.selected_code = None;
    }

    pub fn code(&self) -> Option<String> {
        self.selected_code.clone()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn field_rect(&self, block: Rect) -> Rect {
        self.field.field_rect(block)
    }

    /// Receive search results from the worker. Returns whether the scene changed.
    pub fn on_suggestions(&mut self, sets: &Vec<SetInfo>) -> bool {
        self.in_flight = false;
        self.last_sent_query = Some(self.field.value().trim().to_string());
        self.options = sets.clone();
        self.hovered = None;
        self.open = !self.options.is_empty();
        true
    }

    /// Build the `Command::SearchSets` for the current query.
    pub fn search_command(&self, limit: i64) -> Command {
        Command::SearchSets {
            q: self.field.value().trim().to_string(),
            card: self.card_name.clone(),
            limit,
        }
    }

    pub fn set_focused(&mut self, focused: bool, _ctx: &mut ScreenCtx) {
        if focused {
            if !self.field.input.focused {
                self.field.input.focus();
                self.since_edit = 0.0;
            }
        } else {
            self.field.input.unfocus();
            self.open = false;
            self.hovered = None;
        }
    }

    pub fn handle_text(&mut self, s: &str) -> EventResult {
        if !self.field.input.focused {
            return EventResult::IGNORED;
        }
        self.field.handle_text(s);
        self.open = false;
        self.hovered = None;
        self.selected_code = None;
        self.since_edit = 0.0;
        EventResult::changed()
    }

    pub fn handle_edit_key(&mut self, key: EditKey) -> (bool, bool) {
        if !self.field.input.focused && !self.open {
            return (false, false);
        }
        match key {
            EditKey::Enter => {
                if self.open {
                    if let Some(i) = self.hovered {
                        self.pick(i);
                        return (true, true);
                    }
                }
                (false, false)
            }
            EditKey::Tab => (false, false),
            _ => {
                if self.open {
                    match key {
                        EditKey::Left | EditKey::Right | EditKey::Home | EditKey::End => {
                            let changed = self.field.handle_edit_key(key).1;
                            return (true, changed);
                        }
                        EditKey::Up => {
                            if !self.options.is_empty() {
                                let i = self.hovered.map_or(0, |i| i.saturating_sub(1));
                                self.hovered = Some(i);
                                return (true, true);
                            }
                            (true, false)
                        }
                        EditKey::Down => {
                            if !self.options.is_empty() {
                                let last = self.options.len() - 1;
                                let i = self.hovered.map_or(0, |i| (i + 1).min(last));
                                self.hovered = Some(i);
                                return (true, true);
                            }
                            (true, false)
                        }
                        _ => (false, false),
                    }
                } else {
                    let changed = self.field.handle_edit_key(key).1;
                    (true, changed)
                }
            }
        }
    }

    pub fn handle_escape(&mut self) -> bool {
        if self.open {
            self.open = false;
            self.hovered = None;
            true
        } else {
            false
        }
    }

    fn pick(&mut self, i: usize) {
        if let Some(set) = self.options.get(i) {
            self.selected_code = Some(set.code.clone());
            self.field.set_value(set.name.as_deref().unwrap_or(&set.code));
            self.open = false;
            self.hovered = None;
        }
    }

    fn dropdown_rect(&self, block: Rect) -> Rect {
        let field = self.field_rect(block);
        let rows = self.options.len().min(MAX_ROWS) as f32;
        let h = rows * ROW_H + (rows.max(1.0) + 1.0) * GAP;
        Rect::new(field.x, field.y + field.h + 4.0, field.w, h)
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, block: Rect) -> EventResult {
        let field = self.field_rect(block);
        match *event {
            WidgetEvent::MouseMove { x, y } => {
                if self.open {
                    let dd = self.dropdown_rect(block);
                    if dd.contains(x, y) {
                        let i = ((y - dd.y - GAP) / (ROW_H + GAP)).floor() as usize;
                        if i < self.options.len() {
                            if self.hovered != Some(i) {
                                self.hovered = Some(i);
                                return EventResult::changed();
                            }
                        }
                    }
                }
                EventResult::IGNORED
            }
            WidgetEvent::MouseDown { x, y } => {
                if self.open {
                    let dd = self.dropdown_rect(block);
                    if dd.contains(x, y) {
                        let i = ((y - dd.y - GAP) / (ROW_H + GAP)).floor() as usize;
                        if i < self.options.len() {
                            self.pick(i);
                            return EventResult::clicked();
                        }
                    }
                }
                if field.contains(x, y) {
                    self.field.click(x - field.x);
                    EventResult::changed()
                } else {
                    EventResult::IGNORED
                }
            }
            _ => EventResult::IGNORED,
        }
    }

    pub fn tick(&mut self, dt: f32, ctx: &mut ScreenCtx) -> bool {
        self.field.tick(dt);
        self.since_edit += dt;
        let q = self.field.value().trim().to_string();
        if !self.in_flight
            && self.field.input.focused
            && !q.is_empty()
            && self.since_edit >= 0.15
            && self.last_sent_query.as_deref() != Some(&q)
        {
            self.in_flight = true;
            ctx.send(self.search_command(20));
        }
        self.field.input.focused || self.in_flight
    }

    pub fn render(&self, c: &mut Compositor, block: Rect, theme: &Theme) {
        self.field.render(c, block, theme);
    }

    pub fn render_overlay(&self, c: &mut Compositor, _layer: LayerId, block: Rect, theme: &Theme) {
        if !self.open || self.options.is_empty() {
            return;
        }
        let dd = self.dropdown_rect(block);
        let radius = theme.radius.lg;
        c.push(menu_shadow(dd, radius));
        for node in glass_pill(dd, radius, theme.glass.edge_soft.0, 1.5, theme.glass.popover.0) {
            c.push(node);
        }

        let style = TypographyScale::hoff().base_2sm();
        for (i, set) in self.options.iter().enumerate() {
            let row = Rect::new(
                dd.x + GAP,
                dd.y + GAP + i as f32 * (ROW_H + GAP),
                dd.w - GAP * 2.0,
                ROW_H,
            );
            if self.hovered == Some(i) {
                c.push(rounded_rect(
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    theme.radius.md,
                    theme.glass.surface_hover.0,
                ));
            }
            let label = format!(
                "{} ({})",
                set.name.as_deref().unwrap_or(""),
                set.code.to_uppercase()
            );
            c.push(SceneNode::Text {
                key: TextNodeKey::from_style(&label, &style, Some(row.w - 16.0)),
                x: row.x + 8.0,
                y: row.y + (ROW_H - style.line_height) / 2.0,
                color: theme.colors.text.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_picker_creation() {
        let theme = engine::theme::Theme::hoff();
        let picker = SetPicker::new("Edição", &theme, None);
        assert!(picker.code().is_none());
        assert!(!picker.is_open());
    }
}

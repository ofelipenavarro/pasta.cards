//! Generic filter component for Collection, Decks, and Wishlist screens.
//!
//! Port of `desktop/ui/js/ui/card-filters.js`. Provides a unified filter
//! toolbar using single-select dropdowns and removable chips for active filters.

use engine::compositor::Compositor;
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{Button, ButtonVariant, Chip, EventResult, Rect, Select, WidgetEvent};

use super::{EditKey, LabeledField, SearchField};
use crate::view::text;

/// Serializable filter state for persistence and URL routing.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FilterState {
    pub status: Vec<String>,
    pub colors: Vec<String>,
    pub types: Vec<String>,
    pub rarities: Vec<String>,
    pub sets: Vec<String>,
    pub search: String,
}

impl FilterState {
    pub fn is_empty(&self) -> bool {
        self.status.is_empty()
            && self.colors.is_empty()
            && self.types.is_empty()
            && self.rarities.is_empty()
            && self.sets.is_empty()
            && self.search.is_empty()
    }

    pub fn active_count(&self) -> usize {
        self.status.len()
            + self.colors.len()
            + self.types.len()
            + self.rarities.len()
            + self.sets.len()
            + if self.search.is_empty() { 0 } else { 1 }
    }

    pub fn clear(&mut self) {
        self.status.clear();
        self.colors.clear();
        self.types.clear();
        self.rarities.clear();
        self.sets.clear();
        self.search.clear();
    }
}

/// Predefined filter options for each category.
pub mod options {
    pub const STATUSES: &[&str] = &["Owned", "Missing", "Wishlist", "Sideboard"];
    pub const COLORS: &[&str] = &["W", "U", "B", "R", "G", "C", "Multicolor", "Colorless"];
    pub const TYPES: &[&str] = &[
        "Creature", "Instant", "Sorcery", "Artifact", "Enchantment",
        "Planeswalker", "Land", "Battle",
    ];
    pub const RARITIES: &[&str] = &["Common", "Uncommon", "Rare", "Mythic"];
}

/// A filter category with single-select dropdown + active chips.
struct FilterCategory {
    label: &'static str,
    select: Select,
    options: Vec<String>,
    selected: Vec<usize>,
    rect: Rect,
}

impl FilterCategory {
    pub fn new(label: &'static str, options: &[&str]) -> Self {
        let opts: Vec<String> = options.iter().map(|s| s.to_string()).collect();
        Self {
            label,
            select: Select::new(options.iter().copied(), 0),
            options: opts,
            selected: Vec::new(),
            rect: Rect::ZERO,
        }
    }

    pub fn set_options(&mut self, options: Vec<String>) {
        self.options = options;
        self.select = Select::new(self.options.iter().cloned(), 0);
        self.selected.clear();
    }

    pub fn selected_labels(&self) -> Vec<String> {
        self.selected.iter().filter_map(|&i| self.options.get(i).cloned()).collect()
    }

    pub fn set_selected(&mut self, labels: &[String]) {
        self.selected.clear();
        for (i, opt) in self.options.iter().enumerate() {
            if labels.contains(opt) {
                self.selected.push(i);
            }
        }
    }

    pub fn is_open(&self) -> bool {
        self.select.is_open()
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, layout_rect: Rect) -> EventResult {
        self.rect = layout_rect;
        let result = self.select.handle_event(event, layout_rect.x, layout_rect.y, layout_rect.w, layout_rect.h);
        if self.select.is_open() == false && result.changed() {
            // Selection changed - toggle multi-select
            let sel = self.select.selected;
            if self.selected.contains(&sel) {
                self.selected.retain(|&i| i != sel);
            } else {
                self.selected.push(sel);
            }
            self.select.close();
            return EventResult::changed();
        }
        result
    }

    pub fn render(&self, c: &mut Compositor, theme: &Theme, block: Rect) {
        // Render select dropdown
        self.select.render(c, theme, block.x, block.y, block.w, block.h);
        // Render active chips below
        let mut y = block.y + block.h + 4.0;
        for &idx in &self.selected {
            if let Some(label) = self.options.get(idx) {
                let chip = Chip::new(label).selected(true).interactive(true).intent(Intent::Constructive);
                let chip_w = chip.width(theme);
                chip.render(c, block.x, y, theme);
                y += 28.0;
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.select.selected = 0;
        self.select.close();
    }
}

/// Complete filter toolbar for a screen.
pub struct FilterBar {
    pub state: FilterState,
    status: FilterCategory,
    color: FilterCategory,
    type_: FilterCategory,
    rarity: FilterCategory,
    set: FilterCategory,
    search: SearchField,
    clear_btn: Button,
    layout: FilterLayout,
    on_change: Option<Box<dyn Fn(&FilterState) + Send + Sync>>,
}

#[derive(Default)]
struct FilterLayout {
    status: Rect,
    color: Rect,
    type_: Rect,
    rarity: Rect,
    set: Rect,
    search: Rect,
    clear: Rect,
}

impl FilterBar {
    const CLEAR_LABEL: &'static str = "Limpar";

    pub fn new(theme: &Theme, on_change: impl Fn(&FilterState) + Send + Sync + 'static) -> Self {
        Self {
            state: FilterState::default(),
            status: FilterCategory::new("Status", options::STATUSES),
            color: FilterCategory::new("Cor", options::COLORS),
            type_: FilterCategory::new("Tipo", options::TYPES),
            rarity: FilterCategory::new("Raridade", options::RARITIES),
            set: FilterCategory::new("Edição", &[]),
            search: SearchField::new_without_callback("Buscar...", theme),
            clear_btn: Button::new(Self::CLEAR_LABEL).variant(ButtonVariant::Ghost),
            layout: FilterLayout::default(),
            on_change: Some(Box::new(on_change)),
        }
    }

    pub fn set_set_options(&mut self, sets: Vec<String>) {
        self.set.set_options(sets);
    }

    pub fn layout(&mut self, content: Rect) {
        const GAP: f32 = 12.0;
        const DROPDOWN_W: f32 = 140.0;
        const SEARCH_W: f32 = 220.0;
        const CLEAR_W: f32 = 80.0;
        const ROW_H: f32 = 36.0;

        let mut x = content.x;
        let y = content.y;

        self.layout.status = Rect::new(x, y, DROPDOWN_W, ROW_H);
        x += DROPDOWN_W + GAP;

        self.layout.color = Rect::new(x, y, DROPDOWN_W, ROW_H);
        x += DROPDOWN_W + GAP;

        self.layout.type_ = Rect::new(x, y, DROPDOWN_W, ROW_H);
        x += DROPDOWN_W + GAP;

        self.layout.rarity = Rect::new(x, y, DROPDOWN_W, ROW_H);
        x += DROPDOWN_W + GAP;

        self.layout.set = Rect::new(x, y, DROPDOWN_W, ROW_H);
        x += DROPDOWN_W + GAP;

        self.layout.search = Rect::new(x, y, SEARCH_W, ROW_H);
        x += SEARCH_W + GAP;

        self.layout.clear = Rect::new(x, y, CLEAR_W, ROW_H);
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, content: Rect) -> EventResult {
        self.layout(content);
        let mut changed = false;

        for (cat, layout) in [
            (&mut self.status, self.layout.status),
            (&mut self.color, self.layout.color),
            (&mut self.type_, self.layout.type_),
            (&mut self.rarity, self.layout.rarity),
            (&mut self.set, self.layout.set),
        ] {
            let result = cat.handle_event(event, layout);
            if result.changed() {
                changed = true;
            }
        }

        // Search field
        if self.search.value() != self.state.search {
            self.state.search = self.search.value().to_string();
            changed = true;
        }

        // Clear all button
        if let WidgetEvent::MouseUp { x, y } = *event {
            if self.layout.clear.contains(x, y) && !self.state.is_empty() {
                self.state.clear();
                self.status.clear_selection();
                self.color.clear_selection();
                self.type_.clear_selection();
                self.rarity.clear_selection();
                self.set.clear_selection();
                self.search.set_value("");
                changed = true;
            }
        }

        if changed {
            self.sync_state_from_ui();
            if let Some(cb) = &self.on_change {
                cb(&self.state);
            }
            return EventResult::changed();
        }

        EventResult::IGNORED
    }

    fn sync_state_from_ui(&mut self) {
        self.state.status = self.status.selected_labels();
        self.state.colors = self.color.selected_labels();
        self.state.types = self.type_.selected_labels();
        self.state.rarities = self.rarity.selected_labels();
        self.state.sets = self.set.selected_labels();
        self.state.search = self.search.value().to_string();
    }

    pub fn render(&self, c: &mut Compositor, theme: &Theme, content: Rect) {
        for (cat, layout) in [
            (&self.status, self.layout.status),
            (&self.color, self.layout.color),
            (&self.type_, self.layout.type_),
            (&self.rarity, self.layout.rarity),
            (&self.set, self.layout.set),
        ] {
            cat.render(c, theme, layout);
        }

        self.search.render(c, self.layout.search, theme);

        if self.state.active_count() > 0 {
            self.clear_btn.render(c, self.layout.clear, theme);
        }

        if self.state.active_count() > 0 {
            text(
                c,
                &format!("{} filtro(s) ativo(s)", self.state.active_count()),
                12.0,
                500,
                content.x,
                content.y + 52.0,
                theme.colors.text_dim.0,
            );
        }
    }

    pub fn apply_state(&mut self, state: FilterState) {
        self.state = state.clone();
        self.status.set_selected(&state.status);
        self.color.set_selected(&state.colors);
        self.type_.set_selected(&state.types);
        self.rarity.set_selected(&state.rarities);
        self.set.set_selected(&state.sets);
        self.search.set_value(&state.search);
    }
}

/// Filter answer returned to the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterAnswer {
    Changed,
    Cleared,
}
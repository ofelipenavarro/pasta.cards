//! Generic filter component for Collection, Decks, and Wishlist screens.
//!
//! Port of `desktop/ui/js/ui/card-filters.js`. Provides a unified filter
//! toolbar with dropdowns for status, color, type, rarity, set, and a
//! text search field. The filter state is serializable for URL routing.

use engine::compositor::Compositor;
use engine::theme::Theme;
use engine::ui::widgets::{Button, ButtonVariant, Chip, Dropdown, DropdownItem, EventResult, Rect, WidgetEvent};

use super::{EditKey, LabeledField, SearchField};
use super::super::text;

/// Filter categories shared across screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterCategory {
    Status,
    Color,
    Type,
    Rarity,
    Set,
    Search,
}

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

/// A single filter dropdown with multi-select chips.
pub struct FilterDropdown {
    label: &'static str,
    items: Vec<DropdownItem>,
    selected: Vec<usize>,
    dropdown: Dropdown,
    rect: Rect,
    open: bool,
}

impl FilterDropdown {
    pub fn new(label: &'static str, options: &[&str]) -> Self {
        let items = options.iter().map(|s| DropdownItem::new(*s)).collect();
        Self {
            label,
            items,
            selected: Vec::new(),
            dropdown: Dropdown::new(),
            rect: Rect::ZERO,
            open: false,
        }
    }

    pub fn selected_labels(&self) -> Vec<String> {
        self.selected.iter().map(|&i| self.items[i].label.clone()).collect()
    }

    pub fn set_selected(&mut self, labels: &[String]) {
        self.selected.clear();
        for (i, item) in self.items.iter().enumerate() {
            if labels.contains(&item.label) {
                self.selected.push(i);
            }
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, layout_rect: Rect) -> EventResult {
        self.rect = layout_rect;
        let result = self.dropdown.handle_event(event, layout_rect.x, layout_rect.y, layout_rect.w, layout_rect.h);
        if let Some(selected_idx) = self.dropdown.take_selected() {
            if self.selected.contains(&selected_idx) {
                self.selected.retain(|&i| i != selected_idx);
            } else {
                self.selected.push(selected_idx);
            }
            self.open = true;
            return EventResult::changed();
        }
        result
    }

    pub fn render(&self, c: &mut Compositor, theme: &Theme, block: Rect) {
        self.dropdown.render(c, block.x, block.y, block.w, block.h, theme);
        // Render selected chips below the dropdown
        let mut y = block.y + block.h + 4.0;
        for idx in &self.selected {
            let label = &self.items[*idx].label;
            let chip = Chip::new(label).removable(true);
            let chip_w = chip.width(theme);
            chip.render(c, block.x, y, theme);
            y += 28.0;
        }
    }
}

/// Complete filter toolbar for a screen.
pub struct FilterBar {
    pub state: FilterState,
    status: FilterDropdown,
    color: FilterDropdown,
    type_: FilterDropdown,
    rarity: FilterDropdown,
    set: FilterDropdown,
    search: SearchField,
    /// Layout rects for each dropdown (computed in layout pass).
    layout: FilterLayout,
    /// Callback when filter changes.
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
    total_height: f32,
}

impl FilterBar {
    /// Label shown on the clear-all button.
    const CLEAR_LABEL: &'static str = "Limpar";

    pub fn new(theme: &Theme, on_change: impl Fn(&FilterState) + Send + Sync + 'static) -> Self {
        let status = FilterDropdown::new("Status", options::STATUSES);
        let color = FilterDropdown::new("Cor", options::COLORS);
        let type_ = FilterDropdown::new("Tipo", options::TYPES);
        let rarity = FilterDropdown::new("Raridade", options::RARITIES);
        let set = FilterDropdown::new("Edição", &[]); // populated dynamically
        let search = SearchField::new_without_callback("Buscar...", theme);

        Self {
            state: FilterState::default(),
            status,
            color,
            type_,
            rarity,
            set,
            search,
            layout: FilterLayout::default(),
            on_change: Some(Box::new(on_change)),
        }
    }

    /// Update set options from the card index (called on mount/refresh).
    pub fn set_set_options(&mut self, sets: Vec<String>) {
        self.set.items = sets.into_iter().map(DropdownItem::new).collect();
    }

    /// Total height needed for the filter bar (depends on open dropdowns/chips).
    pub fn height(&self, content_width: f32) -> f32 {
        let mut h = 44.0; // base row
        for d in [&self.status, &self.color, &self.type_, &self.rarity, &self.set] {
            if d.is_open() || !d.selected.is_empty() {
                h += 28.0 * (d.selected.len() + 1) as f32;
            }
        }
        h
    }

    /// Compute layout rects for a given content rect.
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

        self.layout.total_height = ROW_H;
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, content: Rect) -> EventResult {
        self.layout(content);
        let mut changed = false;

        // Handle dropdowns
        for (dropdown, layout) in [
            (&mut self.status, self.layout.status),
            (&mut self.color, self.layout.color),
            (&mut self.type_, self.layout.type_),
            (&mut self.rarity, self.layout.rarity),
            (&mut self.set, self.layout.set),
        ] {
            let result = dropdown.handle_event(event, layout);
            if result.changed() {
                changed = true;
            }
        }

        // Handle search field
        if let Some(search_layout) = Some(self.layout.search) {
            // Note: SearchField needs its own event handling
        }

        // Handle clear all button
        if let WidgetEvent::MouseUp { x, y } = *event {
            if self.layout.clear.contains(x, y) {
                self.state.clear();
                self.status.selected.clear();
                self.color.selected.clear();
                self.type_.selected.clear();
                self.rarity.selected.clear();
                self.set.selected.clear();
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
        // Render dropdowns
        for (dropdown, layout) in [
            (&self.status, self.layout.status),
            (&self.color, self.layout.color),
            (&self.type_, self.layout.type_),
            (&self.rarity, self.layout.rarity),
            (&self.set, self.layout.set),
        ] {
            dropdown.render(c, theme, layout);
        }

        // Render search field
        self.search.render(c, self.layout.search, theme);

        // Render clear button
        if self.state.active_count() > 0 {
            let btn = Button::new(Self::CLEAR_LABEL)
                .variant(ButtonVariant::Ghost)
                .size(CLEAR_W, 36.0);
            btn.render(c, self.layout.clear.x, self.layout.clear.y, theme);
        }

        // Active filter chips count badge
        if self.state.active_count() > 0 {
            let count = self.state.active_count();
            text(
                c,
                &format!("{} filtro(s) ativo(s)", count),
                12.0,
                500,
                content.x,
                content.y + self.layout.total_height + 4.0,
                theme.colors.text_dim.0,
            );
        }
    }

    /// Apply filter state from URL/serialized form.
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
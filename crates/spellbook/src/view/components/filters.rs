//! Card filtering shared by the deck, collection and wishlist screens.
//!
//! Port of `desktop/ui/js/ui/card-filters.js`. Both the JS screens filter
//! the same thing — a list of cards enriched from the Scryfall index — so
//! the predicates live here once, and each screen owns a [`FilterState`]
//! and decides what to do when it changes.
//!
//! The JS toggled chips in a dropdown; here the menu is [`FilterBar`], a
//! retained widget the screen embeds beside its search field: toggle
//! opens it, chips flip in place, "Limpar filtros" clears. Filtering
//! itself runs client-side over the list already loaded, exactly as the
//! JS did it — toggling a chip must not cost a round trip.

use engine::compositor::{Compositor, LayerId};
use engine::theme::Theme;
use engine::ui::icons;
use engine::ui::widgets::{
    Button, ButtonVariant, Chip, EventResult, Rect, WidgetEvent, glass_pill, menu_shadow,
    rounded_rect,
};
use spellbook_core::ops::collection::CollectionEntry;
use spellbook_core::ops::wishlist::WishlistGroup;

use super::super::text;

pub const CATEGORY_LABELS: [(&str, &str); 9] = [
    ("Comandante", "Comandante"),
    ("Land", "Terrenos"),
    ("Creature", "Criaturas"),
    ("Instant", "Instantâneas"),
    ("Sorcery", "Feitiços"),
    ("Artifact", "Artefatos"),
    ("Enchantment", "Encantamentos"),
    ("Planeswalker", "Planeswalker"),
    ("Outro", "Outro"),
];

/// `CATEGORY_LABELS` order minus the commander, which is never chip-filtered.
pub const FILTERABLE_TYPES: [&str; 8] = [
    "Land", "Creature", "Instant", "Sorcery", "Artifact", "Enchantment", "Planeswalker", "Outro",
];

pub const FILTER_TYPE_LABELS: [(&str, &str); 8] = [
    ("Land", "Terreno"),
    ("Creature", "Criatura"),
    ("Instant", "Inst."),
    ("Sorcery", "Feit."),
    ("Artifact", "Art."),
    ("Enchantment", "Enc."),
    ("Planeswalker", "PW"),
    ("Outro", "Outro"),
];

pub const CMC_BUCKETS: [&str; 6] = ["0", "1", "2", "3", "4", "5"];

pub const RARITIES: [&str; 4] = ["common", "uncommon", "rare", "mythic"];
pub const RARITY_LABELS: [(&str, &str); 4] = [
    ("common", "Comum"),
    ("uncommon", "Incomum"),
    ("rare", "Rara"),
    ("mythic", "Mítica"),
];

/// The colors a chip can filter by, with their pip backgrounds (the
/// `FILTER_COLORS` palette). `C` is colorless.
pub const FILTER_COLORS: [(char, [f32; 4]); 6] = [
    ('W', [1.0, 0.984, 0.835, 1.0]),   // #fffbd5
    ('U', [0.667, 0.878, 0.980, 1.0]), // #aae0fa
    ('B', [0.796, 0.761, 0.749, 1.0]), // #cbc2bf
    ('R', [0.976, 0.667, 0.561, 1.0]), // #f9aa8f
    ('G', [0.608, 0.827, 0.682, 1.0]), // #9bd3ae
    ('C', [0.545, 0.514, 0.596, 1.0]), // #8b8398
];

/// What a card needs to answer the predicates. Screens map their own card
/// types into this — collection entries, deck cards and wishlist groups all
/// carry the same enriched fields.
pub trait FilterCard {
    fn filter_name(&self) -> &str;
    fn filter_type_line(&self) -> Option<&str>;
    fn filter_colors(&self) -> Option<&str>;
    fn filter_cmc(&self) -> Option<f64>;
    fn filter_rarity(&self) -> Option<&str>;
}

/// Filter state, mutated in place ("clear" keeps the object alive so
/// references survive, like the JS's in-place mutation).
#[derive(Clone, Default, Debug)]
pub struct FilterState {
    pub q: String,
    pub types: Vec<String>,
    pub colors: Vec<char>,
    pub cmcs: Vec<String>,
    pub rarities: Vec<String>,
}

impl FilterState {
    pub fn is_active(&self) -> bool {
        !(self.q.is_empty()
            && self.types.is_empty()
            && self.colors.is_empty()
            && self.cmcs.is_empty()
            && self.rarities.is_empty())
    }

    pub fn clear(&mut self) {
        self.q.clear();
        self.types.clear();
        self.colors.clear();
        self.cmcs.clear();
        self.rarities.clear();
    }

    fn toggle<T: PartialEq>(list: &mut Vec<T>, value: T) {
        match list.iter().position(|v| *v == value) {
            Some(i) => {
                list.remove(i);
            }
            None => list.push(value),
        }
    }

    pub fn toggle_type(&mut self, t: &str) {
        Self::toggle(&mut self.types, t.to_string());
    }

    pub fn toggle_color(&mut self, c: char) {
        Self::toggle(&mut self.colors, c);
    }

    pub fn toggle_cmc(&mut self, v: &str) {
        Self::toggle(&mut self.cmcs, v.to_string());
    }

    pub fn toggle_rarity(&mut self, r: &str) {
        Self::toggle(&mut self.rarities, r.to_string());
    }
}

/// The client-side backend's `cardCategory`, on the trait's type line.
pub fn card_category(type_line: Option<&str>) -> &'static str {
    let t = type_line.unwrap_or("");
    for cat in [
        "Land",
        "Creature",
        "Planeswalker",
        "Battle",
        "Artifact",
        "Enchantment",
        "Instant",
        "Sorcery",
    ] {
        if t.contains(cat) {
            return cat;
        }
    }
    "Outro"
}

/// The client-side backend's `cardCmcBucket` — 6+ collapses into "5+"
/// buckets here (the JS had ["0".."6+"], seven chips; six fit the bar).
pub fn card_cmc_bucket(cmc: Option<f64>) -> &'static str {
    let n = cmc.unwrap_or(0.0).floor() as i64;
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        _ => "5",
    }
}

/// The search/type/color/CMC/rarity fields the two enriched list payloads
/// (collection and wishlist) already carry, wired to the trait once here.
impl FilterCard for CollectionEntry {
    fn filter_name(&self) -> &str {
        &self.card_name
    }
    fn filter_type_line(&self) -> Option<&str> {
        self.type_line.as_deref()
    }
    fn filter_colors(&self) -> Option<&str> {
        self.colors.as_deref()
    }
    fn filter_cmc(&self) -> Option<f64> {
        self.cmc
    }
    fn filter_rarity(&self) -> Option<&str> {
        self.rarity.as_deref()
    }
}

impl FilterCard for WishlistGroup {
    fn filter_name(&self) -> &str {
        &self.card_name
    }
    fn filter_type_line(&self) -> Option<&str> {
        self.type_line.as_deref()
    }
    fn filter_colors(&self) -> Option<&str> {
        self.colors.as_deref()
    }
    fn filter_cmc(&self) -> Option<f64> {
        self.cmc
    }
    fn filter_rarity(&self) -> Option<&str> {
        self.rarity.as_deref()
    }
}

/// The JS's `matchesFilters`, on the [`FilterCard`] trait.
pub fn matches_filters<F: FilterCard>(c: &F, f: &FilterState) -> bool {
    if !f.q.is_empty() && !c.filter_name().to_lowercase().contains(&f.q.to_lowercase()) {
        return false;
    }
    if !f.types.is_empty() {
        let cat = card_category(c.filter_type_line());
        if !f.types.iter().any(|t| t == cat) {
            return false;
        }
    }
    if !f.colors.is_empty() {
        let letters: Vec<char> = c.filter_colors().unwrap_or("").chars().collect();
        let is_colorless = letters.is_empty();
        let hit = (is_colorless && f.colors.contains(&'C'))
            || letters.iter().any(|l| f.colors.contains(l));
        if !hit {
            return false;
        }
    }
    if !f.cmcs.is_empty() && !f.cmcs.iter().any(|v| *v == card_cmc_bucket(c.filter_cmc())) {
        return false;
    }
    if !f.rarities.is_empty() {
        let rarity = c.filter_rarity().map(|s| s.to_ascii_lowercase());
        if !f.rarities.iter().any(|r| Some(r.as_str()) == rarity.as_deref()) {
            return false;
        }
    }
    true
}

/// Filter menu + toggle button, one retained widget a screen embeds.
/// `rarity` shows the rarity row (collection/wishlist have rarity, the
/// deck's grouped lists carry it too). Open state, chip rects and the
/// clear button are all here so screens only place one rect.
pub struct FilterBar {
    pub state: FilterState,
    pub show_rarity: bool,
    open: bool,
    hover_menu: bool,

    type_chips: Vec<Chip>,
    color_chips: Vec<Chip>,
    cmc_chips: Vec<Chip>,
    rarity_chips: Vec<Chip>,
    clear_btn: Button,
}

impl FilterBar {
    pub fn new(_theme: &Theme, show_rarity: bool) -> Self {
        let chips = |labels: &[(&str, &str)]| -> Vec<Chip> {
            labels
                .iter()
                .map(|(_, label)| Chip::new(*label).interactive(true))
                .collect()
        };
        let type_labels: Vec<(&str, &str)> = FILTER_TYPE_LABELS.to_vec();
        let rarity_labels: Vec<(&str, &str)> = RARITY_LABELS.to_vec();
        Self {
            state: FilterState::default(),
            show_rarity,
            open: false,
            hover_menu: false,
            type_chips: chips(&type_labels),
            color_chips: FILTER_COLORS
                .iter()
                .map(|(c, _)| Chip::new(c.to_string()).interactive(true))
                .collect(),
            cmc_chips: CMC_BUCKETS
                .iter()
                .map(|b| Chip::new(*b).interactive(true))
                .collect(),
            rarity_chips: chips(&rarity_labels),
            clear_btn: Button::new("Limpar filtros").variant(ButtonVariant::Outline),
        }
    }

    /// The toggle button's rect, for hit-testing around the menu.
    pub fn toggle_w() -> f32 {
        40.0
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Menu size below the toggle.
    fn menu_size(&self) -> (f32, f32) {
        let rows = 3.0 + if self.show_rarity { 1.0 } else { 0.0 };
        (360.0, rows * 34.0 + 30.0 + if self.state.is_active() { 38.0 } else { 0.0 })
    }

    /// Sync chip `selected` from state; called before render and events.
    fn sync_chips(&mut self) {
        for (i, chip) in self.type_chips.iter_mut().enumerate() {
            let t = FILTER_TYPE_LABELS[i].0;
            chip.selected = self.state.types.iter().any(|s| s == t);
        }
        for (i, chip) in self.color_chips.iter_mut().enumerate() {
            let c = FILTER_COLORS[i].0;
            chip.selected = self.state.colors.contains(&c);
        }
        for (i, chip) in self.cmc_chips.iter_mut().enumerate() {
            let b = CMC_BUCKETS[i];
            chip.selected = self.state.cmcs.iter().any(|s| s == b);
        }
        for (i, chip) in self.rarity_chips.iter_mut().enumerate() {
            let r = RARITIES[i];
            chip.selected = self.state.rarities.iter().any(|s| s == r);
        }
    }

    /// Menu rect hanging under the toggle at `toggle_rect`'s right edge.
    pub fn menu_rect(&self, toggle_rect: Rect) -> Rect {
        let (w, h) = self.menu_size();
        let x = (toggle_rect.x + toggle_rect.w - w).max(0.0);
        Rect::new(x, toggle_rect.y + toggle_rect.h + 6.0, w, h)
    }

    /// Any part of the control open/hovering (screens use this to route
    /// pointer events before the tile grid underneath).
    pub fn wants_pointer(&self, x: f32, y: f32, toggle_rect: Rect) -> bool {
        toggle_rect.contains(x, y) || (self.open && self.menu_rect(toggle_rect).contains(x, y))
    }

    /// Handle one pointer event. `toggle_rect` is where the filter button
    /// was drawn this frame. Returns whether the frame changed.
    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        toggle_rect: Rect,
        content: Rect,
    ) -> EventResult {
        let mut changed = EventResult::IGNORED;
        match *event {
            WidgetEvent::MouseDown { x, y } => {
                if toggle_rect.contains(x, y) {
                    self.open = !self.open;
                    return EventResult::clicked();
                }
                if !self.open {
                    return EventResult::IGNORED;
                }
                let menu = self.menu_rect(toggle_rect);
                if !menu.contains(x, y) {
                    self.open = false;
                    return EventResult::changed();
                }
                self.sync_chips();
                self.event_chips(event, menu, &mut changed);
            }
            WidgetEvent::MouseMove { x, y } if self.open => {
                let menu = self.menu_rect(toggle_rect);
                let hover = menu.contains(x, y);
                if hover != self.hover_menu {
                    self.hover_menu = hover;
                    changed = changed.merge(EventResult::changed());
                }
                self.sync_chips();
                self.event_chips(event, menu, &mut changed);
            }
            _ => {}
        }
        let _ = content;
        changed
    }

    fn event_chips(&mut self, event: &WidgetEvent, menu: Rect, changed: &mut EventResult) {
        let rects = self.chip_rects(menu);
        for (i, rect) in rects.types.iter().enumerate() {
            let r = self.type_chips[i].handle_event(event, *rect);
            if r.clicked {
                self.state.toggle_type(FILTER_TYPE_LABELS[i].0);
                *changed = changed.merge(EventResult::clicked());
            }
        }
        for (i, rect) in rects.colors.iter().enumerate() {
            let r = self.color_chips[i].handle_event(event, *rect);
            if r.clicked {
                self.state.toggle_color(FILTER_COLORS[i].0);
                *changed = changed.merge(EventResult::clicked());
            }
        }
        for (i, rect) in rects.cmcs.iter().enumerate() {
            let r = self.cmc_chips[i].handle_event(event, *rect);
            if r.clicked {
                self.state.toggle_cmc(CMC_BUCKETS[i]);
                *changed = changed.merge(EventResult::clicked());
            }
        }
        if self.show_rarity {
            for (i, rect) in rects.rarities.iter().enumerate() {
                let r = self.rarity_chips[i].handle_event(event, *rect);
                if r.clicked {
                    self.state.toggle_rarity(RARITIES[i]);
                    *changed = changed.merge(EventResult::clicked());
                }
            }
        }
        if self.state.is_active() {
            let r = self.clear_btn.handle_event(event, rects.clear);
            if r.clicked {
                self.state.clear();
                *changed = changed.merge(EventResult::clicked());
            }
        }
    }

    fn chip_rects(&self, menu: Rect) -> ChipRects {
        let mut y = menu.y + 10.0;
        let mut types = Vec::new();
        let mut x = menu.x + 10.0;
        for chip in &self.type_chips {
            let (w, _) = chip.preferred_size();
            if x + w > menu.x + menu.w - 10.0 {
                x = menu.x + 10.0;
                y += 28.0;
            }
            types.push(Rect::new(x, y, w, 24.0));
            x += w + 6.0;
        }
        y += 34.0;

        let mut colors = Vec::new();
        x = menu.x + 10.0;
        for chip in &self.color_chips {
            let (w, _) = chip.preferred_size();
            colors.push(Rect::new(x, y, 30.0, 24.0));
            x += w + 6.0;
        }
        y += 34.0;

        let mut cmcs = Vec::new();
        x = menu.x + 10.0;
        for chip in &self.cmc_chips {
            let (w, _) = chip.preferred_size();
            cmcs.push(Rect::new(x, y, w, 24.0));
            x += w + 6.0;
        }
        y += 30.0;

        let mut rarities = Vec::new();
        if self.show_rarity {
            x = menu.x + 10.0;
            for chip in &self.rarity_chips {
                let (w, _) = chip.preferred_size();
                rarities.push(Rect::new(x, y, w, 24.0));
                x += w + 6.0;
            }
            y += 34.0;
        }

        let clear = Rect::new(menu.x + 10.0, y, 140.0, 32.0);
        ChipRects {
            types,
            colors,
            cmcs,
            rarities,
            clear,
        }
    }

    /// Draw the toggle button at `toggle_rect` and, when open, the menu.
    /// The menu renders into `overlay` (it floats over content).
    pub fn render(
        &mut self,
        c: &mut Compositor,
        toggle_rect: Rect,
        overlay: LayerId,
        theme: &Theme,
    ) {
        self.sync_chips();
        // Toggle button: icon in a small pill; a badge dot when active.
        let active = self.state.is_active();
        c.push(rounded_rect(
            toggle_rect.x,
            toggle_rect.y,
            toggle_rect.w,
            toggle_rect.h,
            theme.radius.md,
            if self.open || active {
                theme.glass.surface_active.0
            } else {
                theme.glass.surface.0
            },
        ));
        if let Some(node) = icons::icon_at(
            "filter",
            16.0,
            theme.colors.text.0,
            toggle_rect.x + 12.0,
            toggle_rect.y + 12.0,
        ) {
            c.push(node);
        }
        if active {
            c.push(rounded_rect(
                toggle_rect.x + toggle_rect.w - 12.0,
                toggle_rect.y + 4.0,
                8.0,
                8.0,
                4.0,
                theme.colors.accent.0,
            ));
        }

        if !self.open {
            return;
        }
        let menu = self.menu_rect(toggle_rect);
        c.push_to_layer(overlay, menu_shadow(menu, theme.radius.lg));
        for node in glass_pill(
            menu,
            theme.radius.lg,
            theme.glass.edge_soft.0,
            1.5,
            theme.glass.popover.0,
        ) {
            c.push_to_layer(overlay, node);
        }

        let rects = self.chip_rects(menu);
        let group_label_c = theme.glass.text_placeholder.0;
        macro_rules! group {
            ($label:expr, $x:expr, $y:expr) => {
                text(c, $label, 10.0, 600, $x, $y - 12.0, group_label_c);
            };
        }
        group!("Tipo", menu.x + 10.0, rects.types[0].y);
        for (i, rect) in rects.types.iter().enumerate() {
            self.type_chips[i].render(c, *rect, theme);
        }
        group!("Cor", menu.x + 10.0, rects.colors[0].y);
        for (i, rect) in rects.colors.iter().enumerate() {
            let (letter, _) = FILTER_COLORS[i];
            let active = self.state.colors.contains(&letter);
            let bg = crate::view::mana::pip_color(letter);
            let alpha = if active { 1.0 } else { 0.35 };
            c.push_to_layer(overlay, engine::ui::widgets::rounded_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                theme.radius.md,
                [
                    bg[0],
                    bg[1],
                    bg[2],
                    alpha,
                ],
            ));
            text(
                c,
                &letter.to_string(),
                11.0,
                600,
                rect.x + rect.w / 2.0 - 4.0,
                rect.y + 5.0,
                [0.10, 0.10, 0.12, 1.0],
            );
        }
        group!("CMC", menu.x + 10.0, rects.cmcs[0].y);
        for (i, rect) in rects.cmcs.iter().enumerate() {
            self.cmc_chips[i].render(c, *rect, theme);
        }
        if self.show_rarity && !rects.rarities.is_empty() {
            group!("Raridade", menu.x + 10.0, rects.rarities[0].y);
            for (i, rect) in rects.rarities.iter().enumerate() {
                self.rarity_chips[i].render(c, *rect, theme);
            }
        }
        if self.state.is_active() {
            self.clear_btn.render(c, rects.clear, theme);
        }
    }
}

struct ChipRects {
    types: Vec<Rect>,
    colors: Vec<Rect>,
    cmcs: Vec<Rect>,
    rarities: Vec<Rect>,
    clear: Rect,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Card {
        name: &'static str,
        type_line: Option<&'static str>,
        colors: Option<&'static str>,
        cmc: Option<f64>,
        rarity: Option<&'static str>,
    }

    impl FilterCard for Card {
        fn filter_name(&self) -> &str {
            self.name
        }
        fn filter_type_line(&self) -> Option<&str> {
            self.type_line
        }
        fn filter_colors(&self) -> Option<&str> {
            self.colors
        }
        fn filter_cmc(&self) -> Option<f64> {
            self.cmc
        }
        fn filter_rarity(&self) -> Option<&str> {
            self.rarity
        }
    }

    fn card(name: &'static str, t: &'static str, colors: &'static str, cmc: f64) -> Card {
        Card {
            name,
            type_line: Some(t),
            colors: Some(colors),
            cmc: Some(cmc),
            rarity: Some("rare"),
        }
    }

    #[test]
    fn toggles_add_and_remove() {
        let mut f = FilterState::default();
        f.toggle_type("Creature");
        f.toggle_type("Artifact");
        f.toggle_type("Creature");
        assert_eq!(f.types, vec!["Artifact"]);
    }

    #[test]
    fn color_filter_matches_colorless_with_c() {
        let mut f = FilterState::default();
        f.toggle_color('C');
        let colorless = card("Sol Ring", "Artifact", "", 1.0);
        let colored = card("Giant", "Creature", "R", 3.0);
        assert!(matches_filters(&colorless, &f));
        assert!(!matches_filters(&colored, &f));
    }

    #[test]
    fn type_filter_uses_category_extraction() {
        let mut f = FilterState::default();
        f.toggle_type("Creature");
        let creature = card("Giant", "Creature — Giant", "R", 3.0);
        let artifact = card("Sol Ring", "Artifact", "", 1.0);
        assert!(matches_filters(&creature, &f));
        assert!(!matches_filters(&artifact, &f));
    }

    #[test]
    fn cmc_buckets_collapse_above_five() {
        assert_eq!(card_cmc_bucket(Some(0.0)), "0");
        assert_eq!(card_cmc_bucket(Some(5.0)), "5");
        assert_eq!(card_cmc_bucket(Some(9.0)), "5");
    }

    #[test]
    fn search_is_case_insensitive_substring() {
        let mut f = FilterState::default();
        f.q = "sol".into();
        assert!(matches_filters(&card("Sol Ring", "Artifact", "", 1.0), &f));
        assert!(!matches_filters(&card("Giant", "Creature", "R", 3.0), &f));
    }

    #[test]
    fn active_flag_tracks_any_dimension() {
        let mut f = FilterState::default();
        assert!(!f.is_active());
        f.toggle_cmc("2");
        assert!(f.is_active());
        f.clear();
        assert!(!f.is_active());
    }
}
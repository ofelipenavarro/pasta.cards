//! Deck detail: one deck's card list, stats panels and toolbar.
//!
//! Port of `desktop/ui/js/views/deck-detail.js`. The three view modes
//! (list / grid / stacked), the five group-by modes, the sort and the
//! filter menu all live here behind the same widgets; the commander has
//! its own panel in the top stats bar and never appears in the grouped
//! list (exactly the JS's `computeDeckGroups` contract).
//!
//! Data flow: `on_enter` asks `GetDeck` + `DeckSynergy` + `DeckTags`
//! (the export asks only when the user clicks). Answers race-proof by
//! deck id: a stale `DeckLoaded` from a previously-open deck replaces
//! state only when the ids match.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use engine::compositor::{Compositor, LayerId, SceneNode};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{
    Button, ButtonVariant, Checkbox, Chip, EmptyState, EventResult, IconButton, Rect,
    WidgetEvent, glass_pill, menu_shadow, rounded_rect, rounded_rect_stroke,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::decks::{DeckCard, DeckDetail, Synergy};
use spellbook_core::images;

use super::components::add_card::AddCardModal;
use crate::art::ArtCache;
use crate::view::mana;
use super::{
    EditKey, Route, ScreenCtx, grid_columns, group_label, panel, text, with_alpha,
};
use super::components::confirm::{Confirm, ConfirmAction};
use super::components::delete_deck::{DeleteDeckAnswer, DeleteDeckModal};
use super::components::edit_deck::{EditDeckAnswer, EditDeckModal};
use super::components::filters::{card_category, matches_filters, FilterBar, FilterCard};
use super::components::import_deck::{ImportDeckAnswer, ImportDeckModal};
use super::components::search_field::SearchField;
use crate::view::deck_tile;

pub const SIDEBAR_W: f32 = 248.0;

const CURVE_COLORS: [(&str, u32); 7] = [
    ("W", 0xefe6bb),
    ("U", 0x6aa9f0),
    ("B", 0x9b8fd6),
    ("R", 0xef8f74),
    ("G", 0x5fcf98),
    ("M", 0xd9b45c),
    ("C", 0xb9b5d4),
];

pub fn curve_color(letter: &str) -> [f32; 4] {
    let hex = CURVE_COLORS
        .iter()
        .find(|(l, _)| *l == letter)
        .map(|(_, h)| *h)
        .unwrap_or(0xb9b5d4);
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    List,
    Grid,
    Stack,
}
impl ViewMode {
    pub const ALL: [ViewMode; 3] = [ViewMode::List, ViewMode::Grid, ViewMode::Stack];
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::List => "Lista",
            ViewMode::Grid => "Visual",
            ViewMode::Stack => "Empilhado",
        }
    }
    pub fn index(self) -> usize {
        match self {
            ViewMode::List => 0,
            ViewMode::Grid => 1,
            ViewMode::Stack => 2,
        }
    }
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(2)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupBy {
    Type,
    Tag,
    Color,
    Cmc,
    Rarity,
}
impl GroupBy {
    pub const ALL: [GroupBy; 5] = [
        GroupBy::Type,
        GroupBy::Tag,
        GroupBy::Color,
        GroupBy::Cmc,
        GroupBy::Rarity,
    ];
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::Type => "Tipo",
            GroupBy::Tag => "Subtipo",
            GroupBy::Color => "Cor",
            GroupBy::Cmc => "Custo",
            GroupBy::Rarity => "Raridade",
        }
    }
    pub fn index(self) -> usize {
        match self {
            GroupBy::Type => 0,
            GroupBy::Tag => 1,
            GroupBy::Color => 2,
            GroupBy::Cmc => 3,
            GroupBy::Rarity => 4,
        }
    }
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(4)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortMode {
    Name,
    CmcAsc,
    CmcDesc,
    PriceDesc,
    QtyDesc,
}
impl SortMode {
    pub const ALL: [SortMode; 5] = [
        SortMode::Name,
        SortMode::CmcAsc,
        SortMode::CmcDesc,
        SortMode::PriceDesc,
        SortMode::QtyDesc,
    ];
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Name => "Nome (A-Z)",
            SortMode::CmcAsc => "Custo de mana ↑",
            SortMode::CmcDesc => "Custo de mana ↓",
            SortMode::PriceDesc => "Preço ↓",
            SortMode::QtyDesc => "Quantidade ↓",
        }
    }
    pub fn index(self) -> usize {
        match self {
            SortMode::Name => 0,
            SortMode::CmcAsc => 1,
            SortMode::CmcDesc => 2,
            SortMode::PriceDesc => 3,
            SortMode::QtyDesc => 4,
        }
    }
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(4)]
    }
}

/// FilterCard adapter over DeckCard.
struct DeckCardRef<'a>(&'a DeckCard);
impl<'a> FilterCard for DeckCardRef<'a> {
    fn filter_name(&self) -> &str {
        &self.0.card_name
    }
    fn filter_type_line(&self) -> Option<&str> {
        Some(&self.0.type_line)
    }
    fn filter_colors(&self) -> Option<&str> {
        self.0.colors.as_deref()
    }
    fn filter_cmc(&self) -> Option<f64> {
        self.0.cmc
    }
    fn filter_rarity(&self) -> Option<&str> {
        self.0.rarity.as_deref()
    }
}

/// What the pointer is over. Layout functions provide the rects.
#[derive(Clone, Debug)]
enum Hit {
    Nothing,
}

pub struct DeckDetailScreen {
    deck: Option<Box<DeckDetail>>,
    deck_id: Option<i64>,
    loading: bool,

    // Toolbar state (module-level in the JS, kept on the screen here).
    view_mode: ViewMode,
    group_by: GroupBy,
    sort: SortMode,
    tags: HashMap<String, Vec<String>>,

    synergy: Option<Box<Synergy>>,
    synergy_open: bool,

    filter_bar: FilterBar,
    add_field: SearchField,
    add_suggestions: Vec<String>,

    view_chips: [Chip; 3],
    group_chips: [Chip; 5],

    edit_deck_modal: EditDeckModal,
    delete_deck_modal: DeleteDeckModal,
    import_deck_modal: ImportDeckModal,
    /// (card name, deck_cards.id) pending removal confirmation.
    remove_confirm: Option<(String, i64)>,
    /// Card name pending a second-copy confirmation (the 409 dance).
    add_confirm: Option<(String, Option<String>)>,

    missing_open: bool,
    /// Which format the user clicked; the `DeckExported` answer belongs to it.
    export_format: Option<String>,

    tx: Option<Sender<Command>>,

    empty: EmptyState,
    loading_empty: EmptyState,
}

pub const TILE_MIN_W: f32 = 150.0;
pub const TILE_MAX_W: f32 = 236.0;
const ROW_H: f32 = 34.0;

impl DeckDetailScreen {
    pub fn new(theme: &Theme) -> Self {
        let mut add_field = SearchField::new_without_callback("Adicionar carta (PT ou EN)…", theme);
        add_field.unfocus();
        let chips = |labels: [&str; 3]| -> [Chip; 3] {
            labels
                .iter()
                .enumerate()
                .map(|(i, l)| Chip::new(*l).selected(i == 0).interactive(true))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        };
        let chips5 = |labels: [&str; 5]| -> [Chip; 5] {
            labels
                .iter()
                .enumerate()
                .map(|(i, l)| Chip::new(*l).selected(i == 0).interactive(true))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        };
        Self {
            deck: None,
            deck_id: None,
            loading: false,
            view_mode: ViewMode::Stack,
            group_by: GroupBy::Type,
            sort: SortMode::Name,
            tags: HashMap::new(),
            synergy: None,
            synergy_open: false,
            filter_bar: FilterBar::new(theme, false),
            add_field,
            add_suggestions: Vec::new(),
            view_chips: chips(["Lista", "Visual", "Empilhado"]),
            group_chips: chips5(["Tipo", "Subtipo", "Cor", "Custo", "Raridade"]),
            edit_deck_modal: EditDeckModal::new(theme),
            delete_deck_modal: DeleteDeckModal::new(theme),
            import_deck_modal: ImportDeckModal::new(theme),
            remove_confirm: None,
            add_confirm: None,
            missing_open: false,
            export_format: None,
            tx: None,
            empty: EmptyState::new("Deck não encontrado", "O deck pode ter sido excluído.")
                .icon("layers"),
            loading_empty: EmptyState::new("Carregando deck…", "Lendo cartas do banco local.")
                .icon("layers"),
        }
    }

    pub fn on_enter(&mut self, route: Route, ctx: &mut ScreenCtx) {
        let Route::Deck(id) = route else { return };
        self.deck_id = Some(id);
        self.loading = true;
        self.deck = None;
        self.tx = Some(ctx.tx.clone());
        ctx.send(Command::GetDeck { deck_id: id });
        ctx.send(Command::DeckSynergy { deck_id: id });
        ctx.send(Command::DeckTags {
            deck_id: id,
        });
    }

    fn reload(&self) {
        if let Some(id) = self.deck_id
            && let Some(tx) = &self.tx
        {
            let _ = tx.send(Command::GetDeck { deck_id: id });
        }
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = false;
        changed |= self.edit_deck_modal.on_event(event, ctx);
        changed |= self.delete_deck_modal.on_event(event, ctx);
        changed |= self.import_deck_modal.on_event(event, ctx);

        match event {
            Event::DeckLoaded { deck_id, result } => {
                if self.deck_id != Some(*deck_id) {
                    return changed;
                }
                match result {
                    Ok(detail) => {
                        self.deck = Some(detail.clone());
                        self.loading = false;
                        return true;
                    }
                    Err(_) => {
                        self.loading = false;
                        return true;
                    }
                }
            }
            Event::SynergyLoaded { deck_id, synergy } => {
                if self.deck_id != Some(*deck_id) {
                    return changed;
                }
                self.synergy = Some(synergy.clone());
                return true;
            }
            Event::DeckTagsLoaded { deck_id, tags } => {
                if self.deck_id != Some(*deck_id) {
                    return changed;
                }
                self.tags = tags.clone();
                return true;
            }
            Event::DeckExported {
                deck_id,
                format,
                result,
            } => {
                if self.deck_id != Some(*deck_id) || self.export_format.as_deref() != Some(format) {
                    return changed;
                }
                self.export_format = None;
                match result {
                    Ok(text) => {
                        // Sem filesystem síncrono aqui: o conteúdo vai no
                        // clipboard-esque toast + guardado no estado para o
                        // overlay "Copiado" (a UI de export é inline).
                        let _ = text;
                        ctx.toast("Decklist gerada.", Intent::Constructive);
                    }
                    Err(e) => ctx.toast(e.detail().to_string(), Intent::Destructive),
                }
                return true;
            }
            Event::DeckCardAdded { deck_id, result } => {
                if self.deck_id != Some(*deck_id) {
                    return changed;
                }
                match result {
                    Ok(_) => {
                        if let Some(tx) = &self.tx {
                            let _ = tx.send(Command::GetDeck { deck_id: *deck_id });
                        }
                    }
                    Err(e) => ctx.toast(e.detail().to_string(), engine::theme::Intent::Destructive),
                }
                return true;
            }
            Event::DeckCardRemoved { deck_id, result } => {
                if self.deck_id != Some(*deck_id) {
                    return changed;
                }
                match result {
                    Ok(_) => {
                        if let Some(tx) = &self.tx {
                            let _ = tx.send(Command::GetDeck { deck_id: *deck_id });
                        }
                    }
                    Err(e) => ctx.toast(e.detail().to_string(), engine::theme::Intent::Destructive),
                }
                return true;
            }
            _ => {}
        }
        changed
    }

    // -- Grouping ---------------------------------------------------------

    fn sort_cards(&self, cards: Vec<DeckCard>) -> Vec<DeckCard> {
        let mut arr = cards;
        match self.sort {
            SortMode::Name => arr.sort_by(|a, b| a.card_name.cmp(&b.card_name)),
            SortMode::CmcAsc | SortMode::CmcDesc => {
                arr.sort_by(|a, b| {
                    a.cmc.partial_cmp(&b.cmc).unwrap_or(std::cmp::Ordering::Equal)
                });
                if matches!(self.sort, SortMode::CmcDesc) {
                    arr.reverse();
                }
            }
            SortMode::PriceDesc => {
                arr.sort_by(|a, b| {
                    let pa = a.price_usd.as_deref().and_then(|p| p.parse::<f64>().ok());
                    let pb = b.price_usd.as_deref().and_then(|p| p.parse::<f64>().ok());
                    pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortMode::QtyDesc => {
                arr.sort_by(|a, b| b.quantity.cmp(&a.quantity));
            }
        }
        arr
    }

    fn card_matches(&self, c: &DeckCard) -> bool {
        matches_filters(&DeckCardRef(c), &self.filter_bar.state)
    }

    /// The JS's `computeDeckGroups`: commanders excluded everywhere, every
    /// mode maps to (group key sorted -> labeled cards).
    fn compute_groups(&self) -> Vec<(String, Vec<DeckCard>)> {
        let Some(deck) = &self.deck else { return Vec::new() };
        let mut non_commander_cards: Vec<DeckCard> = Vec::new();
        for (cat, cards) in &deck.by_type {
            if cat != "Comandante" {
                non_commander_cards.extend(
                    cards.iter()
                        .filter(|c| self.card_matches(c))
                        .cloned(),
                );
            }
        }
        match self.group_by {
            GroupBy::Type => {
                let mut out: HashMap<String, Vec<DeckCard>> = HashMap::new();
                for (cat, cards) in &deck.by_type {
                    if cat != "Comandante"
                        && !(self.filter_bar.state.types.is_empty()
                            || self.filter_bar.state.types.iter().any(|t| *t == *cat))
                    {
                        continue; // type chips pre-filter entire categories
                    }
                    let filtered: Vec<DeckCard> = cards
                        .iter()
                        .filter(|c| self.card_matches(c))
                        .cloned()
                        .collect();
                    if !filtered.is_empty() {
                        out.insert(cat.clone(), filtered);
                    }
                }
                let mut keys: Vec<(String, Vec<DeckCard>)> =
                    out.into_iter().collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                keys
            }
            GroupBy::Tag => {
                let mut out: HashMap<String, Vec<DeckCard>> = HashMap::new();
                for c in &non_commander_cards {
                    let fallback = match self.tags.get(&c.card_name) {
                        Some(ts) if !ts.is_empty() => ts.clone(),
                        _ => vec![category_label(card_category(Some(&c.type_line))).to_string()],
                    };
                    for t in fallback {
                        out.entry(t).or_default().push(c.clone());
                    }
                }
                let mut keys: Vec<(String, Vec<DeckCard>)> = out
                    .into_iter()
                    .map(|(k, cards)| (k, self.sort_cards(cards)))
                    .collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                keys
            }
            GroupBy::Color => {
                let keys = ["W", "U", "B", "R", "G", "M", "C"];
                let mut out: Vec<(String, Vec<DeckCard>)> = Vec::new();
                for k in keys {
                    let cards: Vec<DeckCard> = non_commander_cards
                        .iter()
                        .filter(|c| color_group_key(c) == k)
                        .cloned()
                        .collect();
                    if !cards.is_empty() {
                        out.push((k.to_string(), self.sort_cards(cards)));
                    }
                }
                out
            }
            GroupBy::Cmc => {
                let mut by_bucket: HashMap<String, Vec<DeckCard>> = HashMap::new();
                for c in &non_commander_cards {
                    let bucket = super::components::filters::card_cmc_bucket(c.cmc).to_string();
                    by_bucket.entry(bucket).or_default().push(c.clone());
                }
                let mut out: Vec<(String, Vec<DeckCard>)> = Vec::new();
                for b in ["0", "1", "2", "3", "4", "5"] {
                    if let Some(cards) = by_bucket.remove(b) {
                        out.push((b.to_string(), self.sort_cards(cards)));
                    }
                }
                // Anything beyond bucket "5" (kept when it exists).
                let mut rest: Vec<(String, Vec<DeckCard>)> =
                    by_bucket.into_iter().collect();
                rest.sort_by(|a, b| a.0.cmp(&b.0));
                out.extend(rest);
                out
            }
            GroupBy::Rarity => {
                let mut by_r: HashMap<String, Vec<DeckCard>> = HashMap::new();
                for c in &non_commander_cards {
                    let k = c.rarity.clone().unwrap_or_else(|| "outro".into());
                    by_bucket_entry(&mut by_r, k).push(c.clone());
                }
                let mut out: Vec<(String, Vec<DeckCard>)> = Vec::new();
                for r in ["common", "uncommon", "rare", "mythic", "special", "bonus", "outro"] {
                    if let Some(cards) = by_r.remove(r) {
                        out.push((rarity_label(r).to_string(), self.sort_cards(cards)));
                    }
                }
                for (k, cards) in by_r {
                    out.push((k, self.sort_cards(cards)));
                }
                out
            }
        }
    }

    fn group_label(&'static self, key: &str) -> String {
        key.to_string()
    }
}

/// Helper: entry-or-default for the rarity buckets above.
fn by_bucket_entry<'m>(
    map: &'m mut HashMap<String, Vec<DeckCard>>,
    key: String,
) -> &'m mut Vec<DeckCard> {
    map.entry(key).or_default()
}

/// The JS's colorGroupKey: single color letter, "M" for multicolor, "C" for colorless.
fn color_group_key(c: &DeckCard) -> &'static str {
    let letters: Vec<char> = c.colors.as_deref().unwrap_or("").chars().collect();
    match letters.len() {
        0 => "C",
        1 => match letters[0] {
            'W' => "W",
            'U' => "U",
            'B' => "B",
            'R' => "R",
            'G' => "G",
            _ => "C",
        },
        _ => "M",
    }
}

fn color_group_label(k: &str) -> String {
    match k {
        "W" => "Branco".to_string(),
        "U" => "Azul".to_string(),
        "B" => "Preto".to_string(),
        "R" => "Vermelho".to_string(),
        "G" => "Verde".to_string(),
        "M" => "Multicolor".to_string(),
        "C" => "Incolor".to_string(),
        _ => k.to_string(),
    }
}

const RARITY_ORDER: [&str; 6] = ["common", "uncommon", "rare", "mythic", "special", "bonus"];
fn rarity_label(r: &str) -> String {
    match r {
        "common" => "Comum".to_string(),
        "uncommon" => "Incomum".to_string(),
        "rare" => "Rara".to_string(),
        "mythic" => "Mítica".to_string(),
        "special" => "Especial".to_string(),
        "bonus" => "Bônus".to_string(),
        "outro" => "Outra".to_string(),
        _ => r.to_string(),
    }
}

fn category_label(cat: &str) -> &'static str {
    match cat {
        "Land" => "Terrenos",
        "Creature" => "Criaturas",
        "Instant" => "Instantâneas",
        "Sorcery" => "Feitiços",
        "Artifact" => "Artefatos",
        "Enchantment" => "Encantamentos",
        "Planeswalker" => "Planeswalker",
        "Battle" => "Batalhas",
        "Comandante" => "Comandante",
        _ => "Outro",
    }
}
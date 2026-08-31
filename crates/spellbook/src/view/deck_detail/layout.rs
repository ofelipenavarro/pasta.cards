//! Pure layout for the deck detail screen: stat panels, toolbar rects and
//! the card grid/list, plus the shared types the events side reads.


use engine::ui::widgets::Rect;
use spellbook_core::ops::decks::DeckCard;

use super::DeckDetailScreen;
use super::super::grid_columns;
use super::{CHIP_GAP, CHIP_H, GROUP_LABEL_H, ROW_H, STAT_PANEL_H, TILE_MAX_W, TILE_MIN_W, TOOLBAR_H, ViewMode};

pub struct StatsRects {
    pub commander: Rect,
    pub synergy: Rect,
    pub curve: Rect,
    pub height: f32,
}

pub struct ToolbarRects {
    pub add: Rect,
    pub views: Vec<Rect>,
    pub groups: Vec<Rect>,
    pub sort: Rect,
    pub filter: Rect,
}

/// The four header action buttons, in layout order.
#[derive(Clone, Copy, Default)]
pub struct HeaderRects {
    pub export: Rect,
    pub import: Rect,
    pub edit: Rect,
    pub delete: Rect,
}

/// A hit target laid out this frame: group label, list row, or tile.
pub(crate) enum LayoutHit {
    GroupLabel,
    Row { group: String, idx: usize },
    Tile { group: String, idx: usize },
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

const RARITY_ORDER: [&str; 7] = ["common", "uncommon", "rare", "mythic", "special", "bonus", "outro"];
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


impl DeckDetailScreen {
    /// Content rect of each top stats panel, given the full content rect.
    pub(crate) fn stats_rects(&self, content: Rect) -> StatsRects {
        let panel_h = if self.synergy_open {
            230.0
        } else {
            STAT_PANEL_H
        };
        let y = content.y;
        StatsRects {
            commander: Rect::new(content.x, y, content.w * 0.30, panel_h),
            synergy: Rect::new(content.x + content.w * 0.31, y, content.w * 0.36, panel_h),
            curve: Rect::new(
                content.x + content.w * 0.68,
                y,
                content.w * 0.32,
                panel_h,
            ),
            height: panel_h,
        }
    }

    /// Y of the toolbar row.
    pub(crate) fn toolbar_y(&self, content: Rect) -> f32 {
        content.y + self.stats_rects(content).height + 20.0
    }

    pub(crate) fn toolbar_rect(&self, content: Rect) -> Rect {
        Rect::new(content.x, self.toolbar_y(content), content.w, TOOLBAR_H)
    }

    /// Chip rects across the toolbar, left to right:
    /// [add field][view chips][group chips][sort][filter].
    pub(crate) fn toolbar_rects(&self, content: Rect) -> ToolbarRects {
        let bar = self.toolbar_rect(content);
        let y = bar.y + (TOOLBAR_H - CHIP_H) / 2.0;
        let mut x = bar.x;

        let add = Rect::new(x, y, 210.0, CHIP_H);
        x += add.w + 18.0;

        let mut views = Vec::with_capacity(3);
        for chip in &self.view_chips {
            let (w, _) = chip.preferred_size();
            views.push(Rect::new(x, y, w, CHIP_H));
            x += w + CHIP_GAP;
        }
        x += 14.0;

        let mut groups = Vec::with_capacity(5);
        for chip in &self.group_chips {
            let (w, _) = chip.preferred_size();
            groups.push(Rect::new(x, y, w, CHIP_H));
            x += w + CHIP_GAP;
        }
        x += 14.0;

        let sort_w = 110.0;
        let sort = Rect::new(x, y, sort_w, CHIP_H);
        x += sort_w + CHIP_GAP + 14.0;

        let filter = Rect::new(x, y, crate::view::components::filters::FilterBar::toggle_w(), CHIP_H.max(36.0));

        ToolbarRects {
            add,
            views,
            groups,
            sort,
            filter,
        }
    }

    /// Y of the first group label below the toolbar.
    pub(crate) fn cards_y(&self, content: Rect) -> f32 {
        self.toolbar_y(content) + TOOLBAR_H + 14.0
    }

    pub(crate) fn card_rects(&self, content: Rect) -> Vec<(LayoutHit, Rect)> {
        let groups = self.compute_groups();
        let y0 = self.cards_y(content);
        let mut out: Vec<(LayoutHit, Rect)> = Vec::new();
        let mut y = y0;
        match self.view_mode {
            ViewMode::List => {
                for (gkey, cards) in &groups {
                    let total: i64 = cards.iter().map(|c| c.quantity).sum();
                    out.push((LayoutHit::GroupLabel, Rect::new(content.x, y, content.w, GROUP_LABEL_H)));
                    y += GROUP_LABEL_H;
                    let _ = total;
                    for (i, _c) in cards.iter().enumerate() {
                        out.push((
                            LayoutHit::Row { group: gkey.clone(), idx: i },
                            Rect::new(content.x, y, content.w, ROW_H),
                        ));
                        y += ROW_H + 2.0;
                    }
                    y += 12.0;
                }
            }
            ViewMode::Grid | ViewMode::Stack => {
                let (cols, col_w) =
                    grid_columns(content.w, TILE_MIN_W, TILE_MAX_W, 16.0);
                let tile_h = col_w * (680.0 / 488.0) + 56.0;
                for (gkey, cards) in &groups {
                    let total: i64 = cards.iter().map(|c| c.quantity).sum();
                    let _ = total;
                    out.push((LayoutHit::GroupLabel, Rect::new(content.x, y, content.w, GROUP_LABEL_H)));
                    y += GROUP_LABEL_H;
                    for (i, _c) in cards.iter().enumerate() {
                        let (row, col) = (i / cols, i % cols);
                        out.push((
                            LayoutHit::Tile { group: gkey.clone(), idx: i },
                            Rect::new(
                                content.x + col as f32 * (col_w + 16.0),
                                y + row as f32 * (tile_h + 16.0),
                                col_w,
                                tile_h,
                            ),
                        ));
                    }
                    let rows = cards.len().div_ceil(cols) as f32;
                    y += rows * (tile_h + 16.0) + 12.0;
                }
            }
        }
        out
    }

    pub fn content_height(&self, content: Rect) -> f32 {
        if self.deck.is_none() && self.loading {
            return content.h;
        }
        let rects = self.card_rects(content);
        // Group labels are part of the list; the last tile's bottom is enough.
        let bottom = rects
            .last()
            .map(|(_, r)| r.y + r.h)
            .unwrap_or(self.cards_y(content));
        (bottom + 24.0 - content.y).max(content.h)
    }

    // -- Pointer / text ------------------------------------------------------

    pub fn overlay_open(&self) -> bool {
        self.edit_deck_modal.is_open()
            || self.delete_deck_modal.is_open()
            || self.import_deck_modal.is_open()
            || self.remove_confirm.is_some()
            || self.add_confirm.is_some()
            || self.filter_bar.is_open()
            || self.export_menu_open
            || self.sort_menu_open
    }
}

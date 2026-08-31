//! Drawing for the deck detail screen: header, stats panels, toolbar and
//! the three card-list renderings.

use std::collections::HashMap;

use engine::compositor::{Compositor, LayerId, SceneNode};
use engine::theme::Theme;
use engine::ui::widgets::{Rect, glass_pill, menu_shadow, rounded_rect, rounded_rect_stroke};
use spellbook_core::client::Command;
use spellbook_core::ops::decks::{DeckCard, DeckDetail};
use spellbook_core::images;

use super::super::text;
use super::super::{group_label, panel, with_alpha};
use super::super::mana;
use super::DeckDetailScreen;
use super::layout::{HeaderRects, LayoutHit, StatsRects};
use super::super::{EditKey, ScreenCtx, text as core_text};
use super::{GroupBy, SortMode, ViewMode, curve_color, ROW_H};
use super::{CHIP_H, STAT_PANEL_H};
use super::events::{confirm_buttons, ownership_tag};
use crate::art::ArtCache;
use crate::view::components::confirm::render_confirm_dialog;

impl DeckDetailScreen {

    pub fn render(
        &mut self,
        c: &mut Compositor,
        _layer: LayerId,
        content: Rect,
        theme: &Theme,
        art: &mut ArtCache,
    ) {
        if self.deck.is_none() {
            if self.loading {
                self.loading_empty.render(c, content, theme);
            } else {
                self.empty.render(c, content, theme);
            }
            return;
        }
        let deck = self.deck.as_ref().expect("checked above");

        // Breadcrumb.
        text(
            c,
            "Meus Decks",
            12.0,
            500,
            content.x,
            content.y,
            theme.glass.text_placeholder.0,
        );
        text(c, "/", 12.0, 400, content.x + 66.0, content.y, theme.glass.text_faint.0);
        text(
            c,
            &deck.name,
            12.0,
            600,
            content.x + 76.0,
            content.y,
            theme.colors.text.0,
        );
        let header_y = content.y + 22.0;
        let header_h = 62.0;

        // Header: name + philosophy + tags, then pill + buttons right.
        text(c, &deck.name, 26.0, 700, content.x, header_y, theme.colors.text.0);
        if let Some(philosophy) = deck.philosophy.as_deref().filter(|p| !p.is_empty()) {
            text(
                c,
                philosophy,
                12.0,
                400,
                content.x,
                header_y + 36.0,
                theme.colors.text_dim.0,
            );
        }
        if let Some(tags) = deck.tags.as_deref().filter(|t| !t.is_empty()) {
            let mut tx = content.x + deck.name.len() as f32 * 16.0 + 24.0;
            for tag in tags.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                let w = tag.len() as f32 * 6.4 + 16.0;
                c.push(rounded_rect(
                    tx,
                    header_y + 6.0,
                    w,
                    20.0,
                    10.0,
                    theme.glass.surface_active.0,
                ));
                text(c, tag, 10.0, 600, tx + 8.0, header_y + 10.0, theme.colors.text.0);
                tx += w + 8.0;
            }
        }

        // 100/100 pill.
        let pill_label = format!("{}/100", deck.total_cards);
        let pill_color = if deck.is_valid_100 {
            theme.colors.success.0
        } else {
            theme.colors.danger.0
        };
        let pill_w = 84.0;
        let pill_x = content.x + content.w - pill_w - 320.0;
        c.push(rounded_rect(pill_x, header_y + 8.0, pill_w, 30.0, 15.0, with_alpha(pill_color, 0.18)));
        text(
            c,
            &pill_label,
            14.0,
            700,
            pill_x + 16.0,
            header_y + 14.0,
            pill_color,
        );

        // Header right: Exportar / Importar / editar / excluir.
        let export_r = Rect::new(pill_x + pill_w + 16.0, header_y + 6.0, 100.0, 38.0);
        c.push(rounded_rect(export_r.x, export_r.y, export_r.w, export_r.h, 12.0, theme.glass.surface.0));
        c.push(rounded_rect_stroke(export_r.x, export_r.y, export_r.w, export_r.h, 12.0, theme.glass.edge_soft.0, 1.0));
        text(c, "Exportar ▾", 13.0, 500, export_r.x + 14.0, export_r.y + 10.0, theme.colors.text.0);

        let import_r = Rect::new(export_r.x + export_r.w + 10.0, header_y + 6.0, 150.0, 38.0);
        c.push(rounded_rect(import_r.x, import_r.y, import_r.w, import_r.h, 12.0, theme.glass.surface.0));
        c.push(rounded_rect_stroke(import_r.x, import_r.y, import_r.w, import_r.h, 12.0, theme.glass.edge_soft.0, 1.0));
        text(c, "Importar decklist", 13.0, 500, import_r.x + 14.0, import_r.y + 10.0, theme.colors.text.0);

        let edit_r = Rect::new(import_r.x + import_r.w + 10.0, header_y + 6.0, 44.0, 38.0);
        c.push(rounded_rect(edit_r.x, edit_r.y, edit_r.w, edit_r.h, 12.0, theme.glass.surface.0));
        text(c, "✎", 15.0, 600, edit_r.x + 14.0, edit_r.y + 8.0, theme.colors.text.0);

        let del_r = Rect::new(edit_r.x + edit_r.w + 8.0, header_y + 6.0, 44.0, 38.0);
        c.push(rounded_rect(del_r.x, del_r.y, del_r.w, del_r.h, 12.0, with_alpha(theme.colors.danger.0, 0.16)));
        text(c, "🗑", 15.0, 600, del_r.x + 14.0, del_r.y + 8.0, theme.colors.danger.0);

        // Remember header buttons for pointer routing.
        self.header_rects = HeaderRects { export: export_r, import: import_r, edit: edit_r, delete: del_r };

        // Export dropdown.
        if self.export_menu_open {
            let menu_r = Rect::new(export_r.x, export_r.y + export_r.h + 4.0, 220.0, 78.0);
            c.push(menu_shadow(menu_r, theme.radius.lg));
            for node in glass_pill(menu_r, theme.radius.lg, theme.glass.edge_soft.0, 1.5, theme.glass.popover.0) {
                c.push(node);
            }
            text(c, "Formato Moxfield (.txt)", 12.0, 500, menu_r.x + 12.0, menu_r.y + 12.0, theme.colors.text.0);
            text(c, "Texto simples (.txt)", 12.0, 500, menu_r.x + 12.0, menu_r.y + 46.0, theme.colors.text.0);
        }

        // Sort dropdown.
        if self.sort_menu_open {
            let t = self.toolbar_rects(content);
            let menu_r = self.sort_menu_rect(content);
            c.push(menu_shadow(menu_r, theme.radius.lg));
            for node in glass_pill(menu_r, theme.radius.lg, theme.glass.edge_soft.0, 1.5, theme.glass.popover.0) {
                c.push(node);
            }
            for (i, mode) in SortMode::ALL.iter().enumerate() {
                let ty = menu_r.y + 8.0 + i as f32 * 34.0;
                let is_sel = *mode == self.sort;
                text(
                    c,
                    mode.label(),
                    12.0,
                    if is_sel { 700 } else { 400 },
                    t.sort.x + 16.0,
                    ty,
                    if is_sel { theme.colors.text.0 } else { theme.colors.text_dim.0 },
                );
            }
        }
        let _ = self.header_rects; // reserved for pointer routing below

        // Overage banner.
        let overage = deck.total_cards - 100;
        let mut banner_y = header_y + header_h - 18.0;
        if overage != 0 {
            let (msg, col) = if overage > 0 {
                (
                    format!("Deck com {overage} carta(s) além do limite — remova antes de continuar."),
                    theme.colors.danger.0,
                )
            } else {
                (
                    format!("Faltam {} carta(s) para fechar os 100.", -overage),
                    theme.colors.warning.0,
                )
            };
            let banner_r = Rect::new(content.x, banner_y, content.w, 34.0);
            c.push(rounded_rect(banner_r.x, banner_r.y, banner_r.w, banner_r.h, 10.0, with_alpha(col, 0.12)));
            c.push(rounded_rect_stroke(banner_r.x, banner_r.y, banner_r.w, banner_r.h, 10.0, with_alpha(col, 0.5), 1.0));
            text(c, &msg, 12.0, 500, banner_r.x + 14.0, banner_r.y + 10.0, col);
            banner_y += 44.0;
        }

        // Ownership summary.
        if let Some(summary) = self.ownership_summary(deck) {
            let r = Rect::new(content.x, banner_y, content.w, 44.0);
            c.push(rounded_rect(r.x, r.y, r.w, r.h, 10.0, theme.glass.surface.0));
            c.push(rounded_rect_stroke(r.x, r.y, r.w, r.h, 10.0, theme.glass.edge_soft.0, 1.0));
            text(c, &summary, 12.0, 500, r.x + 14.0, r.y + 14.0, theme.colors.text_dim.0);
        }

        self.render_stats(c, content, theme, art);
        self.render_toolbar(c, content);
        self.render_cards(c, content, theme, art);
    }

    fn ownership_summary(&self, deck: &DeckDetail) -> Option<String> {
        let missing: Vec<&String> = deck
            .ownership
            .iter()
            .filter(|(_, o)| o.status == spellbook_core::wizard::OwnershipStatus::Missing)
            .map(|(n, _)| n)
            .collect();
        let borrowed: Vec<&String> = deck
            .ownership
            .iter()
            .filter(|(_, o)| o.status == spellbook_core::wizard::OwnershipStatus::OwnedInDeck)
            .map(|(n, _)| n)
            .collect();
        match (missing.is_empty(), borrowed.is_empty()) {
            (true, true) => None,
            _ => {
                let mut parts = Vec::new();
                if !missing.is_empty() {
                    parts.push(format!("{} carta(s) que você não tem na coleção", missing.len()));
                }
                if !borrowed.is_empty() {
                    parts.push(format!(
                        "{} carta(s) em outro deck — usar aqui significa desmontar",
                        borrowed.len()
                    ));
                }
                Some(parts.join("  ·  "))
            }
        }
    }

    fn render_stats(&mut self, c: &mut Compositor, content: Rect, theme: &Theme, art: &mut ArtCache) {
        let s = self.stats_rects(content);
        let Some(deck) = &self.deck else { return };

        // Commander panel.
        panel(c, s.commander, theme);
        let commanders: Vec<&DeckCard> = deck
            .by_type
            .get("Comandante")
            .map(|v| v.iter().collect())
            .unwrap_or_default();
        let mut cy = s.commander.y + 14.0;
        for cmd in commanders.iter() {
            let rel = cmd
                .image
                .as_ref()
                .and_then(|i| i.rel.as_deref())
                .map(|r| images::with_variant(r, "normal"));
            match rel.as_deref().and_then(|r| art.get(r)) {
                Some(handle) => {
                    // Full art panel, like the JS's commander art.
                    let art_w = 72.0;
                    let art_h = art_w * 680.0 / 488.0;
                    let art_rect = Rect::new(s.commander.x + 14.0, cy, art_w, art_h);
                    c.push(SceneNode::Image {
                        x: art_rect.x,
                        y: art_rect.y,
                        w: art_rect.w,
                        h: art_rect.h,
                        image: handle,
                        corner_radius: theme.radius.md,
                    });
                    text(c, &cmd.card_name, 14.0, 700, s.commander.x + 14.0 + art_w + 12.0, cy + 8.0, theme.colors.accent.0);
                    cy += art_h.max(48.0);
                }
                None => {
                    text(c, &cmd.card_name, 14.0, 700, s.commander.x + 14.0, cy, theme.colors.text.0);
                    cy += 24.0;
                }
            }
            if let Some(tl) = Some(cmd.type_line.clone())
                && !tl.is_empty()
            {
                text(c, &tl, 11.0, 400, s.commander.x + 14.0, cy, theme.colors.text_dim.0);
                cy += 18.0;
            }
            if let Some(cost) = &cmd.mana_cost {
                mana::render_mana_cost(c, cost, s.commander.x + 14.0, cy + 4.0, theme);
                cy += 26.0;
            }
        }

        // Synergy / related cards panel.
        panel(c, s.synergy, theme);
        text(
            c,
            "Cards relacionados",
            13.0,
            600,
            s.synergy.x + 14.0,
            s.synergy.y + 12.0,
            theme.colors.text.0,
        );
        let syn = self.synergy.clone();
        match syn {
            None => {
                text(
                    c,
                    "Sinergia não conhecida.",
                    12.0,
                    400,
                    s.synergy.x + 14.0,
                    s.synergy.y + 40.0,
                    theme.colors.text_dim.0,
                );
            }
            Some(synergy) if !synergy.cached => {
                text(
                    c,
                    "Sem cache do EDHREC para este comandante ainda.",
                    12.0,
                    400,
                    s.synergy.x + 14.0,
                    s.synergy.y + 40.0,
                    theme.colors.text_dim.0,
                );
                let btn = Rect::new(s.synergy.x + 14.0, s.synergy.y + 70.0, 200.0, 38.0);
                c.push(rounded_rect(btn.x, btn.y, btn.w, btn.h, 12.0, theme.colors.accent.0));
                text(c, "Buscar sinergia agora", 12.0, 600, btn.x + 16.0, btn.y + 12.0, [0.06, 0.06, 0.1, 1.0]);
            }
            Some(synergy) => {
                let mut sy = s.synergy.y + 40.0;
                for rec in synergy.recommendations.iter().take(6) {
                    let name = rec.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let score = rec.get("synergy").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let decks = rec.get("num_decks").and_then(|v| v.as_i64()).unwrap_or(0);
                    text(c, name, 12.0, 500, s.synergy.x + 14.0, sy, theme.colors.text.0);
                    let meta = format!("sinergia {score:+.2} · {} decks", decks);
                    text(
                        c,
                        &meta,
                        10.0,
                        400,
                        s.synergy.x + 14.0,
                        sy + 16.0,
                        theme.colors.text_dim.0,
                    );
                    let add = Rect::new(s.synergy.x + s.synergy.w - 40.0, sy, 30.0, 30.0);
                    c.push(rounded_rect(add.x, add.y, add.w, add.h, 8.0, theme.glass.surface_active.0));
                    text(c, "+", 15.0, 600, add.x + 10.0, add.y + 4.0, theme.colors.text.0);
                    sy += 38.0;
                }
            }
        }

        // Similar commanders, when synergy is cached.
        if let Some(synergy) = &self.synergy
            && synergy.cached
        {
            let mut sy = s.synergy.y + 40.0 + synergy.recommendations.len().min(6) as f32 * 38.0 + 8.0;
            text(c, "Comandantes parecidos", 12.0, 600, s.synergy.x + 14.0, sy, theme.colors.text.0);
            sy += 22.0;
            for sim in synergy.similar_commanders.iter().take(4) {
                let name = sim.as_str().unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let w = name.len() as f32 * 6.4 + 20.0;
                c.push(rounded_rect(s.synergy.x + 14.0, sy, w, 24.0, 12.0, theme.glass.surface_active.0));
                text(c, name, 11.0, 500, s.synergy.x + 22.0, sy + 6.0, theme.colors.text_dim.0);
                sy += 28.0;
            }
        }

        // Mana curve with per-color segments and legend.
        panel(c, s.curve, theme);
        text(c, "Curva de mana", 13.0, 600, s.curve.x + 14.0, s.curve.y + 12.0, theme.colors.text.0);
        let max = deck.mana_curve.values().copied().max().unwrap_or(1).max(1);
        let curve_x = s.curve.x + 14.0;
        let bars_w = s.curve.w - 28.0;
        let bar_count = 8usize; // 0..=7+
        let bar_w = bars_w / bar_count as f32;
        let curve_base = s.curve.y + s.curve.h - 28.0;
        let bar_x = |i: usize| curve_x + i as f32 * bar_w;
        for i in 0..bar_count {
            let (count, label) = if i < 7 {
                (deck.mana_curve.get(&(i as i64)).copied().unwrap_or(0), format!("{i}"))
            } else {
                let seven_plus: i64 = deck
                    .mana_curve
                    .iter()
                    .filter(|(k, _)| **k >= 7)
                    .map(|(_, v)| v)
                    .sum();
                (seven_plus, "7+".to_string())
            };
            if count > 0 {
                let h = (count as f32 / max as f32).max(0.02) * (curve_base - s.curve.y - 34.0);
                c.push(rounded_rect(
                    bar_x(i),
                    curve_base - h.max(2.0),
                    bar_w - 6.0,
                    h.max(2.0),
                    6.0,
                    theme.glass.surface_active.0,
                ));
            }
            text(
                c,
                &label,
                10.0,
                500,
                bar_x(i) + 4.0,
                curve_base + 8.0,
                theme.colors.text_dim.0,
            );
        }
        // Legend, like the JS's `.curve-legend`. Only colors present in the deck.
        let legend_x = curve_x;
        let legend_y = s.curve.y + s.curve.h - 12.0;
        let mut lx = legend_x;
        let mut seen = std::collections::HashSet::new();
        for cards in deck.by_type.values() {
            for card in cards {
                if let Some(colors) = &card.colors {
                    for color in colors.chars() {
                        if seen.insert(color) {
                            let label = match color {
                                'W' => "Branco",
                                'U' => "Azul",
                                'B' => "Preto",
                                'R' => "Vermelho",
                                'G' => "Verde",
                                _ => "Incolor",
                            };
                            c.push(rounded_rect(lx, legend_y, 8.0, 8.0, 4.0, curve_color(&color.to_string())));
                            text(c, label, 10.0, 400, lx + 12.0, legend_y - 2.0, theme.glass.text_placeholder.0);
                            lx += 12.0 + label.len() as f32 * 6.2 + 14.0;
                        }
                    }
                }
            }
        }
        if seen.is_empty() {
            c.push(rounded_rect(lx, legend_y, 8.0, 8.0, 4.0, curve_color("C")));
            text(c, "Incolor", 10.0, 400, lx + 12.0, legend_y - 2.0, theme.glass.text_placeholder.0);
        }
    }

    fn render_toolbar(&mut self, c: &mut Compositor, content: Rect) {
        let theme = &Theme::hoff();
        let t = self.toolbar_rects(content);

        // View chips.
        for (i, r) in t.views.iter().enumerate() {
            self.view_chips[i].selected = ViewMode::from_index(i) == self.view_mode;
            self.view_chips[i].render(c, *r, theme);
        }
        // Group chips.
        for (i, r) in t.groups.iter().enumerate() {
            self.group_chips[i].selected = GroupBy::from_index(i) == self.group_by;
            self.group_chips[i].render(c, *r, theme);
        }
        // Add field.
        self.add_field.render(c, t.add, theme);
        // Suggestion list under the add field.
        if !self.add_suggestions.is_empty() {
            let suggest = Rect::new(t.add.x, t.add.y + t.add.h + 4.0, t.add.w, 6.0 * 34.0 + 4.0);
            c.push(rounded_rect(
                suggest.x,
                suggest.y,
                suggest.w,
                suggest.h,
                theme.radius.md,
                theme.glass.popover.0,
            ));
            c.push(menu_shadow(suggest, theme.radius.md));
            for (i, name) in self.add_suggestions.iter().enumerate() {
                let row = Rect::new(
                    suggest.x + 4.0,
                    suggest.y + 4.0 + i as f32 * 34.0,
                    suggest.w - 8.0,
                    30.0,
                );
                text(c, name, 12.0, 500, row.x + 8.0, row.y + 7.0, theme.colors.text.0);
            }
        }
        // Sort (single chip showing the current mode, opens the menu above).
        let sort_label = format!("Ordenar: {}", self.sort.label());
        c.push(rounded_rect(t.sort.x, t.sort.y, t.sort.w, CHIP_H, CHIP_H / 2.0, theme.glass.surface.0));
        c.push(rounded_rect_stroke(t.sort.x, t.sort.y, t.sort.w, CHIP_H, CHIP_H / 2.0, theme.glass.edge_soft.0, 1.0));
        text(c, &sort_label, 11.0, 500, t.sort.x + 10.0, t.sort.y + 7.0, theme.colors.text_dim.0);
        // Filter toggle (menu body renders in the overlay pass).
        let layer = c.create_layer(200);
        self.filter_bar.render(c, t.filter, layer, theme);
    }

    fn render_cards(&mut self, c: &mut Compositor, content: Rect, theme: &Theme, art: &mut ArtCache) {
        let groups = self.compute_groups();
        if groups.is_empty() {
            text(
                c,
                "Nenhuma carta corresponde aos filtros atuais.",
                12.0,
                400,
                content.x,
                self.cards_y(content) + 8.0,
                theme.colors.text_dim.0,
            );
            return;
        }
        let rects = self.card_rects(content);
        let mut group_cards_iter = groups.iter();
        for (hit, rect) in rects {
            match hit {
                LayoutHit::GroupLabel => {
                    let (key, cards) = group_cards_iter
                        .next()
                        .expect("layout produces one label per group");
                    let total: i64 = cards.iter().map(|c| c.quantity).sum();
                    text(
                        c,
                        &format!("{} ({})", key.to_uppercase(), total),
                        12.0,
                        700,
                        rect.x,
                        rect.y + 8.0,
                        theme.glass.text_placeholder.0,
                    );
                }
                LayoutHit::Row { .. } => {
                    // Rows render inline in compute order; keep a simple line.
                    self.render_card_row(c, rect, &groups, theme, art, &hit);
                }
                LayoutHit::Tile { .. } => {
                    self.render_card_tile(c, rect, &groups, &hit, theme, art);
                }
            }
        }
    }

    fn card_for<'g>(&self, groups: &'g [(String, Vec<DeckCard>)], hit: &LayoutHit) -> Option<&'g DeckCard> {
        match hit {
            LayoutHit::Row { group, idx } | LayoutHit::Tile { group, idx } => groups
                .iter()
                .find(|(k, _)| *k == *group)
                .and_then(|(_, cards)| cards.get(*idx)),
            LayoutHit::GroupLabel => None,
        }
    }

    fn render_card_row(
        &self,
        c: &mut Compositor,
        rect: Rect,
        groups: &[(String, Vec<DeckCard>)],
        theme: &Theme,
        _art: &mut ArtCache,
        hit: &LayoutHit,
    ) {
        let Some(card) = self.card_for(groups, hit) else { return };
        c.push(rounded_rect(rect.x, rect.y, rect.w, ROW_H, 8.0, theme.glass.surface.0));
        text(c, &format!("{}x", card.quantity), 12.0, 600, rect.x + 12.0, rect.y + 10.0, theme.colors.text.0);
        text(c, &card.card_name, 13.0, 500, rect.x + 44.0, rect.y + 9.0, theme.colors.text.0);
        // Ownership tag.
        if let Some(deck) = &self.deck
            && let Some(o) = deck.ownership.get(&card.card_name)
        {
            let (label, col) = ownership_tag(o);
            let tw = label.len() as f32 * 6.4 + 14.0;
            let tx = rect.x + 44.0 + card.card_name.len() as f32 * 7.2 + 16.0;
            c.push(rounded_rect(tx, rect.y + 8.0, tw, 18.0, 9.0, with_alpha(col, 0.16)));
            text(c, &label, 10.0, 600, tx + 7.0, rect.y + 11.0, col);
        }
        // Mana cost.
        if let Some(cost) = &card.mana_cost {
            mana::render_mana_cost(
                c,
                cost,
                rect.x + rect.w - 120.0,
                rect.y + 8.0,
                theme,
            );
        }
        // Remove button.
        let x_r = Rect::new(rect.x + rect.w - 58.0, rect.y + 5.0, 48.0, ROW_H - 10.0);
        c.push(rounded_rect(x_r.x, x_r.y, x_r.w, x_r.h, 8.0, theme.glass.surface_active.0));
        text(c, "✕", 11.0, 600, x_r.x + 18.0, rect.y + 11.0, theme.colors.text.0);
    }

    fn render_card_tile(
        &self,
        c: &mut Compositor,
        rect: Rect,
        groups: &[(String, Vec<DeckCard>)],
        hit: &LayoutHit,
        theme: &Theme,
        art: &mut ArtCache,
    ) {
        let Some(card) = self.card_for(groups, hit) else { return };
        let art_h = rect.w * (680.0 / 488.0);
        // Tile backing.
        c.push(rounded_rect(rect.x, rect.y, rect.w, rect.h, theme.radius.lg, theme.glass.surface.0));
        c.push(rounded_rect_stroke(rect.x, rect.y, rect.w, rect.h, theme.radius.lg, theme.glass.edge_soft.0, 1.0));
        let rel = card
            .image
            .as_ref()
            .and_then(|i| i.rel.as_deref())
            .map(|r| images::with_variant(r, "normal"));
        match rel.as_deref().and_then(|r| art.get(r)) {
            Some(handle) => {
                c.push(SceneNode::Image {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: art_h,
                    image: handle,
                    corner_radius: theme.radius.md,
                });
            }
            None => {
                c.push(rounded_rect(rect.x, rect.y, rect.w, art_h, theme.radius.md, theme.glass.surface_active.0));
                text(c, &card.card_name, 11.0, 500, rect.x + 8.0, rect.y + art_h / 2.0 - 8.0, theme.colors.text_dim.0);
            }
        }
        // Qty badge.
        let qty = format!("{}x", card.quantity);
        let qw = qty.len() as f32 * 7.5 + 14.0;
        c.push(rounded_rect(rect.x + rect.w - qw - 8.0, rect.y + 8.0, qw, 20.0, 10.0, with_alpha(theme.colors.surface.0, 0.92)));
        text(c, &qty, 11.0, 600, rect.x + rect.w - qw, rect.y + 12.0, theme.colors.text.0);
        // Ownership dot.
        if let Some(deck) = &self.deck
            && let Some(o) = deck.ownership.get(&card.card_name)
        {
            let (_, col) = ownership_tag(o);
            c.push(rounded_rect(rect.x + 8.0, rect.y + 8.0, 12.0, 12.0, 6.0, col));
        }
    }


    pub fn handle_text(&mut self, s: &str, ctx: &mut ScreenCtx) -> bool {
        if let Some((name, _)) = &self.remove_confirm {
            let _ = name;
            return false; // confirm dialogs take no text
        }
        if self.add_confirm.is_some() {
            return false;
        }
        if self.edit_deck_modal.is_open() {
            return self.edit_deck_modal.handle_text(s);
        }
        if self.import_deck_modal.is_open() {
            return self.import_deck_modal.handle_text(s);
        }
        // Add-card inline: debounce the card search.
        let consumed = self.add_field.handle_text(s);
        if consumed {
            self.add_search_dirty = true;
            self.add_search_debounce = 0.25;
        }
        if self.filter_bar.is_open() {
            // filter menu's text input, when it grows one
        }
        let _ = ctx;
        consumed
    }

    pub fn handle_edit_key(&mut self, key: EditKey, ctx: &mut ScreenCtx) -> bool {
        if self.remove_confirm.is_some() {
            let (name, id) = self.remove_confirm.clone().unwrap();
            match key {
                EditKey::Enter => {
                    self.remove_confirm = None;
                    ctx.send(Command::RemoveDeckCard {
                        deck_id: self.deck_id.unwrap_or(0),
                        card_id: id,
                    });
                    let _ = name;
                    return true;
                }
                _ => {}
            }
        }
        if self.add_confirm.is_some() && key == EditKey::Enter {
            let (name, oracle) = self.add_confirm.take().unwrap();
            self.send_add_confirmed(&name, oracle.as_deref());
            return true;
        }
        if self.edit_deck_modal.is_open() {
            return self.edit_deck_modal.handle_edit_key(key, ctx).changed;
        }
        if self.import_deck_modal.is_open() {
            return self.import_deck_modal.handle_edit_key(key, ctx).changed;
        }
        false
    }

    pub fn handle_escape(&mut self) -> bool {
        if self.export_menu_open {
            self.export_menu_open = false;
            return true;
        }
        if self.sort_menu_open {
            self.sort_menu_open = false;
            return true;
        }
        if self.add_confirm.take().is_some() {
            return true;
        }
        if self.remove_confirm.take().is_some() {
            return true;
        }
        if self.edit_deck_modal.is_open() {
            if self.edit_deck_modal.handle_escape() {
                return true;
            }
            self.edit_deck_modal.close();
            return true;
        }
        if self.import_deck_modal.is_open() {
            if self.import_deck_modal.handle_escape() {
                return true;
            }
            self.import_deck_modal.close();
            return true;
        }
        if self.delete_deck_modal.is_open() {
            if self.delete_deck_modal.handle_escape() {
                return true;
            }
            self.delete_deck_modal.close();
            return true;
        }
        false
    }

    pub fn tick(&mut self, dt: f32, ctx: &mut ScreenCtx) -> bool {
        // Debounced add-card search.
        if self.add_search_dirty {
            self.add_search_debounce -= dt;
            if self.add_search_debounce <= 0.0 {
                self.add_search_dirty = false;
                let q = self.add_field.value().trim().to_string();
                if q.len() >= 2 {
                    ctx.send(Command::SearchCards { q, limit: 6 });
                } else {
                    self.add_suggestions.clear();
                }
            }
        }
        false
    }

    pub(crate) fn send_add_confirmed(&mut self, name: &str, oracle_id: Option<&str>) {
        if let Some(tx) = &self.tx
            && let Some(deck_id) = self.deck_id
        {
            let _ = tx.send(Command::AddDeckCard {
                deck_id,
                card: Box::new(spellbook_core::ops::decks::DeckCardIn {
                    card_name: name.to_string(),
                    quantity: 1,
                    confirm: true,
                    oracle_id: oracle_id.map(str::to_string),
                }),
            });
        }
    }

    pub(crate) fn open_add_card(&mut self, name: &str, oracle_id: Option<&str>, ctx: &mut ScreenCtx) {
        if let Some(tx) = &self.tx
            && let Some(deck_id) = self.deck_id
        {
            tx.send(Command::AddDeckCard {
                deck_id,
                card: Box::new(spellbook_core::ops::decks::DeckCardIn {
                    card_name: name.to_string(),
                    quantity: 1,
                    confirm: false,
                    oracle_id: oracle_id.map(str::to_string),
                }),
            })
            .ok();
            let _ = ctx;
        }
}
}

impl DeckDetailScreen {
    /// Floating chrome over the page: modal windows (edit/import/delete),
    /// the two confirm dialogs, the open filter menu and the export list.
    pub fn render_overlay(
        &mut self,
        c: &mut Compositor,
        layer: LayerId,
        window: Rect,
        theme: &Theme,
        art: &mut ArtCache,
    ) {
        let _ = art;
        // Filter menu float.
        if self.filter_bar.is_open() && !self.edit_deck_modal.is_open() && !self.import_deck_modal.is_open() && !self.delete_deck_modal.is_open() {
            let content = window;
            let t = self.toolbar_rects(content);
            self.filter_bar.render(c, t.filter, layer, theme);
        }
        // Modals.
        if self.edit_deck_modal.is_open() {
            self.edit_deck_modal.render(c, layer, window, theme);
        } else if self.import_deck_modal.is_open() {
            self.import_deck_modal.render(c, layer, window, theme);
        } else if self.delete_deck_modal.is_open() {
            self.delete_deck_modal.render(c, layer, window, theme);
        }

        // Confirm dialogs, over everything.
        if let Some((name, _)) = self.remove_confirm.clone() {
            render_confirm_dialog(c, layer, window, theme,
                "Remover do deck?",
                &format!("{name} sai do deck e a cópia volta para as cartas livres da coleção."),
                ("Remover", true));
        } else if let Some((name, _)) = self.add_confirm.clone() {
            render_confirm_dialog(c, layer, window, theme,
                "Adicionar outra cópia?",
                &format!("{name} já está neste deck. Adicionar mais uma?"),
                ("Adicionar", false));
        }
    }
}

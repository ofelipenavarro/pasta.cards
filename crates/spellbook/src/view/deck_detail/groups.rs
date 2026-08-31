//! How the deck's cards are organized: the port of `computeDeckGroups`
//! and its helpers (sort, group-by mode, labels).

use std::collections::HashMap;

use spellbook_core::ops::decks::DeckCard;

use super::DeckDetailScreen;
use super::super::components::filters::{card_cmc_bucket, FilterCard};
use super::{GroupBy, DeckCardRef};

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



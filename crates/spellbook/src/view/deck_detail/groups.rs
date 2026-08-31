//! How the deck's cards are organized: the port of `computeDeckGroups`
//! and its helpers (sort, group-by mode, labels).

use spellbook_core::ops::decks::DeckCard;

/// The JS's colorGroupKey: single color letter, "M" for multicolor, "C" for colorless.
pub(crate) fn color_group_key(c: &DeckCard) -> &'static str {
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

pub(crate) const RARITY_ORDER: [&str; 7] = ["common", "uncommon", "rare", "mythic", "special", "bonus", "outro"];
pub(crate) fn rarity_label(r: &str) -> String {
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

pub(crate) fn category_label(cat: &str) -> &'static str {
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

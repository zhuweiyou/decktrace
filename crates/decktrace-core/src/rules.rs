use std::collections::{BTreeMap, BTreeSet};

use crate::config::ResetConfig;
use crate::models::SUIT_SYMBOLS;

/// 一局游戏的静态规则快照，供通用记牌器使用，避免 core 依赖具体游戏。
#[derive(Debug, Clone)]
pub struct DeckRules {
    pub total_cards: usize,
    pub rank_totals: BTreeMap<String, u8>,
    pub display_ranks: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub suits: Vec<String>,
    pub recent_limit: usize,
    pub bottom_count: usize,
    pub bottom_stable_frames: usize,
    pub reset: ResetConfig,
}

impl DeckRules {
    pub fn all_card_ids(&self) -> Vec<String> {
        let mut result = Vec::with_capacity(self.total_cards);
        for rank in &self.display_ranks {
            let total = *self.rank_totals.get(rank).unwrap_or(&0) as usize;
            if total == 1 && self.labels.contains_key(rank) {
                result.push(rank.clone());
                continue;
            }
            for suit in self.suits.iter().take(total) {
                result.push(format!("{rank}{suit}"));
            }
        }
        result
    }

    pub fn remaining_card_ids(&self, seen_ids: &BTreeSet<String>) -> Vec<String> {
        self.all_card_ids()
            .into_iter()
            .filter(|id| !seen_ids.contains(id))
            .collect()
    }

    pub fn label(&self, rank: &str, suit: &str) -> String {
        if let Some(label) = self.labels.get(rank) {
            return label.clone();
        }
        if let Some((_, symbol)) = SUIT_SYMBOLS.iter().find(|(s, _)| *s == suit) {
            format!("{rank}{symbol}")
        } else {
            rank.to_string()
        }
    }
}

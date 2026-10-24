use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::bottom::BottomSpec;
use crate::{RecognitionConfig, ResetConfig, SUIT_SYMBOLS, WindowSelector};

/// 一个游戏的完整接入描述：窗口、牌堆、模板、底牌策略、重置规则。
#[derive(Clone)]
pub struct GameProfile {
    pub id: String,
    pub name: String,
    pub window: WindowSelector,
    pub templates_dir: PathBuf,
    pub card_assets_dir: PathBuf,
    pub rank_totals: BTreeMap<String, u8>,
    pub display_ranks: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub suits: Vec<String>,
    pub recognition: RecognitionConfig,
    pub reset: ResetConfig,
    /// None 表示该游戏没有“等底牌开局”的概念，直接开始记牌。
    pub bottom: Option<Arc<dyn BottomSpec>>,
    pub recent_limit: usize,
    pub default_source: String,
    pub supported_sources: Vec<String>,
}

impl GameProfile {
    pub fn total_cards(&self) -> usize {
        self.rank_totals.values().map(|v| *v as usize).sum()
    }

    pub fn all_card_ids(&self) -> Vec<String> {
        let mut result = Vec::with_capacity(self.total_cards());
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

    pub fn validate(&self) -> Result<(), String> {
        if !Path::new(&self.templates_dir).exists() {
            return Err(format!(
                "游戏模板目录不存在：{}",
                self.templates_dir.display()
            ));
        }
        for rank in &self.display_ranks {
            if !self.rank_totals.contains_key(rank) {
                return Err(format!("display_ranks 未定义张数：{rank}"));
            }
        }
        if self.total_cards() == 0 {
            return Err("牌堆总数必须大于 0".to_string());
        }
        Ok(())
    }
}

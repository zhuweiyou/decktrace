use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::Detection;
use crate::rules::DeckRules;

#[derive(Debug, Clone)]
pub struct TrackerSnapshot {
    pub counts: BTreeMap<String, usize>,
    pub seen_ids: BTreeSet<String>,
    pub seen_count: usize,
    pub remaining: usize,
    pub recent: Vec<Detection>,
    /// 已在底牌区确认、但尚未打出的牌；仍计入 remaining。
    pub bottom_ids: BTreeSet<String>,
    /// 是否已识别到约定的底牌并正式开始记牌。
    pub armed: bool,
}

struct Pending {
    hits: usize,
    last_ts: f64,
    best: Detection,
}

/// 只认“牌 ID（点数+花色）”，同一张牌只扣一次。
pub struct CardTracker {
    rules: DeckRules,
    min_hits: usize,
    instant_score: f32,
    pending_ttl: f64,
    seen_ids: BTreeSet<String>,
    bottom_ids: BTreeSet<String>,
    seen_by_rank: HashMap<String, usize>,
    recent: VecDeque<Detection>,
    pending: HashMap<String, Pending>,
    armed: bool,
    bottom_count: usize,
    bottom_stable: usize,
    bottom_history: Vec<BTreeSet<String>>,
}

impl CardTracker {
    pub fn new(rules: DeckRules, min_hits: usize, instant_score: f32, pending_ttl: f64) -> Self {
        let armed = rules.bottom_count == 0;
        Self {
            bottom_count: rules.bottom_count,
            bottom_stable: rules.bottom_stable_frames.max(1),
            rules,
            min_hits: min_hits.max(1),
            instant_score,
            pending_ttl,
            seen_ids: BTreeSet::new(),
            bottom_ids: BTreeSet::new(),
            seen_by_rank: HashMap::new(),
            recent: VecDeque::new(),
            pending: HashMap::new(),
            armed,
            bottom_history: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.seen_ids.clear();
        self.bottom_ids.clear();
        self.seen_by_rank.clear();
        self.recent.clear();
        self.pending.clear();
        self.armed = self.bottom_count == 0;
        self.bottom_history.clear();
    }

    pub fn armed(&self) -> bool {
        self.armed
    }

    pub fn seen_ids(&self) -> &BTreeSet<String> {
        &self.seen_ids
    }

    pub fn bottom_ids(&self) -> &BTreeSet<String> {
        &self.bottom_ids
    }

    /// 消费一帧检测结果，返回本帧新入账的牌。
    pub fn observe(&mut self, detections: &[Detection], now: f64) -> Vec<Detection> {
        let mut just_armed = false;

        if !self.armed {
            // 底牌区没凑齐约定的牌数，说明还没发完牌/还没决出地主；
            // 翻牌动画、发牌过程中的检测一律不作数。
            let bottom_frame: BTreeSet<String> = detections
                .iter()
                .filter(|det| det.is_bottom)
                .map(|det| det.card_id())
                .collect();
            if bottom_frame.len() == self.bottom_count {
                self.bottom_history.push(bottom_frame.clone());
                let keep = (self.bottom_stable * 2).max(4);
                if self.bottom_history.len() > keep {
                    let extra = self.bottom_history.len() - keep;
                    self.bottom_history.drain(0..extra);
                }
                let seen_count = self
                    .bottom_history
                    .iter()
                    .filter(|frame| **frame == bottom_frame)
                    .count();
                if seen_count >= self.bottom_stable {
                    self.bottom_ids = bottom_frame;
                    self.armed = true;
                    just_armed = true;
                    for card_id in &self.bottom_ids {
                        self.pending.remove(card_id);
                    }
                }
            }
            if !self.armed {
                return Vec::new();
            }
        }

        // 真正参与出牌判定的只有非底牌检测；同帧按 card_id 去重。
        // 刚开局的这一帧，底牌动画残影不要入账。
        let mut best_by_id: HashMap<String, Detection> = HashMap::new();
        for det in detections {
            if det.is_bottom {
                continue;
            }
            let card_id = det.card_id();
            if just_armed && self.bottom_ids.contains(&card_id) {
                continue;
            }
            match best_by_id.get(&card_id) {
                Some(current) if current.score >= det.score => {}
                _ => {
                    best_by_id.insert(card_id, det.clone());
                }
            }
        }

        self.pending
            .retain(|_, pending| now - pending.last_ts <= self.pending_ttl);

        let mut committed = Vec::new();
        for (_, det) in best_by_id {
            let card_id = det.card_id();
            if self.seen_ids.contains(&card_id) {
                continue;
            }

            let existed = self.pending.contains_key(&card_id);
            let pending = self
                .pending
                .entry(card_id.clone())
                .or_insert_with(|| Pending {
                    hits: 1,
                    last_ts: now,
                    best: det.clone(),
                });
            if existed {
                pending.hits += 1;
                pending.last_ts = now;
                if det.score > pending.best.score {
                    pending.best = det.clone();
                }
            }

            if pending.hits >= self.min_hits || det.score >= self.instant_score {
                let best = pending.best.clone();
                self.pending.remove(&card_id);
                self.commit(&best);
                committed.push(best);
            }
        }

        for card_id in self.seen_ids.clone() {
            self.pending.remove(&card_id);
        }
        self.bottom_ids.retain(|id| !self.seen_ids.contains(id));
        committed
    }

    fn commit(&mut self, det: &Detection) {
        let card_id = det.card_id();
        if self.seen_ids.contains(&card_id) {
            return;
        }
        self.seen_ids.insert(card_id.clone());
        *self.seen_by_rank.entry(det.rank.clone()).or_insert(0) += 1;
        self.recent.push_back(det.clone());
        while self.recent.len() > self.rules.recent_limit {
            self.recent.pop_front();
        }
        // 底牌真正打出（或进入自己的手牌）后，不再参与底牌标记。
        self.bottom_ids.remove(&card_id);
    }

    pub fn counts(&self) -> BTreeMap<String, usize> {
        self.rules
            .rank_totals
            .iter()
            .map(|(rank, total)| {
                let seen = self.seen_by_rank.get(rank).copied().unwrap_or(0);
                (rank.clone(), (*total as usize).saturating_sub(seen))
            })
            .collect()
    }

    pub fn remaining(&self) -> usize {
        self.rules.total_cards.saturating_sub(self.seen_ids.len())
    }

    pub fn snapshot(&self) -> TrackerSnapshot {
        TrackerSnapshot {
            counts: self.counts(),
            seen_ids: self.seen_ids.clone(),
            seen_count: self.seen_ids.len(),
            remaining: self.remaining(),
            recent: self.recent.iter().cloned().collect(),
            bottom_ids: self.bottom_ids.clone(),
            armed: self.armed,
        }
    }
}

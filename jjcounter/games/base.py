"""游戏抽象。

一个 GameProfile 描述“这是哪个游戏、窗口长什么样、牌面模板在哪、
牌堆怎么算、什么时候算新一局”。业务层不出现 JJ 斗地主专用常量。
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Mapping, Optional, Sequence, Tuple

from ..core.models import SUIT_SYMBOLS, WindowSelector


@dataclass(frozen=True)
class JokerBand:
    top_ratio: float
    bottom_ratio: float
    template: str
    scales: Tuple[float, ...]


@dataclass(frozen=True)
class RecognitionConfig:
    rank_template_height: int = 70
    ref_suit_height: float = 45.0
    reference_width: int = 1800
    reference_height: int = 1100
    rank_min_score: float = 0.55
    suit_min_score: float = 0.60
    face_min_fraction: float = 0.25
    rank_margin: float = 0.08
    joker_template: str = "joker.png"
    joker_compact_template: Optional[str] = "joker_compact.png"
    joker_bands: Tuple[JokerBand, ...] = ()


@dataclass(frozen=True)
class ResetConfig:
    hand_top_ratio: float = 0.60
    hand_bottom_ratio: float = 0.70
    min_hand_cards: int = 12
    min_hand_for_jump: int = 15
    empty_hand_threshold: int = 5
    seen_for_reset: int = 15
    overlap_threshold: float = 0.60
    absent_seconds: float = 1.5
    min_reset_gap: float = 2.0


@dataclass(frozen=True)
class GameProfile:
    id: str
    name: str
    window: WindowSelector
    templates_dir: Path
    rank_totals: Mapping[str, int]
    display_ranks: Tuple[str, ...]
    card_assets_dir: Optional[Path] = None
    suits: Tuple[str, ...] = ("H", "D", "S", "C")
    labels: Mapping[str, str] = field(default_factory=dict)
    recognition: RecognitionConfig = field(default_factory=RecognitionConfig)
    reset: ResetConfig = field(default_factory=ResetConfig)
    recent_limit: int = 12
    default_source: str = "image"
    supported_sources: Tuple[str, ...] = ("image",)

    @property
    def total_cards(self) -> int:
        return sum(self.rank_totals.values())

    def all_card_ids(self) -> Tuple[str, ...]:
        """按显示顺序展开完整牌堆；大小王等单张牌没有花色。"""

        result = []
        for rank in self.display_ranks:
            total = self.rank_totals[rank]
            if total == 1 and rank in self.labels:
                result.append(rank)
                continue
            for suit in self.suits[:total]:
                result.append(f"{rank}{suit}")
        return tuple(result)

    def remaining_card_ids(self, seen_ids) -> Tuple[str, ...]:
        seen = set(seen_ids)
        return tuple(card for card in self.all_card_ids() if card not in seen)

    def label(self, rank: str, suit: str = "") -> str:
        if rank in self.labels:
            return self.labels[rank]
        if suit:
            return f"{rank}{SUIT_SYMBOLS.get(suit, suit)}"
        return rank

    def validate(self) -> None:
        if not self.templates_dir.exists():
            raise FileNotFoundError(f"游戏模板目录不存在：{self.templates_dir}")
        missing = set(self.display_ranks) - set(self.rank_totals)
        if missing:
            raise ValueError(f"display_ranks 未定义张数：{missing}")
        if sum(self.rank_totals.values()) <= 0:
            raise ValueError("牌堆总数必须大于 0")

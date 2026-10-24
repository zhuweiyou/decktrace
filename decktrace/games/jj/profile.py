"""JJ 斗地主：窗口特征、牌堆、模板和重置规则。"""

from __future__ import annotations

from pathlib import Path

from ...core.models import WindowSelector
from ...games.base import (
    GameProfile,
    JokerBand,
    RecognitionConfig,
    ResetConfig,
)

TEMPLATES = Path(__file__).resolve().parent / "templates"

JJ_GAME = GameProfile(
    id="jj",
    name="JJ斗地主",
    window=WindowSelector(title="JJ斗地主"),
    templates_dir=TEMPLATES,
    card_assets_dir=Path(__file__).resolve().parent / "assets" / "cards",
    rank_totals={
        "BJ": 1,
        "SJ": 1,
        "2": 4,
        "A": 4,
        "K": 4,
        "Q": 4,
        "J": 4,
        "10": 4,
        "9": 4,
        "8": 4,
        "7": 4,
        "6": 4,
        "5": 4,
        "4": 4,
        "3": 4,
    },
    display_ranks=(
        "BJ", "SJ", "2", "A", "K", "Q", "J", "10",
        "9", "8", "7", "6", "5", "4", "3",
    ),
    labels={"BJ": "大王", "SJ": "小王"},
    recognition=RecognitionConfig(
        rank_template_height=70,
        ref_suit_height=45.0,
        joker_template="joker.png",
        joker_compact_template="joker_compact.png",
        joker_bands=(
            # 底牌栏：压缩 JOK 字形
            JokerBand(0.00, 0.22, "joker_compact.png", (0.90, 1.00, 1.10)),
            # 出牌区：约为手牌尺寸的 0.6 倍
            JokerBand(0.22, 0.50, "joker.png", (0.54, 0.60, 0.66, 0.72)),
            # 手牌区
            JokerBand(0.62, 1.00, "joker.png", (0.92, 1.00, 1.08)),
        ),
    ),
    reset=ResetConfig(
        hand_top_ratio=0.60,
        hand_bottom_ratio=0.70,
        min_hand_cards=12,
        min_hand_for_jump=15,
        empty_hand_threshold=5,
        seen_for_reset=15,
        overlap_threshold=0.60,
        absent_seconds=1.5,
        min_reset_gap=2.0,
    ),
)

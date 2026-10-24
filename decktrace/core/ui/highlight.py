"""炸弹高亮判断。"""

from __future__ import annotations

from collections import Counter
from typing import Iterable, Set

from ...games.base import GameProfile


def rank_of(card_id: str, game: GameProfile) -> str:
    if card_id in game.labels:
        return card_id
    return card_id[:-1]


def card_visual_state(
    card_id: str, bottom_ids: Iterable[str], bomb_ids: Iterable[str]
) -> str:
    """底牌 > 炸弹 > 普通；底牌即使凑成炸弹也保持底牌色。"""

    if card_id in set(bottom_ids):
        return "bottom"
    if card_id in set(bomb_ids):
        return "bomb"
    return "normal"


def bomb_card_ids(remaining: Iterable[str], game: GameProfile) -> Set[str]:
    """剩余牌中某点数 4 张齐全，或双王都在，返回要高亮的牌。"""

    cards = list(remaining)
    counts = Counter(rank_of(card_id, game) for card_id in cards)
    result: Set[str] = set()
    for rank, count in counts.items():
        total = game.rank_totals.get(rank, 0)
        if total >= 4 and count >= 4:
            result.update(card_id for card_id in cards if rank_of(card_id, game) == rank)
    if "BJ" in cards and "SJ" in cards:
        result.update({"BJ", "SJ"})
    return result

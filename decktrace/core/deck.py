"""牌堆状态：按物理牌 ID 去重、计算剩余张数。"""

from __future__ import annotations

import time
from collections import Counter
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Sequence, Set

from ..games.base import GameProfile
from .models import Detection


@dataclass
class _Pending:
    hits: int
    first_ts: float
    last_ts: float
    best: Detection


@dataclass
class TrackerSnapshot:
    counts: Dict[str, int]
    seen_ids: Set[str]
    seen_count: int
    remaining: int
    recent: List[Detection] = field(default_factory=list)
    # 只在底牌栏出现过、尚未打出的牌；仍计入 remaining。
    bottom_ids: Set[str] = field(default_factory=set)


class CardTracker:
    """只认“牌 ID（点数+花色）”，同一张牌只扣一次。"""

    def __init__(
        self,
        game: GameProfile,
        min_hits: int = 2,
        instant_score: float = 0.92,
        pending_ttl: float = 1.6,
    ) -> None:
        self.game = game
        self.min_hits = max(1, min_hits)
        self.instant_score = instant_score
        self.pending_ttl = pending_ttl
        self.seen_ids: Set[str] = set()
        self.bottom_ids: Set[str] = set()
        # 已经确认过的底牌，之后即使还显示在底牌栏也不再重复标记。
        self.bottom_played: Set[str] = set()
        self.seen_by_rank: Counter = Counter()
        self.recent: List[Detection] = []
        self._pending: Dict[str, _Pending] = {}

    def reset(self) -> None:
        self.seen_ids.clear()
        self.bottom_ids.clear()
        self.bottom_played.clear()
        self.seen_by_rank.clear()
        self.recent.clear()
        self._pending.clear()

    def observe(
        self, detections: Sequence[Detection], now: Optional[float] = None
    ) -> List[Detection]:
        now = now if now is not None else time.monotonic()
        committed: List[Detection] = []

        all_detections = list(detections)

        # 底牌栏是“未出”的权威信号。翻牌动画期间牌会先在中间放大出现，
        # 可能已被误判成出牌；确认底牌后要撤回这类误入账。
        confirmed_now: Set[str] = set()
        for det in all_detections:
            if not det.is_bottom or det.card_id in self.bottom_played:
                continue
            card_id = det.card_id
            if card_id not in self.bottom_ids:
                self._unsee(card_id)
                self.bottom_ids.add(card_id)
                self._pending.pop(card_id, None)
                confirmed_now.add(card_id)

        # 真正参与出牌判定的只有非底牌检测；同帧按 card_id 去重。
        # 底牌确认当帧的动画残影不要重新入账。
        best_by_id: Dict[str, Detection] = {}
        for det in all_detections:
            if det.is_bottom or det.card_id in confirmed_now:
                continue
            current = best_by_id.get(det.card_id)
            if current is None or det.score > current.score:
                best_by_id[det.card_id] = det
        detections = list(best_by_id.values())

        expired = [
            card_id
            for card_id, pending in self._pending.items()
            if now - pending.last_ts > self.pending_ttl
        ]
        for card_id in expired:
            del self._pending[card_id]

        for det in detections:
            card_id = det.card_id
            if card_id in self.seen_ids:
                continue

            pending = self._pending.get(card_id)
            if pending is None or now - pending.last_ts > self.pending_ttl:
                pending = _Pending(hits=1, first_ts=now, last_ts=now, best=det)
                self._pending[card_id] = pending
            else:
                pending.hits += 1
                pending.last_ts = now
                if det.score > pending.best.score:
                    pending.best = det

            if pending.hits >= self.min_hits or det.score >= self.instant_score:
                self._commit(pending.best, now)
                del self._pending[card_id]
                committed.append(pending.best)

        for det in detections:
            if det.card_id in self.seen_ids:
                self._pending.pop(det.card_id, None)
        # 已经打出的牌不再算底牌；GUI 也不会显示它。
        self.bottom_ids -= self.seen_ids
        return committed

    def _commit(self, det: Detection, now: float) -> None:
        if det.card_id in self.seen_ids:
            return
        self.seen_ids.add(det.card_id)
        self.seen_by_rank[det.rank] += 1
        self.recent.append(det)
        if len(self.recent) > self.game.recent_limit:
            self.recent = self.recent[-self.game.recent_limit:]
        # 底牌真正打出（或进入自己的手牌）后，不再参与底牌标记。
        if det.card_id in self.bottom_ids:
            self.bottom_ids.discard(det.card_id)
            self.bottom_played.add(det.card_id)

    def _unsee(self, card_id: str) -> None:
        """撤回一张牌的入账；用于纠正翻牌动画期间的误判。"""

        if card_id not in self.seen_ids:
            return
        self.seen_ids.discard(card_id)
        rank = card_id if card_id in self.game.labels else card_id[:-1]
        remaining = self.seen_by_rank[rank] - 1
        if remaining > 0:
            self.seen_by_rank[rank] = remaining
        else:
            self.seen_by_rank.pop(rank, None)
        self.recent = [det for det in self.recent if det.card_id != card_id]

    def counts(self) -> Dict[str, int]:
        return {
            rank: max(0, total - self.seen_by_rank.get(rank, 0))
            for rank, total in self.game.rank_totals.items()
        }

    @property
    def remaining(self) -> int:
        return max(0, self.game.total_cards - len(self.seen_ids))

    @property
    def seen_count(self) -> int:
        return len(self.seen_ids)

    def snapshot(self) -> TrackerSnapshot:
        return TrackerSnapshot(
            counts=self.counts(),
            seen_ids=set(self.seen_ids),
            seen_count=len(self.seen_ids),
            remaining=self.remaining,
            recent=list(self.recent),
            bottom_ids=set(self.bottom_ids),
        )

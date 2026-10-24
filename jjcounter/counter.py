"""牌堆状态：去重、剩余张数、终端表格。"""

from __future__ import annotations

import time
import unicodedata
from collections import Counter
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Sequence, Set

from .recognition import Detection, SUIT_NAMES

RANK_TOTALS = {
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
}
DISPLAY_RANKS = ["BJ", "SJ", "2", "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3"]
LABELS = {"BJ": "大王", "SJ": "小王"}
TOTAL_CARDS = 54


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
    recent: List[Detection]
    newly: List[Detection] = field(default_factory=list)


class CardTracker:
    """只认“牌 ID（点数+花色）”，同一张牌只扣一次。

    一张牌要在连续帧里出现（默认 2 帧）才正式记入，避免把动画残影
    或 UI 误判永久写进牌堆；高置信度可以立即记入。
    """

    def __init__(
        self,
        min_hits: int = 2,
        instant_score: float = 0.92,
        pending_ttl: float = 1.6,
    ) -> None:
        self.min_hits = max(1, min_hits)
        self.instant_score = instant_score
        self.pending_ttl = pending_ttl
        self.seen_ids: Set[str] = set()
        self.seen_by_rank: Counter = Counter()
        self.recent: List[Detection] = []
        self._pending: Dict[str, _Pending] = {}
        self._last_commit_ts = 0.0

    def reset(self) -> None:
        self.seen_ids.clear()
        self.seen_by_rank.clear()
        self.recent.clear()
        self._pending.clear()
        self._last_commit_ts = 0.0

    def observe(
        self, detections: Sequence[Detection], now: Optional[float] = None
    ) -> List[Detection]:
        """输入一帧识别结果，返回本帧新确认的牌。"""

        now = now if now is not None else time.monotonic()
        committed: List[Detection] = []

        # 同一帧里同一张牌可能出现两次（例如底牌在顶部又出现在手牌里），
        # 只算一次观察，避免一帧就凑满 min_hits。
        best_by_id: Dict[str, Detection] = {}
        for det in detections:
            current = best_by_id.get(det.card_id)
            if current is None or det.score > current.score:
                best_by_id[det.card_id] = det
        detections = list(best_by_id.values())

        # 过期未再出现的候选
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

        # 已经入账的牌不应继续占用 pending
        for det in detections:
            if det.card_id in self.seen_ids:
                self._pending.pop(det.card_id, None)

        return committed

    def _commit(self, det: Detection, now: float) -> None:
        if det.card_id in self.seen_ids:
            return
        self.seen_ids.add(det.card_id)
        self.seen_by_rank[det.rank] += 1
        self.recent.append(det)
        if len(self.recent) > 12:
            self.recent = self.recent[-12:]
        self._last_commit_ts = now

    def counts(self) -> Dict[str, int]:
        return {
            rank: max(0, total - self.seen_by_rank.get(rank, 0))
            for rank, total in RANK_TOTALS.items()
        }

    @property
    def remaining(self) -> int:
        return max(0, TOTAL_CARDS - len(self.seen_ids))

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
        )


# ---------------- 终端表格 ----------------

RESET = "\033[0m"
BOLD = "\033[1m"
DIM = "\033[2m"
RED = "\033[31m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
CYAN = "\033[36m"
GREY = "\033[90m"


def _display_width(text: str) -> int:
    return sum(
        2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1 for ch in text
    )


def _pad(text: str, width: int) -> str:
    return text + " " * max(0, width - _display_width(text))


def _color_count(value: int, color: bool) -> str:
    text = str(value)
    if not color:
        return text
    if value == 0:
        return f"{RED}{BOLD}{text}{RESET}"
    if value == 1:
        return f"{YELLOW}{BOLD}{text}{RESET}"
    return f"{GREEN}{text}{RESET}"


def _color_card(det: Detection, color: bool) -> str:
    if not color:
        return det.label
    if det.rank == "BJ":
        return f"{RED}{BOLD}{det.label}{RESET}"
    if det.rank == "SJ":
        return f"{GREY}{BOLD}{det.label}{RESET}"
    return f"{CYAN}{det.label}{RESET}"


def render_table(
    snapshot: TrackerSnapshot,
    *,
    color: bool = True,
    status: str = "",
    recent: Optional[Sequence[Detection]] = None,
    width: int = 78,
) -> str:
    """生成终端表格；不负责清屏。"""

    counts = snapshot.counts
    headers = [_pad(LABELS.get(rank, rank), 4) for rank in DISPLAY_RANKS]
    values = []
    for rank in DISPLAY_RANKS:
        value = counts.get(rank, 0)
        values.append(_color_count(value, color) + " " * max(0, 4 - len(str(value))))

    title = "JJ 斗地主 · 记牌器"
    stats = f"已识别 {snapshot.seen_count}/54   剩余 {snapshot.remaining} 张"
    gap = max(1, width - _display_width(title) - _display_width(stats))
    line1 = f"{BOLD}{title}{RESET}" + " " * gap + stats if color else f"{title}{' ' * gap}{stats}"

    rule = "─" * width
    header_line = " ".join(headers).rstrip()
    value_line = " ".join(values).rstrip()

    recent = list(recent if recent is not None else snapshot.recent)[-8:]
    if recent:
        recent_text = "  ".join(_color_card(det, color) for det in recent)
    else:
        recent_text = "等待识别到牌…"

    lines = [
        line1,
        rule,
        header_line,
        value_line,
        rule,
        f"最近入账：{recent_text}",
    ]
    if status:
        lines.append(status)
    lines.append(
        "r 重置本局    Ctrl+C 退出"
        + ("    （只有数字+花色同时成立才会计入）" if color else "")
    )
    return "\n".join(lines)


def clear_screen() -> str:
    return "\033[2J\033[H"


def hide_cursor() -> str:
    return "\033[?25l"


def show_cursor() -> str:
    return "\033[?25h"

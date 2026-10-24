"""牌堆状态：去重、剩余张数、终端表格。"""

from __future__ import annotations

import io
import time
from collections import Counter
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Sequence, Set

from rich import box
from rich.console import Console
from rich.table import Table
from rich.text import Text

from .recognition import Detection

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

# 剩余越多，颜色越深；0 留空
COUNT_STYLES = {
    1: "#93c5fd",  # 浅蓝
    2: "#60a5fa",
    3: "#2563eb",
    4: "#1e3a8a",  # 深蓝，深色背景下仍可读
}


def _count_style(value: int, total: int) -> str:
    if value <= 0:
        return ""
    if total == 1:
        # 大小王只剩 1 张就是“满”的，直接给最深色
        return "bold #1e3a8a"
    color = COUNT_STYLES.get(value, "#1e3a8a")
    return f"{color} bold" if value >= 3 else color


def _card_text(det: Detection) -> Text:
    text = Text(det.label)
    if det.rank == "BJ":
        text.stylize("bold #dc2626")
    elif det.rank == "SJ":
        text.stylize("bold #64748b")
    elif det.suit in ("H", "D"):
        text.stylize("#dc2626")
    return text


def render_table(
    snapshot: TrackerSnapshot,
    *,
    color: bool = True,
    status: str = "",
    recent: Optional[Sequence[Detection]] = None,
    width: int = 78,
) -> str:
    """生成带边框的终端记牌表；不负责清屏。"""

    buffer = io.StringIO()
    console = Console(
        file=buffer,
        record=True,
        width=width,
        color_system="truecolor" if color else None,
        force_terminal=color,
        force_jupyter=False,
    )

    table = Table(
        box=box.ROUNDED,
        show_header=True,
        header_style="bold #64748b",
        border_style="#94a3b8",
        padding=(0, 1),
        expand=False,
    )
    for rank in DISPLAY_RANKS:
        table.add_column(
            Text(LABELS.get(rank, rank), justify="center"),
            justify="center",
            no_wrap=True,
        )

    counts = snapshot.counts
    cells = []
    for rank in DISPLAY_RANKS:
        value = counts.get(rank, 0)
        total = RANK_TOTALS[rank]
        if value <= 0:
            cells.append(Text(""))
            continue
        cell = Text(str(value))
        style = _count_style(value, total)
        if color and style:
            cell.stylize(style)
        cells.append(cell)
    table.add_row(*cells)

    title = Text.assemble(
        ("JJ 斗地主 · 记牌器", "bold"),
        "    ",
        (
            f"已识别 {snapshot.seen_count}/54   剩余 {snapshot.remaining} 张",
            "dim #64748b",
        ),
    )

    recent_items = list(recent if recent is not None else snapshot.recent)[-8:]
    recent_line = Text("最近入账：", style="dim")
    if recent_items:
        for index, det in enumerate(recent_items):
            if index:
                recent_line.append("  ")
            recent_line.append_text(_card_text(det))
    else:
        recent_line.append("等待识别到牌…", style="dim")

    console.print(title)
    console.print(table)
    console.print(recent_line)
    if status:
        console.print(Text(status, style="dim #64748b"))
    console.print(
        Text(
            "颜色越深 = 剩余越多；0 不显示    r 重置本局    Ctrl+C 退出",
            style="dim",
        )
    )
    return buffer.getvalue()


def clear_screen() -> str:
    return "\033[2J\033[H"


def hide_cursor() -> str:
    return "\033[?25l"


def show_cursor() -> str:
    return "\033[?25h"

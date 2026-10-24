"""Rich 终端记牌表。"""

from __future__ import annotations

import io
from typing import Optional, Sequence

from rich import box
from rich.console import Console
from rich.table import Table
from rich.text import Text

from ..games.base import GameProfile
from .deck import TrackerSnapshot
from .models import Detection

COUNT_STYLES = {
    1: "#93c5fd",
    2: "#60a5fa",
    3: "#2563eb",
    4: "#1e3a8a",
}


def _count_style(value: int, total: int) -> str:
    if value <= 0:
        return ""
    if total == 1:
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
    game: GameProfile,
    *,
    color: bool = True,
    status: str = "",
    recent: Optional[Sequence[Detection]] = None,
    width: int = 78,
) -> str:
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
    for rank in game.display_ranks:
        table.add_column(
            Text(game.labels.get(rank, rank), justify="center"),
            justify="center",
            no_wrap=True,
        )

    cells = []
    for rank in game.display_ranks:
        value = snapshot.counts.get(rank, 0)
        total = game.rank_totals[rank]
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
        (f"{game.name} · 记牌器", "bold"),
        "    ",
        (
            f"已识别 {snapshot.seen_count}/{game.total_cards}   "
            f"剩余 {snapshot.remaining} 张",
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

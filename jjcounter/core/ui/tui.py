"""Rich TUI 渲染器：保留 fps、队列等调试信息。"""

from __future__ import annotations

import sys

from ...games.base import GameProfile
from ..deck import TrackerSnapshot
from ..table import hide_cursor, render_table, show_cursor
from .base import Renderer


class TuiRenderer(Renderer):
    name = "tui"

    def __init__(self, color: bool = True) -> None:
        self.color = color
        self._open = False

    def start(self) -> None:
        self._open = True
        sys.stdout.write("\033[2J\033[H" + hide_cursor())
        sys.stdout.flush()

    def update(self, snapshot: TrackerSnapshot, game: GameProfile, status: str = "") -> None:
        if not self._open:
            return
        table = render_table(snapshot, game, color=self.color, status=status)
        sys.stdout.write("\033[H" + table + "\033[J")
        sys.stdout.flush()

    def close(self) -> None:
        if self._open:
            sys.stdout.write(show_cursor() + "\n")
            sys.stdout.flush()
        self._open = False

    @property
    def is_open(self) -> bool:
        return self._open

"""通用记牌运行时：消费 DetectionBatch，去做重、重置和 TUI。

这里不认识 macOS/Windows，也不认识图片/内存；只依赖 DetectionSource。
"""

from __future__ import annotations

import json
import os
import select
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import List, Optional, Set

from ..games.base import GameProfile
from .deck import CardTracker
from .models import DetectionBatch, Frame
from .source import DetectionSource
from .ui.base import Renderer


@dataclass
class SharedState:
    game: GameProfile
    tracker: CardTracker
    auto_reset: bool = True
    reset_requested: bool = False
    auto_reset_count: int = 0
    last_batch: Optional[DetectionBatch] = None
    last_frame: Optional[Frame] = None
    frames: int = 0
    errors: int = 0
    lock: threading.Lock = field(default_factory=threading.Lock)
    _prev_hand_ids: Set[str] = field(default_factory=set)
    _prev_hand_ts: float = 0.0
    _prev_hand_count: int = 0
    _last_hand_seen_ts: float = 0.0
    _last_reset_ts: float = 0.0

    def request_reset(self) -> None:
        with self.lock:
            self.reset_requested = True

    def ingest(self, batch: DetectionBatch) -> list:
        now = time.monotonic()
        with self.lock:
            if self.reset_requested:
                self._reset("手动")
                self.reset_requested = False

            if self.auto_reset:
                if batch.meta.get("new_round"):
                    self._reset("来源信号")
                elif batch.frame is not None:
                    self._maybe_auto_reset(batch.detections, batch.frame.height, now)

            newly = self.tracker.observe(batch.detections, now=now)
            self.last_batch = batch
            self.last_frame = batch.frame
            self.frames += 1
            return newly

    def _reset(self, reason: str) -> None:
        self.tracker.reset()
        self._prev_hand_ids.clear()
        self._prev_hand_ts = 0.0
        self._prev_hand_count = 0
        self._last_hand_seen_ts = 0.0
        self._last_reset_ts = time.monotonic()
        self.auto_reset_count += 1
        print(f"\n[重置] {reason}", file=sys.stderr)

    def _maybe_auto_reset(self, detections, height: int, now: float) -> None:
        cfg = self.game.reset
        hand_ids = {
            det.card_id
            for det in detections
            if det.box[1] > height * cfg.hand_top_ratio
            and det.box[1] + det.box[3] > height * cfg.hand_bottom_ratio
        }
        if len(hand_ids) < cfg.min_hand_cards:
            if now - self._last_hand_seen_ts > cfg.absent_seconds:
                self._prev_hand_count = 0
            return

        seen = len(self.tracker.seen_ids)
        jump = (
            self._prev_hand_count <= cfg.empty_hand_threshold
            and len(hand_ids) >= cfg.min_hand_for_jump
            and seen > 0
        )
        overlap = (
            len(hand_ids & self._prev_hand_ids) / max(len(hand_ids), 1)
            if self._prev_hand_ids
            else 0.0
        )
        changed = (
            bool(self._prev_hand_ids)
            and seen >= cfg.seen_for_reset
            and now - self._last_reset_ts > cfg.min_reset_gap
            and overlap < cfg.overlap_threshold
        )
        if jump or changed:
            reason = "手牌张数跳变" if jump else f"手牌重合度 {overlap:.0%}"
            self._reset(f"{reason}，清空前已识别 {seen} 张")
            self._prev_hand_ids = set(hand_ids)
            self._prev_hand_ts = now
            self._prev_hand_count = len(hand_ids)
            self._last_hand_seen_ts = now
            return

        self._prev_hand_ids = set(hand_ids)
        self._prev_hand_ts = now
        self._prev_hand_count = len(hand_ids)
        self._last_hand_seen_ts = now

    def snapshot(self):
        with self.lock:
            return (
                self.tracker.snapshot(),
                self.last_batch,
                self.last_frame,
                self.frames,
                self.auto_reset_count,
            )


def _write_state(out_dir: Path, snap, batch: Optional[DetectionBatch], status: str) -> None:
    try:
        out_dir.mkdir(parents=True, exist_ok=True)
        payload = {
            "seen_count": snap.seen_count,
            "remaining": snap.remaining,
            "counts": {k: int(v) for k, v in snap.counts.items()},
            "seen_ids": sorted(snap.seen_ids),
            "bottom_ids": sorted(snap.bottom_ids),
            "recent": [d.label for d in snap.recent],
            "detections": (
                [
                    {
                        "card": d.label,
                        "id": d.card_id,
                        "score": round(d.score, 3),
                        "box": [int(v) for v in d.box],
                        "bottom": bool(d.is_bottom),
                    }
                    for d in batch.detections
                ]
                if batch
                else []
            ),
            "status": status,
        }
        tmp = out_dir / "state.tmp.json"
        tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        os.replace(tmp, out_dir / "state.json")
    except OSError:
        pass


def _input_loop(state: SharedState, stop_event: threading.Event) -> None:
    if not sys.stdin.isatty():
        return
    import termios
    import tty

    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    try:
        tty.setcbreak(fd)
        while not stop_event.is_set():
            ready, _, _ = select.select([sys.stdin], [], [], 0.1)
            if not ready:
                continue
            ch = sys.stdin.read(1)
            if ch in ("r", "R"):
                state.request_reset()
            elif ch in ("q", "Q"):
                stop_event.set()
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)


def _status_line(stats, auto_resets: int) -> str:
    return (
        f"{stats.fps:.1f}fps · 识别 {stats.recognition_ms:.0f}ms · "
        f"队列 {stats.queue_size} · 延迟 {stats.frame_age:.1f}s · "
        f"错 {stats.errors} · 重置 {auto_resets} · {stats.backend}"
    )


def run_game(
    *,
    game: GameProfile,
    source: DetectionSource,
    out_dir: Path,
    auto_reset: bool,
    min_hits: int,
    debug: bool,
    duration: Optional[float],
    renderer: Renderer,
    header: str = "",
) -> int:
    state = SharedState(
        game=game,
        tracker=CardTracker(game, min_hits=min_hits),
        auto_reset=auto_reset,
    )
    stop_event = threading.Event()

    if header:
        print(header)
    print(
        f"游戏：{game.name}   来源：{source.name}   输出：{out_dir.resolve()}   "
        f"自动重置：{'开' if auto_reset else '关'}"
    )
    print("按 r 重置本局，Ctrl+C 退出。")
    try:
        renderer.start()
    except Exception as exc:
        print(f"启动 UI 失败：{exc}", file=sys.stderr)
        return 2
    try:
        source.start()
    except Exception as exc:
        print(f"启动识别来源失败：{exc}", file=sys.stderr)
        source.stop()
        renderer.close()
        return 2

    input_thread = threading.Thread(
        target=_input_loop, args=(state, stop_event), name="keyboard", daemon=True
    )
    input_thread.start()
    started = time.monotonic()
    try:
        while not stop_event.is_set():
            if duration is not None and time.monotonic() - started >= duration:
                break
            batch = source.read(timeout=0.15)
            if batch is not None:
                with state.lock:
                    state.errors = source.stats().errors
                state.ingest(batch)
            snap, last_batch, last_frame, frames, auto_resets = state.snapshot()
            stats = source.stats()
            status = _status_line(stats, auto_resets)
            renderer.update(snap, game, status)
            if not renderer.is_open:
                break
            _write_state(out_dir, snap, last_batch, status)
            if batch is None and stats.failed:
                print(f"\n[来源中断] {stats.errors} 次错误", file=sys.stderr)
                break
    except KeyboardInterrupt:
        pass
    finally:
        stop_event.set()
        source.stop()
        input_thread.join(timeout=1.0)
        renderer.close()
        snap, _, last_frame, frames, auto_resets = state.snapshot()
        print(
            f"已停止：识别 {snap.seen_count} 张，剩余 {snap.remaining} 张，"
            f"自动重置 {auto_resets} 次。"
        )
        if last_frame and last_frame.path:
            print(f"最后一帧：{last_frame.path.resolve()}")
        sys.stdout.flush()
    return 0

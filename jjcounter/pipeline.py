"""采集线程 + 有界队列 + 识别线程 + 终端表格。

采集线程只负责截图并尽快入队；识别慢不会拖慢截图。
队列满时丢最旧帧，保证看到的始终是较新的画面。
"""

from __future__ import annotations

import json
import os
import queue
import select
import sys
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import List, Optional, Set

import cv2
import numpy as np

from .capture import CaptureError, Frame, WindowCapture
from .counter import (
    CardTracker,
    TrackerSnapshot,
    hide_cursor,
    render_table,
    show_cursor,
)
from .recognition import (
    CardRecognizer,
    Detection,
    RecognitionResult,
    annotate,
)
from .sck import SCKCapture, SCKError, RawFrame, save_jpeg_atomic
from .window import WindowError, WindowInfo, find_window


@dataclass
class FramePacket:
    seq: int
    frame: Frame
    image: np.ndarray
    enqueued_at: float


@dataclass
class SharedState:
    tracker: CardTracker
    lock: threading.Lock = field(default_factory=threading.Lock)
    auto_reset: bool = True
    reset_requested: bool = False
    auto_reset_count: int = 0
    last_frame: Optional[Frame] = None
    last_result: Optional[RecognitionResult] = None
    last_annotated: Optional[np.ndarray] = None
    recognition_ms: float = 0.0
    frames: int = 0
    errors: int = 0
    dropped: int = 0
    _prev_hand_ids: Set[str] = field(default_factory=set)
    _prev_hand_ts: float = 0.0
    _prev_hand_count: int = 0
    _last_hand_seen_ts: float = 0.0
    _last_reset_ts: float = 0.0

    def request_reset(self) -> None:
        with self.lock:
            self.reset_requested = True

    def reset_now(self, reason: str = "手动") -> None:
        with self.lock:
            self.tracker.reset()
            self._prev_hand_ids.clear()
            self._prev_hand_ts = 0.0
            self._prev_hand_count = 0
            self._last_hand_seen_ts = 0.0
            self._last_reset_ts = time.monotonic()
            self.auto_reset_count += 1 if reason.startswith("自动") else 0

    def ingest(
        self, packet: FramePacket, result: RecognitionResult, make_annotated: bool = False
    ) -> List[Detection]:
        now = time.monotonic()
        annotated = annotate(packet.image, result.detections) if make_annotated else None
        with self.lock:
            if self.reset_requested:
                self.tracker.reset()
                self._prev_hand_ids.clear()
                self._prev_hand_ts = 0.0
                self._prev_hand_count = 0
                self._last_hand_seen_ts = 0.0
                self._last_reset_ts = now
                self.reset_requested = False

            if self.auto_reset:
                self._maybe_auto_reset(packet.image, result.detections, now)

            newly = self.tracker.observe(result.detections, now=now)
            self.last_frame = packet.frame
            self.last_result = result
            if annotated is not None:
                self.last_annotated = annotated
            self.recognition_ms = result.elapsed_ms
            self.frames += 1
            return newly

    def _maybe_auto_reset(
        self, image: np.ndarray, detections: List[Detection], now: float
    ) -> None:
        """新一局开始时手牌会整体换掉；用这个信号自动清空。

        两个信号任一成立即可：
        1. 手牌张数从很少（<=5）突然跳到 >=15：新一局发牌；
        2. 当前手牌与上一帧手牌重合度低于 60%：正常出牌时是上一帧的子集，
           重合度接近 100%，新一局随机发牌期望重合度约 30%。
        """

        height = image.shape[0]
        hand_ids = {
            det.card_id
            for det in detections
            if det.box[1] > height * 0.60
            and det.box[1] + det.box[3] > height * 0.70
        }
        if len(hand_ids) < 12:
            # 手牌离开画面一段时间后，下一局发牌会被视为 0 -> 17 的跳变
            if now - self._last_hand_seen_ts > 1.5:
                self._prev_hand_count = 0
            return

        seen = len(self.tracker.seen_ids)
        jump = self._prev_hand_count <= 5 and len(hand_ids) >= 15 and seen > 0
        overlap = (
            len(hand_ids & self._prev_hand_ids) / max(len(hand_ids), 1)
            if self._prev_hand_ids
            else 0.0
        )
        changed_hand = (
            self._prev_hand_ids
            and seen >= 15
            and now - self._last_reset_ts > 2.0
            and overlap < 0.60
        )
        should_reset = jump or changed_hand

        if should_reset:
            reason = "手牌张数跳变" if jump else f"手牌重合度 {overlap:.0%}"
            self.tracker.reset()
            self._last_reset_ts = now
            self.auto_reset_count += 1
            print(
                f"\n[自动重置] {reason}，手牌 {len(hand_ids)} 张，"
                f"清空前已识别 {seen} 张",
                file=sys.stderr,
            )
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
            snap = self.tracker.snapshot()
            return (
                snap,
                self.last_result,
                self.last_frame,
                self.recognition_ms,
                self.frames,
                self.errors,
                self.dropped,
                self.auto_reset_count,
            )


class CaptureProducer(threading.Thread):
    def __init__(
        self,
        capture: WindowCapture,
        out_queue: "queue.Queue[FramePacket]",
        stop_event: threading.Event,
        state: SharedState,
        interval: float,
    ) -> None:
        super().__init__(name="capture", daemon=True)
        self.capture = capture
        self.queue = out_queue
        self.stop_event = stop_event
        self.state = state
        self.interval = interval
        self.seq = 0
        self.started_at = time.monotonic()
        self._last_error_print = 0.0

    def run(self) -> None:
        while not self.stop_event.is_set():
            tick = time.monotonic()
            try:
                frame = self.capture.capture()
                image = cv2.imread(str(frame.path), cv2.IMREAD_COLOR)
                if image is None:
                    raise CaptureError(f"无法读取截图：{frame.path}")
                self.seq += 1
                packet = FramePacket(
                    seq=self.seq,
                    frame=frame,
                    image=image,
                    enqueued_at=time.monotonic(),
                )
                while True:
                    try:
                        self.queue.put_nowait(packet)
                        break
                    except queue.Full:
                        try:
                            self.queue.get_nowait()
                            with self.state.lock:
                                self.state.dropped += 1
                        except queue.Empty:
                            pass
            except (CaptureError, WindowError, OSError) as exc:
                self.capture.window = None
                self.capture._resolved_at = 0.0
                with self.state.lock:
                    self.state.errors += 1
                now = time.monotonic()
                if now - self._last_error_print >= 3.0:
                    print(f"\n[采集] {exc}", file=sys.stderr)
                    self._last_error_print = now
                self.stop_event.wait(0.3)
                continue
            except Exception as exc:  # 截图线程不能死
                with self.state.lock:
                    self.state.errors += 1
                print(f"\n[采集] 未预期错误：{exc}", file=sys.stderr)
                self.stop_event.wait(0.5)
                continue

            elapsed = time.monotonic() - tick
            self.stop_event.wait(max(0.01, self.interval - elapsed))

    @property
    def fps(self) -> float:
        elapsed = max(time.monotonic() - self.started_at, 1e-6)
        return self.seq / elapsed


class SCKCaptureProducer(threading.Thread):
    """消费常驻 ScreenCaptureKit helper 的二进制帧流。"""

    def __init__(
        self,
        sck: SCKCapture,
        window: WindowInfo,
        out_queue: "queue.Queue[FramePacket]",
        stop_event: threading.Event,
        state: SharedState,
        latest_path: Path,
    ) -> None:
        super().__init__(name="capture-sck", daemon=True)
        self.sck = sck
        self.window = window
        self.queue = out_queue
        self.stop_event = stop_event
        self.state = state
        self.latest_path = latest_path
        self.seq = 0
        self.started_at = time.monotonic()
        self._last_save = 0.0
        self._last_error_print = 0.0

    def _handle(self, raw: RawFrame) -> None:
        started = time.perf_counter()
        image = cv2.imdecode(
            np.frombuffer(raw.jpeg, dtype=np.uint8), cv2.IMREAD_COLOR
        )
        elapsed = time.perf_counter() - started
        if image is None:
            raise SCKError("无法解码 ScreenCaptureKit JPEG 帧")

        h, w = image.shape[:2]
        captured_at = datetime.fromtimestamp(raw.timestamp).astimezone()
        frame = Frame(
            path=self.latest_path,
            captured_at=captured_at,
            window=self.window,
            width=w,
            height=h,
            scale=w / self.window.width if self.window.width else 1.0,
            elapsed=elapsed,
        )
        self.seq += 1
        packet = FramePacket(
            seq=self.seq,
            frame=frame,
            image=image,
            enqueued_at=time.monotonic(),
        )
        while True:
            try:
                self.queue.put_nowait(packet)
                break
            except queue.Full:
                try:
                    self.queue.get_nowait()
                    with self.state.lock:
                        self.state.dropped += 1
                except queue.Empty:
                    pass

        now = time.monotonic()
        if now - self._last_save >= 0.5:
            try:
                save_jpeg_atomic(self.latest_path, raw.jpeg)
            except OSError:
                pass
            self._last_save = now

    def run(self) -> None:
        raw = self.sck.take_first()
        while not self.stop_event.is_set():
            try:
                if raw is None:
                    raw = self.sck.read(timeout=2.0)
                self._handle(raw)
                raw = None
            except (SCKError, OSError) as exc:
                with self.state.lock:
                    self.state.errors += 1
                now = time.monotonic()
                if now - self._last_error_print >= 3.0:
                    print(f"\n[SCK采集] {exc}", file=sys.stderr)
                    self._last_error_print = now
                # 采集断了就不要继续显示旧牌表，让主循环退出并提示重启
                self.stop_event.set()
                break

    @property
    def fps(self) -> float:
        elapsed = max(time.monotonic() - self.started_at, 1e-6)
        return self.seq / elapsed


class RecognitionWorker(threading.Thread):
    def __init__(
        self,
        recognizer: CardRecognizer,
        out_queue: "queue.Queue[FramePacket]",
        stop_event: threading.Event,
        state: SharedState,
        debug_dir: Optional[Path] = None,
    ) -> None:
        super().__init__(name="recognizer", daemon=True)
        self.recognizer = recognizer
        self.queue = out_queue
        self.stop_event = stop_event
        self.state = state
        self.debug_dir = debug_dir
        self._frame_no = 0
        self._last_debug_save = 0.0
        self._debug_tmp = debug_dir / "annotated.tmp.png" if debug_dir else None
        self._debug_out = debug_dir / "annotated.png" if debug_dir else None
        if debug_dir:
            debug_dir.mkdir(parents=True, exist_ok=True)

    def run(self) -> None:
        while not self.stop_event.is_set():
            try:
                packet = self.queue.get(timeout=0.2)
            except queue.Empty:
                continue
            try:
                with self.state.lock:
                    both_jokers_seen = (
                        "BJ" in self.state.tracker.seen_ids
                        and "SJ" in self.state.tracker.seen_ids
                    )
                # 王一旦都确认过就不用再找；未确认时每 2 帧找一次。
                # 牌会停留数秒，不需要每帧都跑 60ms 的王模板。
                detect_jokers = (
                    not both_jokers_seen and self._frame_no % 2 == 0
                )
                self._frame_no += 1
                result = self.recognizer.recognize(
                    packet.image, detect_jokers=detect_jokers
                )
                self.state.ingest(packet, result, make_annotated=self.debug_dir is not None)
                now = time.monotonic()
                if (
                    self._debug_out
                    and self.state.last_annotated is not None
                    and now - self._last_debug_save >= 0.5
                ):
                    assert self._debug_tmp is not None
                    cv2.imwrite(str(self._debug_tmp), self.state.last_annotated)
                    os.replace(self._debug_tmp, self._debug_out)
                    self._last_debug_save = now
            except Exception as exc:
                with self.state.lock:
                    self.state.errors += 1
                print(f"\n[识别] {exc}", file=sys.stderr)
                self.stop_event.wait(0.2)


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


def _status_line(
    state: SharedState,
    packet_queue: "queue.Queue[FramePacket]",
    producer: CaptureProducer,
) -> str:
    with state.lock:
        result = state.last_result
        recognition_ms = state.recognition_ms
        errors = state.errors
        dropped = state.dropped
        auto_resets = state.auto_reset_count
        last_frame = state.last_frame
    qsize = packet_queue.qsize()
    age = last_frame.age if last_frame else 0.0
    det_count = len(result.detections) if result else 0
    return (
        f"{producer.fps:0.1f}fps · 识别 {recognition_ms:.0f}ms · "
        f"队列 {qsize} · 延迟 {age:.1f}s · 错 {errors} · 重置 {auto_resets}"
    )


def _write_state(out_dir: Path, snap, result, status: str) -> None:
    try:
        out_dir.mkdir(parents=True, exist_ok=True)
        payload = {
            "seen_count": snap.seen_count,
            "remaining": snap.remaining,
            "counts": {k: int(v) for k, v in snap.counts.items()},
            "seen_ids": sorted(snap.seen_ids),
            "recent": [d.label for d in snap.recent],
            "detections": (
                [
                    {
                        "card": d.label,
                        "id": d.card_id,
                        "score": round(d.score, 3),
                        "box": [int(v) for v in d.box],
                    }
                    for d in result.detections
                ]
                if result
                else []
            ),
            "status": status,
        }
        tmp = out_dir / "state.tmp.json"
        tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        os.replace(tmp, out_dir / "state.json")
    except OSError:
        pass


def run_counter(
    *,
    title: str,
    owner: Optional[str],
    window_id: Optional[int],
    interval: float,
    min_hits: int,
    auto_reset: bool,
    debug: bool,
    duration: Optional[float],
    once: bool,
    color: bool,
    out_dir: Path,
    capture_format: str = "jpg",
    capture_backend: str = "sck",
    sck_fps: float = 15.0,
    sck_quality: float = 0.88,
) -> int:
    try:
        window = find_window(title=title, owner=owner, window_id=window_id)
    except WindowError as exc:
        print(f"找不到窗口：{exc}", file=sys.stderr)
        return 2

    try:
        recognizer = CardRecognizer()
    except FileNotFoundError as exc:
        print(f"识别模板加载失败：{exc}", file=sys.stderr)
        return 2

    if once:
        capture = WindowCapture(
            title=title,
            owner=owner,
            window_id=window.id,
            interval=interval,
            fmt=capture_format,
            output_dir=out_dir,
            max_consecutive_errors=20,
        )
        try:
            frame = capture.capture()
            image = cv2.imread(str(frame.path), cv2.IMREAD_COLOR)
            if image is None:
                raise CaptureError(f"无法读取截图：{frame.path}")
            result = recognizer.recognize(image)
            tracker = CardTracker(min_hits=1)
            tracker.observe(result.detections)
            snap = tracker.snapshot()
            print(render_table(snap, color=color, status="单帧模式"))
            annotated = annotate(image, result.detections)
            if debug:
                out_dir.mkdir(parents=True, exist_ok=True)
                cv2.imwrite(str(out_dir / "annotated.png"), annotated)
            return 0
        except (CaptureError, WindowError) as exc:
            print(f"截图失败：{exc}", file=sys.stderr)
            return 1

    state = SharedState(tracker=CardTracker(min_hits=min_hits), auto_reset=auto_reset)
    packet_queue: "queue.Queue[FramePacket]" = queue.Queue(
        maxsize=3 if capture_backend == "sck" else 30
    )
    stop_event = threading.Event()
    sck: Optional[SCKCapture] = None
    backend = capture_backend

    if backend == "sck":
        try:
            sck = SCKCapture(window, fps=sck_fps, quality=sck_quality)
            sck.start()
            producer = SCKCaptureProducer(
                sck,
                window,
                packet_queue,
                stop_event,
                state,
                latest_path=out_dir / "latest.jpg",
            )
        except Exception as exc:
            print(
                f"ScreenCaptureKit 启动失败，回退 screencapture：{exc}",
                file=sys.stderr,
            )
            sck = None
            backend = "legacy"

    if backend == "legacy":
        capture = WindowCapture(
            title=title,
            owner=owner,
            window_id=window.id,
            interval=interval,
            fmt=capture_format,
            output_dir=out_dir,
            max_consecutive_errors=20,
        )
        producer = CaptureProducer(
            capture, packet_queue, stop_event, state, interval
        )

    worker = RecognitionWorker(
        recognizer,
        packet_queue,
        stop_event,
        state,
        debug_dir=(out_dir if debug else None),
    )

    backend_desc = (
        f"ScreenCaptureKit {sck_fps:g}fps"
        if backend == "sck"
        else f"screencapture {interval:g}s"
    )
    print(f"目标窗口：{window}")
    print(
        f"采集后端：{backend_desc}   输出目录：{out_dir.resolve()}   "
        f"自动重置：{'开' if auto_reset else '关'}"
    )
    print("按 r 重置本局，Ctrl+C 退出。")
    time.sleep(0.8)
    sys.stdout.write("\033[2J\033[H")
    sys.stdout.write(hide_cursor())
    sys.stdout.flush()

    producer.start()
    worker.start()
    input_thread = threading.Thread(
        target=_input_loop, args=(state, stop_event), name="keyboard", daemon=True
    )
    input_thread.start()

    started = time.monotonic()
    try:
        while not stop_event.is_set():
            if duration is not None and time.monotonic() - started >= duration:
                break
            time.sleep(0.12)
            snap, result, last_frame, recognition_ms, frames, errors, dropped, resets = (
                state.snapshot()
            )
            status = _status_line(state, packet_queue, producer)
            table = render_table(snap, color=color, status=status)
            sys.stdout.write("\033[H" + table + "\033[J")
            sys.stdout.flush()
            _write_state(out_dir, snap, result, status)
    except KeyboardInterrupt:
        pass
    finally:
        stop_event.set()
        producer.join(timeout=2.0)
        worker.join(timeout=3.0)
        if "input_thread" in locals():
            input_thread.join(timeout=1.0)
        if sck is not None:
            sck.stop()
        sys.stdout.write(show_cursor() + "\n")
        snap, result, last_frame, recognition_ms, frames, errors, dropped, resets = (
            state.snapshot()
        )
        print(
            f"已停止：识别 {snap.seen_count} 张牌，剩余 {snap.remaining} 张，"
            f"错误 {errors} 次，丢帧 {dropped} 次，自动重置 {resets} 次。"
        )
        if last_frame:
            print(f"最后一帧：{last_frame.path.resolve()}")
        sys.stdout.flush()

    return 0

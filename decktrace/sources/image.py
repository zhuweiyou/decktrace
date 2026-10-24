"""图片模板识别来源：平台采集流 + 模板识别引擎。"""

from __future__ import annotations

import os
import queue
import sys
import threading
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Optional

import cv2

from ..core.models import DetectionBatch, Frame, RawFrame, WindowInfo
from ..core.source import DetectionSource, SourceStats
from ..platforms.base import CaptureError, CaptureStream
from ..recognition import CardRecognizer, annotate


@dataclass
class _ImagePacket:
    raw: RawFrame


class ImageDetectionSource(DetectionSource):
    name = "image"

    def __init__(
        self,
        stream: CaptureStream,
        recognizer: CardRecognizer,
        window: WindowInfo,
        *,
        fps: float = 15.0,
        debug_dir: Optional[Path] = None,
        joker_every: int = 2,
    ) -> None:
        self.stream = stream
        self.recognizer = recognizer
        self.window = window
        self.fps = fps
        self.debug_dir = debug_dir
        self.joker_every = max(1, joker_every)
        self.image_queue: "queue.Queue[_ImagePacket]" = queue.Queue(maxsize=3)
        self.detection_queue: "queue.Queue[Optional[DetectionBatch]]" = queue.Queue(
            maxsize=3
        )
        self.stop_event = threading.Event()
        self.dropped = 0
        self.errors = 0
        self.failed = False
        self.frames = 0
        self.started_at = time.monotonic()
        self.last_frame_ts = 0.0
        self.last_recognition_ms = 0.0
        self._frame_no = 0
        self._last_debug_save = 0.0
        self._capture_thread: Optional[threading.Thread] = None
        self._recognize_thread: Optional[threading.Thread] = None

    def start(self) -> None:
        self.stream.start()
        self._capture_thread = threading.Thread(
            target=self._capture_loop, name="capture", daemon=True
        )
        self._recognize_thread = threading.Thread(
            target=self._recognize_loop, name="recognizer", daemon=True
        )
        self._capture_thread.start()
        self._recognize_thread.start()

    def _enqueue(self, q, item) -> None:
        while True:
            try:
                q.put_nowait(item)
                return
            except queue.Full:
                try:
                    q.get_nowait()
                    self.dropped += 1
                except queue.Empty:
                    pass

    def _capture_loop(self) -> None:
        while not self.stop_event.is_set():
            tick = time.monotonic()
            try:
                raw = self.stream.read(timeout=2.0)
                self.frames += 1
                self.last_frame_ts = raw.timestamp
                self._enqueue(self.image_queue, _ImagePacket(raw))
            except (CaptureError, OSError) as exc:
                if self.stop_event.is_set():
                    return
                self.errors += 1
                print(f"\n[采集] {exc}", file=sys.stderr)
                self.failed = True
                self.stop_event.set()
                self._enqueue(self.detection_queue, None)
                return
            if not self.stream.self_paced:
                interval = 1.0 / max(self.stream.target_fps, 0.1)
                elapsed = time.monotonic() - tick
                self.stop_event.wait(max(0.005, interval - elapsed))

    def _recognize_loop(self) -> None:
        while not self.stop_event.is_set():
            try:
                packet = self.image_queue.get(timeout=0.2)
            except queue.Empty:
                continue
            raw = packet.raw
            detect_jokers = self._frame_no % self.joker_every == 0
            self._frame_no += 1
            try:
                result = self.recognizer.recognize(
                    raw.image, detect_jokers=detect_jokers
                )
            except Exception as exc:
                self.errors += 1
                print(f"\n[识别] {exc}", file=sys.stderr)
                self.failed = True
                continue

            h, w = raw.image.shape[:2]
            captured_at = datetime.fromtimestamp(raw.timestamp).astimezone()
            frame = Frame(
                path=self.stream.latest_path,
                captured_at=captured_at,
                window=self.window,
                width=w,
                height=h,
                scale=w / self.window.width if self.window.width else 1.0,
                elapsed=result.elapsed_ms / 1000.0,
            )
            batch = DetectionBatch(
                detections=result.detections,
                timestamp=raw.timestamp,
                frame=frame,
                recognition_ms=result.elapsed_ms,
                seq=raw.seq,
            )
            self.last_recognition_ms = result.elapsed_ms
            self._enqueue(self.detection_queue, batch)

            if self.debug_dir is not None:
                now = time.monotonic()
                if now - self._last_debug_save >= 0.5:
                    try:
                        out = self.debug_dir / "annotated.png"
                        tmp = self.debug_dir / "annotated.tmp.png"
                        cv2.imwrite(str(tmp), annotate(raw.image, result.detections))
                        os.replace(tmp, out)
                    except OSError:
                        pass
                    self._last_debug_save = now

    def read(self, timeout: float = 2.0) -> Optional[DetectionBatch]:
        try:
            return self.detection_queue.get(timeout=timeout)
        except queue.Empty:
            return None

    def stop(self) -> None:
        self.stop_event.set()
        try:
            self.stream.stop()
        except Exception:
            pass
        for thread in (self._capture_thread, self._recognize_thread):
            if thread is not None:
                thread.join(timeout=2.0)
        self._capture_thread = None
        self._recognize_thread = None

    def stats(self) -> SourceStats:
        elapsed = max(time.monotonic() - self.started_at, 1e-6)
        age = max(time.time() - self.last_frame_ts, 0.0) if self.last_frame_ts else 0.0
        return SourceStats(
            fps=self.frames / elapsed,
            recognition_ms=self.last_recognition_ms,
            queue_size=self.image_queue.qsize() + self.detection_queue.qsize(),
            dropped=self.dropped,
            errors=self.errors,
            frame_age=age,
            backend=self.stream.backend_name,
            failed=self.failed,
        )

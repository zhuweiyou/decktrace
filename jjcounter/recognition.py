"""牌面识别：点数 + 花色成对校验。

核心约束：
1. 先用红/黑像素找到花色块；
2. 花色正上方必须找到点数；
3. 点数、花色都要和模板匹配上；
4. 周围必须是浅色卡面。

因此“750”“排名 802/2400”“倍数 1”等纯数字不会被当成牌。
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple

import cv2
import numpy as np

TEMPLATE_DIR = Path(__file__).resolve().parent / "templates"
RANK_ORDER = ["3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A", "2"]
SUIT_NAMES = {"H": "♥", "D": "♦", "S": "♠", "C": "♣"}
REF_SUIT_HEIGHT = 45.0
RANK_TPL_HEIGHT = 70
RANK_MIN_SCORE = 0.55
SUIT_MIN_SCORE = 0.60
FACE_MIN_FRACTION = 0.25
RANK_MARGIN = 0.08


@dataclass(frozen=True)
class Detection:
    rank: str
    suit: str
    score: float
    box: Tuple[int, int, int, int]
    rank_score: float = 0.0
    suit_score: float = 0.0
    face_score: float = 0.0

    @property
    def card_id(self) -> str:
        return f"{self.rank}{self.suit}" if self.suit else self.rank

    @property
    def label(self) -> str:
        if self.rank == "SJ":
            return "小王"
        if self.rank == "BJ":
            return "大王"
        return f"{self.rank}{SUIT_NAMES.get(self.suit, self.suit)}"

    def __str__(self) -> str:
        return f"{self.label}[{self.score:.2f}]"


@dataclass
class RecognitionResult:
    detections: List[Detection]
    elapsed_ms: float
    frame_width: int
    frame_height: int


class CardRecognizer:
    def __init__(self, template_dir: Optional[Path] = None) -> None:
        base = Path(template_dir) if template_dir else TEMPLATE_DIR
        rank_dir = base / "ranks"
        suit_dir = base / "suits"
        if not rank_dir.exists() or not suit_dir.exists():
            raise FileNotFoundError(
                f"缺少模板目录：{rank_dir} / {suit_dir}，"
                "请先运行 tools/build_templates.py"
            )

        self.ranks: Dict[str, np.ndarray] = {
            path.stem: cv2.imread(str(path), cv2.IMREAD_GRAYSCALE)
            for path in sorted(rank_dir.glob("*.png"))
        }
        self.suits: Dict[str, np.ndarray] = {
            path.stem: cv2.imread(str(path), cv2.IMREAD_GRAYSCALE)
            for path in sorted(suit_dir.glob("*.png"))
        }
        self.joker_template = cv2.imread(
            str(base / "joker.png"), cv2.IMREAD_GRAYSCALE
        )
        # 底牌栏的小王会被压缩绘制，单独用一份紧凑模板
        self.joker_compact = cv2.imread(
            str(base / "joker_compact.png"), cv2.IMREAD_GRAYSCALE
        )
        if not self.ranks or len(self.suits) != 4 or self.joker_template is None:
            raise FileNotFoundError("模板文件不完整")

    # ---------- 基础图像处理 ----------

    @staticmethod
    def _masks(image: np.ndarray) -> Tuple[np.ndarray, np.ndarray]:
        b, g, r = cv2.split(image)
        red = (
            (r.astype(int) > 110)
            & (r.astype(int) - g.astype(int) > 45)
            & (r.astype(int) - b.astype(int) > 35)
        ).astype(np.uint8) * 255
        dark = ((r < 90) & (g < 90) & (b < 90)).astype(np.uint8) * 255
        return red, dark

    @staticmethod
    def _components(mask: np.ndarray):
        n, _labels, stats, _centroids = cv2.connectedComponentsWithStats(mask, 8)
        return [
            (int(x), int(y), int(w), int(h), int(area))
            for x, y, w, h, area in stats[1:]
        ]

    @staticmethod
    def _normalize_rank(crop: np.ndarray) -> np.ndarray:
        h, w = crop.shape[:2]
        nh = RANK_TPL_HEIGHT
        nw = max(1, round(w * nh / h))
        resized = cv2.resize(crop, (nw, nh), interpolation=cv2.INTER_AREA)
        _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
        return resized

    @staticmethod
    def _normalize_suit(crop: np.ndarray) -> np.ndarray:
        size = 40
        h, w = crop.shape[:2]
        scale = (size - 6) / max(h, w)
        rw, rh = max(1, round(w * scale)), max(1, round(h * scale))
        resized = cv2.resize(crop, (rw, rh), interpolation=cv2.INTER_AREA)
        _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
        canvas = np.zeros((size, size), np.uint8)
        x = (size - rw) // 2
        y = (size - rh) // 2
        canvas[y:y + rh, x:x + rw] = resized
        return canvas

    # ---------- 定位 ----------

    @staticmethod
    def _rank_bbox(
        mask: np.ndarray, sx: int, sy: int, sw: int, sh: int
    ) -> Optional[Tuple[int, int, int, int]]:
        """在花色正上方定位点数外框。"""

        y0 = max(0, sy - int(2.2 * sh))
        y1 = max(1, sy - 2)
        x0 = max(0, sx - int(0.2 * sw))
        x1 = min(mask.shape[1], sx + int(1.45 * sw))
        sub = mask[y0:y1, x0:x1]
        glyphs = []
        for x, y, w, h, area in CardRecognizer._components(sub):
            if h < max(8, int(0.2 * sh)) or h > int(2.0 * sh):
                continue
            if w > int(1.8 * sh) or area < max(20, int(0.01 * sh * sh)):
                continue
            if y == 0 and w > int(0.8 * sh):
                continue
            glyphs.append((x, y, w, h))
        if not glyphs:
            return None
        x = min(glyph[0] for glyph in glyphs)
        y = min(glyph[1] for glyph in glyphs)
        x2 = max(glyph[0] + glyph[2] for glyph in glyphs)
        y2 = max(glyph[1] + glyph[3] for glyph in glyphs)
        return x0 + x, y0 + y, x2 - x, y2 - y

    @staticmethod
    def _face_fraction(image: np.ndarray, x: int, y: int, w: int, h: int) -> float:
        pad = 8
        x0 = max(0, x - pad)
        y0 = max(0, y - pad)
        x1 = min(image.shape[1], x + w + pad)
        y1 = min(image.shape[0], y + h + pad)
        patch = image[y0:y1, x0:x1]
        if patch.size == 0:
            return 0.0
        b, g, r = cv2.split(patch)
        mx = np.maximum(np.maximum(r, g), b)
        mn = np.minimum(np.minimum(r, g), b)
        okay = (mx > 150) & ((mx.astype(int) - mn.astype(int)) < 35)
        return float(okay.mean())

    # ---------- 分类 ----------

    def _classify_rank(
        self, crop: np.ndarray
    ) -> Tuple[str, float, float]:
        if crop.size == 0:
            return "", 0.0, 0.0
        normalized = self._normalize_rank(crop)
        scores: List[Tuple[str, float]] = []
        for name, template in self.ranks.items():
            resized = cv2.resize(
                normalized,
                (template.shape[1], template.shape[0]),
                interpolation=cv2.INTER_AREA,
            )
            _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
            a = resized > 0
            b = template > 0
            iou = (a & b).sum() / max((a | b).sum(), 1)
            scores.append((name, float(iou)))
        scores.sort(key=lambda item: item[1], reverse=True)
        best_name, best_score = scores[0]
        second_score = scores[1][1] if len(scores) > 1 else 0.0
        return best_name, best_score, second_score

    def _classify_suit(
        self, crop: np.ndarray, color: str
    ) -> Tuple[str, float]:
        if crop.size == 0:
            return "", 0.0
        normalized = self._normalize_suit(crop)
        options = ("H", "D") if color == "red" else ("S", "C")
        best_name, best_score = "", 0.0
        for name in options:
            template = self.suits[name]
            a = normalized > 0
            b = template > 0
            iou = (a & b).sum() / max((a | b).sum(), 1)
            if iou > best_score:
                best_name, best_score = name, float(iou)
        return best_name, best_score

    # ---------- 大小王 ----------

    def _detect_jokers(
        self, image: np.ndarray, combined: np.ndarray, red: np.ndarray, dark: np.ndarray
    ) -> List[Detection]:
        """在三个固定高度带里找大小王。

        手牌/出牌区/底牌栏的 JOK 尺寸不同，全图多尺度匹配会占掉
        80% 的识别时间。这里按画面高度分带，并只尝试各带常见的尺度。
        """

        detections: List[Detection] = []
        if self.joker_template is None:
            return detections

        height, width = combined.shape
        base_scale = width / 1800.0
        bands = []
        if self.joker_compact is not None:
            # 底牌栏：压缩字形，画面上部
            bands.append(
                (0.00, 0.22, self.joker_compact, (0.90, 1.00, 1.10))
            )
        # 出牌区：约为手牌尺寸的 0.6 倍
        bands.append(
            (0.22, 0.50, self.joker_template, (0.54, 0.60, 0.66, 0.72))
        )
        # 手牌区
        bands.append(
            (0.62, 1.00, self.joker_template, (0.92, 1.00, 1.08))
        )

        for top_ratio, bottom_ratio, source, scale_priors in bands:
            y0 = int(height * top_ratio)
            y1 = min(height, int(height * bottom_ratio))
            if y1 <= y0 + 10:
                continue
            search = combined[y0:y1, :]
            base_h = source.shape[0]
            for prior in scale_priors:
                scale = prior * base_scale
                template_h = max(12, round(base_h * scale))
                template_w = max(
                    6, round(source.shape[1] * template_h / base_h)
                )
                if template_h >= search.shape[0] or template_w >= search.shape[1]:
                    continue
                template = cv2.resize(
                    source, (template_w, template_h), interpolation=cv2.INTER_AREA
                )
                _n, template = cv2.threshold(template, 96, 255, cv2.THRESH_BINARY)
                result = cv2.matchTemplate(
                    search, template, cv2.TM_CCOEFF_NORMED
                )
                ys, xs = np.where(result >= 0.62)
                for yy, xx in zip(ys, xs):
                    score = float(result[yy, xx])
                    x = int(xx)
                    y = int(yy) + y0
                    red_count = int(
                        (red[y:y + template_h, x:x + template_w] > 0).sum()
                    )
                    dark_count = int(
                        (dark[y:y + template_h, x:x + template_w] > 0).sum()
                    )
                    if max(red_count, dark_count) < 50:
                        continue
                    rank = "BJ" if red_count > dark_count else "SJ"
                    face = self._face_fraction(
                        image, x, y, template_w, template_h
                    )
                    if face < FACE_MIN_FRACTION:
                        continue
                    detections.append(
                        Detection(
                            rank=rank,
                            suit="",
                            score=score,
                            box=(x, y, template_w, template_h),
                            rank_score=score,
                            suit_score=1.0,
                            face_score=face,
                        )
                    )

        # 同一张王只保留最高分
        detections.sort(key=lambda d: d.score, reverse=True)
        kept: List[Detection] = []
        for det in detections:
            x, y, w, h = det.box
            cx, cy = x + w / 2, y + h / 2
            if any(
                abs(cx - (k.box[0] + k.box[2] / 2)) < max(w, k.box[2]) * 0.35
                and abs(cy - (k.box[1] + k.box[3] / 2)) < max(h, k.box[3]) * 0.25
                for k in kept
            ):
                continue
            kept.append(det)
        return kept

    # ---------- 对外接口 ----------

    def recognize(self, image: np.ndarray) -> RecognitionResult:
        import time

        started = time.perf_counter()
        red, dark = self._masks(image)
        combined = cv2.bitwise_or(red, dark)
        raw: List[Detection] = []

        # 花色候选：只要求“形状像花色”，后面再校验点数和卡面
        for color, mask in (("red", red), ("black", dark)):
            for sx, sy, sw, sh, area in self._components(mask):
                if not (12 <= sw <= 80 and 12 <= sh <= 85):
                    continue
                if not (0.65 <= sw / sh <= 1.45):
                    continue
                if area / (sw * sh) < 0.28 or area < 120:
                    continue

                rank_box = self._rank_bbox(mask, sx, sy, sw, sh)
                if rank_box is None:
                    continue
                rx, ry, rw, rh = rank_box
                rank_crop = mask[ry:ry + rh, rx:rx + rw]
                rank, rank_score, second_score = self._classify_rank(rank_crop)
                suit, suit_score = self._classify_suit(
                    mask[sy:sy + sh, sx:sx + sw], color
                )
                if rank_score < RANK_MIN_SCORE or suit_score < SUIT_MIN_SCORE:
                    continue
                if rank_score - second_score < RANK_MARGIN:
                    continue

                face = self._face_fraction(image, sx, sy, sw, sh)
                if face < FACE_MIN_FRACTION:
                    continue

                score = rank_score * 0.7 + suit_score * 0.3
                raw.append(
                    Detection(
                        rank=rank,
                        suit=suit,
                        score=float(score),
                        box=(sx, sy, sw, sh),
                        rank_score=rank_score,
                        suit_score=suit_score,
                        face_score=face,
                    )
                )

        # 红黑掩码可能同时命中同一张牌（红心外圈会形成黑色轮廓），按位置去重
        raw.sort(key=lambda d: d.score, reverse=True)
        kept: List[Detection] = []
        for det in raw:
            x, y, w, h = det.box
            cx, cy = x + w / 2, y + h / 2
            if any(
                abs(cx - (k.box[0] + k.box[2] / 2)) < min(w, k.box[2]) * 0.6
                and abs(cy - (k.box[1] + k.box[3] / 2)) < min(h, k.box[3]) * 0.6
                for k in kept
            ):
                continue
            kept.append(det)

        kept.extend(self._detect_jokers(image, combined, red, dark))

        elapsed_ms = (time.perf_counter() - started) * 1000
        return RecognitionResult(
            detections=sorted(kept, key=lambda d: (d.box[1] // 100, d.box[0])),
            elapsed_ms=elapsed_ms,
            frame_width=image.shape[1],
            frame_height=image.shape[0],
        )


def annotate(image: np.ndarray, detections: Sequence[Detection]) -> np.ndarray:
    out = image.copy()
    for det in detections:
        x, y, w, h = det.box
        cv2.rectangle(out, (x, y), (x + w, y + h), (0, 255, 0), 2)
        cv2.putText(
            out,
            det.label,
            (x, max(16, y - 5)),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.65,
            (0, 255, 255),
            2,
        )
    return out

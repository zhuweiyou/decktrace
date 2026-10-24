"""从已标注的对局截图生成牌面模板。

这是开发辅助脚本，正常运行不需要它。样本截图放在 shots/ 下，
牌序按截图中的手牌从左到右人工标注。
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import cv2
import numpy as np

ROOT = Path(__file__).resolve().parent.parent
SHOTS = ROOT / "shots"
OUT = ROOT / "jjcounter" / "templates"

# 每张样本截图的非王手牌，从左到右
SAMPLES: List[Tuple[str, List[str], Optional[str]]] = [
    (
        "round_now.png",
        ["2H", "2C", "AS", "AD", "KS", "JS", "10C", "10D",
         "9H", "9D", "7D", "6H", "5H", "5C", "3H", "3C"],
        "BJ",
    ),
    (
        "round_live2.png",
        ["2D", "KC", "KD", "JS", "JH", "JD", "9H", "9C", "9D", "8D",
         "7C", "7D", "6S", "6H", "5S", "5D", "4S", "3D"],
        None,
    ),
    (
        "round_live.png",
        ["2D", "KC", "KD", "QD", "JS", "JH", "JD", "9C", "8D", "7D",
         "6S", "6H", "5S", "5D", "4S", "3D"],
        "SJ",
    ),
]

# 每个点数选一张最干净的样本
RANK_CHOICES = {
    "2": ("round_now.png", "2H"),
    "3": ("round_now.png", "3C"),
    "4": ("round_live2.png", "4S"),
    "5": ("round_now.png", "5C"),
    "6": ("round_now.png", "6H"),
    "7": ("round_now.png", "7D"),
    "8": ("round_live2.png", "8D"),
    "9": ("round_now.png", "9H"),
    "10": ("round_now.png", "10D"),
    "J": ("round_now.png", "JS"),
    "Q": ("round_live.png", "QD"),
    "K": ("round_now.png", "KS"),
    "A": ("round_now.png", "AS"),
}
SUIT_CHOICES = {
    "H": ("round_now.png", "2H"),
    "D": ("round_live2.png", "2D"),
    "S": ("round_now.png", "AS"),
    "C": ("round_now.png", "2C"),
}

REF_SUIT_HEIGHT = 45.0
RANK_TPL_HEIGHT = 70


def masks(img: np.ndarray) -> Tuple[np.ndarray, np.ndarray]:
    b, g, r = cv2.split(img)
    red = (
        (r.astype(int) > 110)
        & (r.astype(int) - g.astype(int) > 45)
        & (r.astype(int) - b.astype(int) > 35)
    ).astype(np.uint8) * 255
    dark = ((r < 90) & (g < 90) & (b < 90)).astype(np.uint8) * 255
    return red, dark


def components(mask: np.ndarray):
    n, _labels, stats, _centroids = cv2.connectedComponentsWithStats(mask, 8)
    return [
        (int(x), int(y), int(w), int(h), int(area))
        for x, y, w, h, area in stats[1:]
    ]


def find_hand_suits(mask: np.ndarray):
    suits = []
    for x, y, w, h, area in components(mask):
        if (
            y > 870
            and y < 980
            and 10 <= w <= 70
            and 15 <= h <= 70
            and 0.70 <= w / h <= 1.30
            and area / (w * h) > 0.35
            and x > 100
        ):
            suits.append((x, y, w, h))
    return sorted(suits)


def rank_bbox(mask: np.ndarray, sx: int, sy: int, sw: int, sh: int):
    y0 = max(0, sy - int(2.2 * sh))
    y1 = max(1, sy - 2)
    x0 = max(0, sx - int(0.2 * sw))
    x1 = min(mask.shape[1], sx + int(1.45 * sw))
    sub = mask[y0:y1, x0:x1]
    glyphs = []
    for x, y, w, h, area in components(sub):
        if h < max(8, int(0.2 * sh)) or h > int(2.0 * sh):
            continue
        if w > int(1.8 * sh) or area < max(20, int(0.01 * sh * sh)):
            continue
        if y == 0 and w > int(0.8 * sh):
            continue
        glyphs.append((x, y, w, h))
    if not glyphs:
        return None
    x = min(g[0] for g in glyphs)
    y = min(g[1] for g in glyphs)
    x2 = max(g[0] + g[2] for g in glyphs)
    y2 = max(g[1] + g[3] for g in glyphs)
    return x0 + x, y0 + y, x2 - x, y2 - y


def clean_mask(crop: np.ndarray) -> Optional[np.ndarray]:
    """去掉卡牌边框/相邻牌边缘，只保留字形主体。"""

    glyphs = components(crop)
    if not glyphs:
        return None
    max_area = max(g[4] for g in glyphs)
    keep = []
    for x, y, w, h, area in glyphs:
        if area < max(12, max_area * 0.06):
            continue
        if w <= 3 or h <= 3:
            continue
        keep.append((x, y, w, h))
    if not keep:
        return None
    x = min(k[0] for k in keep)
    y = min(k[1] for k in keep)
    x2 = max(k[0] + k[2] for k in keep)
    y2 = max(k[1] + k[3] for k in keep)
    return crop[y:y2, x:x2]


def normalize_rank(crop: np.ndarray) -> np.ndarray:
    h, w = crop.shape[:2]
    nh = RANK_TPL_HEIGHT
    nw = max(1, round(w * nh / h))
    resized = cv2.resize(crop, (nw, nh), interpolation=cv2.INTER_AREA)
    _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
    return resized


def normalize_suit(crop: np.ndarray) -> np.ndarray:
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


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "ranks").mkdir(exist_ok=True)
    (OUT / "suits").mkdir(exist_ok=True)

    rank_samples: Dict[Tuple[str, str], np.ndarray] = {}
    suit_samples: Dict[Tuple[str, str], np.ndarray] = {}

    for filename, labels, joker in SAMPLES:
        img = cv2.imread(str(SHOTS / filename))
        if img is None:
            raise SystemExit(f"缺少截图 {filename}")
        red, dark = masks(img)
        cards = []
        for color, mask in (("red", red), ("black", dark)):
            for sx, sy, sw, sh in find_hand_suits(mask):
                rb = rank_bbox(mask, sx, sy, sw, sh)
                if not rb:
                    print(f"[warn] {filename} suit {sx},{sy} 未找到点数")
                    continue
                cards.append((sx, color, sx, sy, sw, sh, rb))
        cards.sort()
        if len(cards) != len(labels):
            raise SystemExit(
                f"{filename} 检测到 {len(cards)} 张，标注 {len(labels)} 张"
            )
        for (_, color, sx, sy, sw, sh, rb), label in zip(cards, labels):
            rx, ry, rw, rh = rb
            rank_mask = clean_mask(red[ry:ry + rh, rx:rx + rw]
                                   if color == "red"
                                   else dark[ry:ry + rh, rx:rx + rw])
            suit_mask = clean_mask(dark[sy:sy + sh, sx:sx + sw]
                                   if color == "black"
                                   else red[sy:sy + sh, sx:sx + sw])
            if rank_mask is None or suit_mask is None:
                print(f"[warn] {filename} {label} 掩码为空")
                continue
            rank_samples[(filename, label)] = rank_mask
            suit_samples[(filename, label)] = suit_mask

    for rank, (filename, label) in RANK_CHOICES.items():
        crop = rank_samples[(filename, label)]
        cv2.imwrite(str(OUT / "ranks" / f"{rank}.png"), normalize_rank(crop))
        print("rank", rank, crop.shape)

    for suit, (filename, label) in SUIT_CHOICES.items():
        crop = suit_samples[(filename, label)]
        cv2.imwrite(str(OUT / "suits" / f"{suit}.png"), normalize_suit(crop))
        print("suit", suit, crop.shape)

    # 大小王共用 JOK 竖排字形模板，用颜色区分
    img = cv2.imread(str(SHOTS / "round_now.png"))
    if img is not None:
        red, dark = masks(img)
        combined = cv2.bitwise_or(red, dark)
        roi = combined[800:950, 28:84]
        cleaned = clean_mask(roi)
        if cleaned is not None:
            h, w = cleaned.shape
            nh = 130
            nw = max(1, round(w * nh / h))
            tpl = cv2.resize(cleaned, (nw, nh), interpolation=cv2.INTER_AREA)
            _n, tpl = cv2.threshold(tpl, 96, 255, cv2.THRESH_BINARY)
            cv2.imwrite(str(OUT / "joker.png"), tpl)
            print("joker", cleaned.shape, "->", tpl.shape)

    meta = {
        "ref_suit_height": REF_SUIT_HEIGHT,
        "rank_template_height": RANK_TPL_HEIGHT,
        "joker_scale_reference": 130 / 130,
        "ranks": sorted(RANK_CHOICES),
        "suits": sorted(SUIT_CHOICES),
    }
    (OUT / "meta.json").write_text(
        json.dumps(meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print("saved to", OUT)


if __name__ == "__main__":
    main()

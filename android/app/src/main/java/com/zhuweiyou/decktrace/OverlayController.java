package com.zhuweiyou.decktrace;

import android.content.Context;
import android.graphics.PixelFormat;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.WindowManager;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/** 管理顶部悬浮窗的添加/更新/移除（必须在主线程操作 View）。 */
public class OverlayController {
    private final Context context;
    private final Handler ui = new Handler(Looper.getMainLooper());
    private WindowManager wm;
    private OverlayView view;
    private WindowManager.LayoutParams lp;
    private int screenW;
    private int screenH;

    public OverlayController(Context context) {
        this.context = context;
    }

    public void show() {
        ui.post(() -> {
            if (view != null) {
                return;
            }
            try {
                OverlayView v = new OverlayView(context);
                wm = (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
                android.util.DisplayMetrics dm = new android.util.DisplayMetrics();
                wm.getDefaultDisplay().getRealMetrics(dm);
                screenW = dm.widthPixels;
                screenH = dm.heightPixels;
                lp = new WindowManager.LayoutParams(
                        WindowManager.LayoutParams.WRAP_CONTENT,
                        WindowManager.LayoutParams.WRAP_CONTENT,
                        WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                                | WindowManager.LayoutParams.FLAG_SECURE
                                | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                                | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
                        PixelFormat.TRANSLUCENT);
                lp.gravity = Gravity.TOP | Gravity.LEFT;
                lp.x = Math.max(0, screenW - 460);
                lp.y = 0;
                v.setOnDrag(this::move);
                wm.addView(v, lp);
                view = v;
            } catch (Throwable t) {
                view = null;
            }
        });
    }

    /** 拖动悬浮窗：按增量移动窗口位置。 */
    private void move(float dx, float dy) {
        if (view == null || lp == null) {
            return;
        }
        int vw = view.getWidth() > 0 ? view.getWidth() : 460;
        int vh = view.getHeight() > 0 ? view.getHeight() : 130;
        lp.x = Math.max(0, Math.min(lp.x + Math.round(dx), Math.max(0, screenW - vw)));
        lp.y = Math.max(0, Math.min(lp.y + Math.round(dy), Math.max(0, screenH - vh)));
        try {
            wm.updateViewLayout(view, lp);
        } catch (Throwable ignored) {
        }
    }

    public void update(String snapshot) {
        Snapshot s = Snapshot.parse(snapshot);
        ui.post(() -> {
            if (view != null) {
                view.setData(s.cards, s.bombs, s.bottom, s.status());
            }
        });
    }

    public void hide() {
        ui.post(() -> {
            if (view != null) {
                try {
                    wm.removeView(view);
                } catch (Throwable ignored) {
                }
                view = null;
            }
        });
    }

    /** 解析 Rust 端返回的快照字符串。 */
    static final class Snapshot {
        boolean armed;
        int det;
        int bd;
        int seen;
        int remaining;
        final List<String> cards = new ArrayList<>();
        final Set<String> bombs = new HashSet<>();
        final Set<String> bottom = new HashSet<>();

        static Snapshot parse(String text) {
            Snapshot s = new Snapshot();
            if (text == null) {
                return s;
            }
            for (String part : text.split(";")) {
                int idx = part.indexOf(':');
                if (idx < 0) {
                    continue;
                }
                String key = part.substring(0, idx);
                String val = part.substring(idx + 1);
                switch (key) {
                    case "armed":
                        s.armed = "1".equals(val);
                        break;
                    case "det":
                        s.det = parseInt(val);
                        break;
                    case "bd":
                        s.bd = parseInt(val);
                        break;
                    case "seen":
                        s.seen = parseInt(val);
                        break;
                    case "remaining":
                        s.remaining = parseInt(val);
                        break;
                    case "cards":
                        Collections.addAll(s.cards, splitNonEmpty(val));
                        break;
                    case "bombs":
                        Collections.addAll(s.bombs, splitNonEmpty(val));
                        break;
                    case "bottom":
                        Collections.addAll(s.bottom, splitNonEmpty(val));
                        break;
                    default:
                        break;
                }
            }
            return s;
        }

        String status() {
            String base = "armed=" + (armed ? 1 : 0) + " seen=" + seen + " 剩=" + remaining
                    + " det=" + det + " 底=" + bd;
            if (det == 0) {
                return base + " 未识别到画面";
            }
            return base;
        }

        private static int parseInt(String value) {
            try {
                return Integer.parseInt(value.trim());
            } catch (NumberFormatException e) {
                return 0;
            }
        }

        private static String[] splitNonEmpty(String value) {
            return value.isEmpty() ? new String[0] : value.split(",");
        }
    }
}

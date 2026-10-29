package com.zhuweiyou.decktrace;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.View;

import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** 顶部记牌条：两排「点数 → 剩余未出张数」表格（不显示花色）。 */
public class OverlayView extends View {
    // 按大小顺序（大王 小王 2 A K Q J 10 9 8 7 6 5 4 3），分两排：前 8 个 / 后 7 个。
    private static final String[] RANKS =
            {"BJ", "SJ", "2", "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3"};
    private static final int ROW0 = 8;

    private static final int CW = 46;   // 单元格宽
    private static final int CH = 40;   // 单元格高
    private static final int GAP = 2;
    private static final int PAD = 4;

    private final Paint bgPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint labelPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint countPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF rect = new RectF();

    private List<String> cards = Collections.emptyList();

    public interface OnDrag {
        void onDrag(float dx, float dy);
        void onDragEnd();
    }

    private OnDrag onDrag;
    private float lastX;
    private float lastY;

    public void setOnDrag(OnDrag listener) { this.onDrag = listener; }

    @Override
    public boolean onTouchEvent(android.view.MotionEvent event) {
        switch (event.getActionMasked()) {
            case android.view.MotionEvent.ACTION_DOWN:
                lastX = event.getRawX(); lastY = event.getRawY(); return true;
            case android.view.MotionEvent.ACTION_MOVE:
                if (onDrag != null) onDrag.onDrag(event.getRawX() - lastX, event.getRawY() - lastY);
                lastX = event.getRawX(); lastY = event.getRawY(); return true;
            case android.view.MotionEvent.ACTION_UP:
            case android.view.MotionEvent.ACTION_CANCEL:
                if (onDrag != null) onDrag.onDragEnd();
                return true;
            default:
                return true;
        }
    }

    public OverlayView(Context context) {
        super(context);
        setBackgroundColor(0xCC0A0E14);
        bgPaint.setColor(0xFF1B2432);
        labelPaint.setColor(0xFFB9C4D4);
        labelPaint.setTextSize(12f);
        labelPaint.setTextAlign(Paint.Align.CENTER);
        countPaint.setColor(0xFFFFFFFF);
        countPaint.setTextSize(20f);
        countPaint.setFakeBoldText(true);
        countPaint.setTextAlign(Paint.Align.CENTER);
    }

    public void setData(List<String> cards, Set<String> bombs, Set<String> bottom) {
        this.cards = cards != null ? cards : Collections.emptyList();
        requestLayout();
        invalidate();
    }

    /** 从牌号（如 "10S" / "AH" / "BJ"）取点数。 */
    private static String rankOf(String id) {
        if (id == null) return "";
        if (id.equals("BJ") || id.equals("SJ")) return id;
        if (id.length() >= 2) return id.substring(0, id.length() - 1);
        return id;
    }

    private Map<String, Integer> remainingByRank() {
        Map<String, Integer> m = new HashMap<>();
        for (String id : cards) {
            String r = rankOf(id);
            m.put(r, m.getOrDefault(r, 0) + 1);
        }
        return m;
    }

    private static int totalOf(String rank) {
        return (rank.equals("BJ") || rank.equals("SJ")) ? 1 : 4;
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int width = ROW0 * (CW + GAP) + PAD * 2;
        int height = 2 * (CH + GAP) + PAD * 2;
        setMeasuredDimension(width, height);
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        Map<String, Integer> rem = remainingByRank();
        for (int i = 0; i < RANKS.length; i++) {
            int row = i < ROW0 ? 0 : 1;
            int col = i < ROW0 ? i : (i - ROW0);
            float x = PAD + col * (CW + GAP);
            float y = PAD + row * (CH + GAP);
            rect.set(x, y, x + CW, y + CH);
            canvas.drawRoundRect(rect, 5f, 5f, bgPaint);

            String rank = RANKS[i];
            int left = rem.getOrDefault(rank, 0);
            boolean isJoker = rank.equals("BJ") || rank.equals("SJ");
            String label = rank.equals("BJ") ? "大王" : rank.equals("SJ") ? "小王" : rank;

            canvas.drawText(label, x + CW / 2f, y + 14f, labelPaint);
            int color = left == 0 ? 0xFF6B7686 : 0xFFFFFFFF;
            countPaint.setColor(color);
            canvas.drawText(String.valueOf(left), x + CW / 2f, y + CH - 8f, countPaint);
        }
    }
}

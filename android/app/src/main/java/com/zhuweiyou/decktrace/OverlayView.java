package com.zhuweiyou.decktrace;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.ColorMatrix;
import android.graphics.ColorMatrixColorFilter;
import android.graphics.Paint;
import android.graphics.Rect;
import android.view.View;

import java.io.IOException;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** 顶部记牌条：按剩余牌绘制，炸弹反色，底牌加“底”印。 */
public class OverlayView extends View {
    private static final int CARD_W = 30;
    private static final int CARD_H = 60;
    private static final int GAP = 1;
    private static final int PAD = 4;
    private static final int MAX_COLS = 64;

    private final Map<String, Bitmap> normal = new HashMap<>();
    private final Map<String, Bitmap> inverted = new HashMap<>();
    private Bitmap badge;
    private final Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG);
    private final Paint bottomPaint = new Paint();

    private List<String> cards = Collections.emptyList();
    private Set<String> bombs = Collections.emptySet();
    private Set<String> bottom = Collections.emptySet();

    public interface OnDrag {
        void onDrag(float dx, float dy);

        void onDragEnd();
    }

    private OnDrag onDrag;
    private float lastX;
    private float lastY;

    public void setOnDrag(OnDrag listener) {
        this.onDrag = listener;
    }

    @Override
    public boolean onTouchEvent(android.view.MotionEvent event) {
        switch (event.getActionMasked()) {
            case android.view.MotionEvent.ACTION_DOWN:
                lastX = event.getRawX();
                lastY = event.getRawY();
                return true;
            case android.view.MotionEvent.ACTION_MOVE:
                if (onDrag != null) {
                    onDrag.onDrag(event.getRawX() - lastX, event.getRawY() - lastY);
                }
                lastX = event.getRawX();
                lastY = event.getRawY();
                return true;
            case android.view.MotionEvent.ACTION_UP:
            case android.view.MotionEvent.ACTION_CANCEL:
                if (onDrag != null) {
                    onDrag.onDragEnd();
                }
                return true;
            default:
                return true;
        }
    }

    public OverlayView(Context context) {
        super(context);
        setBackgroundColor(0xCC0A0E14);
        bottomPaint.setStyle(Paint.Style.STROKE);
        bottomPaint.setStrokeWidth(2.5f);
        bottomPaint.setColor(0xFFE23B3B); // 朱红
        try (InputStream in = context.getAssets().open("bottom_badge.png")) {
            badge = BitmapFactory.decodeStream(in);
        } catch (IOException ignored) {
        }
    }

    public void setData(List<String> cards, Set<String> bombs, Set<String> bottom) {
        this.cards = cards != null ? cards : Collections.emptyList();
        this.bombs = bombs != null ? bombs : Collections.emptySet();
        this.bottom = bottom != null ? bottom : Collections.emptySet();
        requestLayout();
        invalidate();
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int count = Math.max(cards.size(), 1);
        int cols = Math.min(count, MAX_COLS);
        int rows = (count + MAX_COLS - 1) / MAX_COLS;
        int width = cols * (CARD_W + GAP) + PAD * 2;
        int height = rows * (CARD_H + GAP) + PAD * 2;
        setMeasuredDimension(Math.max(width, 1), height);
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        for (int i = 0; i < cards.size(); i++) {
            String id = cards.get(i);
            int col = i % MAX_COLS;
            int row = i / MAX_COLS;
            int x = PAD + col * (CARD_W + GAP);
            int y = PAD + row * (CARD_H + GAP);
            Bitmap bitmap = bombs.contains(id) ? invertedBitmap(id) : normalBitmap(id);
            if (bitmap != null) {
                Rect dst = new Rect(x, y, x + CARD_W, y + CARD_H);
                canvas.drawBitmap(bitmap, null, dst, paint);
            }
        }

    }

    private Bitmap normalBitmap(String id) {
        if (normal.containsKey(id)) {
            return normal.get(id);
        }
        Bitmap bitmap = load(id);
        normal.put(id, bitmap);
        return bitmap;
    }

    private Bitmap invertedBitmap(String id) {
        if (inverted.containsKey(id)) {
            return inverted.get(id);
        }
        Bitmap src = normalBitmap(id);
        Bitmap out = null;
        if (src != null) {
            out = Bitmap.createBitmap(src.getWidth(), src.getHeight(), Bitmap.Config.ARGB_8888);
            Canvas canvas = new Canvas(out);
            Paint p = new Paint();
            ColorMatrix cm = new ColorMatrix(new float[]{
                    -1, 0, 0, 0, 255,
                    0, -1, 0, 0, 255,
                    0, 0, -1, 0, 255,
                    0, 0, 0, 1, 0
            });
            p.setColorFilter(new ColorMatrixColorFilter(cm));
            canvas.drawBitmap(src, 0, 0, p);
        }
        inverted.put(id, out);
        return out;
    }

    private Bitmap load(String id) {
        try (InputStream in = getContext().getAssets().open("cards/" + id + ".png")) {
            return BitmapFactory.decodeStream(in);
        } catch (IOException e) {
            return null;
        }
    }
}

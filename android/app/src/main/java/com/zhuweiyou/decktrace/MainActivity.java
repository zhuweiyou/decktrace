package com.zhuweiyou.decktrace;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.media.projection.MediaProjectionManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.view.Gravity;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

/**
 * 入口页：深色卡片式界面。
 * 第一组：悬浮窗授权 + 停止；第二组：选择游戏并开始截屏。
 */
public class MainActivity extends Activity {
    private static final int REQ_CAPTURE = 1001;
    private static final int REQ_NOTIF = 1002;

    private static final int BG_TOP = 0xFF0C1018;
    private static final int BG_BOTTOM = 0xFF141C29;
    private static final int CARD_BG = 0xFF18212F;
    private static final int CARD_STROKE = 0xFF2A3547;
    private static final int MUTED = 0xFF93A3BA;
    private static final int PRIMARY = 0xFF2F6BFF;
    private static final int DANGER = 0xFFE5484D;
    private static final int GAME_BG = 0xFF21344F;
    private static final int GAME_STROKE = 0xFF3A5378;
    private static final int GAME_FG = 0xFFDCE7FF;

    private TextView status;
    private MediaProjectionManager mpm;
    private String pendingGame = null;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        mpm = (MediaProjectionManager) getSystemService(MEDIA_PROJECTION_SERVICE);

        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackground(new GradientDrawable(
                GradientDrawable.Orientation.TOP_BOTTOM, new int[]{BG_TOP, BG_BOTTOM}));

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(20), dp(28), dp(20), dp(28));
        scroll.addView(root, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        TextView title = new TextView(this);
        title.setText("斗地主记牌器");
        title.setTextColor(0xFFFFFFFF);
        title.setTextSize(26);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        root.addView(title);

        TextView subtitle = new TextView(this);
        subtitle.setText("选择游戏 · 自动识别 · 悬浮窗记牌");
        subtitle.setTextColor(MUTED);
        subtitle.setTextSize(13);
        subtitle.setPadding(0, dp(4), 0, dp(16));
        root.addView(subtitle);

        status = new TextView(this);
        status.setTextColor(0xFFB8C4D8);
        status.setTextSize(13);
        status.setPadding(dp(12), dp(10), dp(12), dp(10));
        status.setBackground(rounded(0xFF111A27, dp(12), 0xFF26313F));
        LinearLayout.LayoutParams slp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        slp.setMargins(0, 0, 0, dp(8));
        root.addView(status, slp);

        // 第一组：悬浮窗授权 + 停止
        LinearLayout card1 = card();
        card1.addView(sectionLabel("① 悬浮窗 / 停止"));
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_HORIZONTAL);
        Button perm = textButton("授权悬浮窗", PRIMARY, 0xFFFFFFFF);
        perm.setOnClickListener(v -> requestOverlay());
        Button stop = textButton("停止", DANGER, 0xFFFFFFFF);
        stop.setOnClickListener(v -> stopCapture());
        LinearLayout.LayoutParams half = new LinearLayout.LayoutParams(0,
                ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        half.setMargins(dp(4), dp(6), dp(4), dp(6));
        LinearLayout.LayoutParams half2 = new LinearLayout.LayoutParams(0,
                ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        half2.setMargins(dp(4), dp(6), dp(4), dp(6));
        row.addView(perm, half);
        row.addView(stop, half2);
        card1.addView(row);
        root.addView(card1);

        // 第二组：选择游戏开始截屏
        LinearLayout card2 = card();
        card2.addView(sectionLabel("② 选择游戏开始截屏"));
        card2.addView(gameButton("小程序欢乐斗地主", "hlddz_mobile"));
        card2.addView(gameButton("王者荣耀斗地主", "wzryddz"));
        TextView hint = new TextView(this);
        hint.setText("选中游戏后会请求屏幕采集授权");
        hint.setTextColor(0xFF6F7E93);
        hint.setTextSize(12);
        hint.setPadding(dp(4), dp(10), 0, 0);
        card2.addView(hint);
        root.addView(card2);

        setContentView(scroll);
        requestNotifPermission();
        refreshStatus();
    }

    private int dp(float v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }

    private GradientDrawable rounded(int color, int radius, int stroke) {
        GradientDrawable g = new GradientDrawable();
        g.setColor(color);
        g.setCornerRadius(radius);
        if (stroke != 0) {
            g.setStroke(dp(1), stroke);
        }
        return g;
    }

    private LinearLayout card() {
        LinearLayout c = new LinearLayout(this);
        c.setOrientation(LinearLayout.VERTICAL);
        c.setPadding(dp(16), dp(14), dp(16), dp(14));
        c.setBackground(rounded(CARD_BG, dp(18), CARD_STROKE));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.setMargins(0, dp(8), 0, dp(8));
        c.setLayoutParams(lp);
        return c;
    }

    private TextView sectionLabel(String text) {
        TextView tv = new TextView(this);
        tv.setText(text);
        tv.setTextColor(0xFFAFC0D8);
        tv.setTextSize(14);
        tv.setTypeface(Typeface.DEFAULT_BOLD);
        tv.setPadding(dp(2), 0, 0, dp(6));
        return tv;
    }

    private Button textButton(String text, int bg, int fg) {
        Button b = new Button(this);
        b.setText(text);
        b.setTextSize(16);
        b.setTextColor(fg);
        b.setAllCaps(false);
        b.setMinHeight(dp(50));
        b.setPadding(dp(10), dp(10), dp(10), dp(10));
        b.setBackground(rounded(bg, dp(14), 0));
        b.setStateListAnimator(null);
        return b;
    }

    private Button gameButton(String text, final String gameId) {
        Button b = textButton(text, GAME_BG, GAME_FG);
        b.setBackground(rounded(GAME_BG, dp(14), GAME_STROKE));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.setMargins(0, dp(6), 0, dp(6));
        b.setLayoutParams(lp);
        b.setOnClickListener(v -> startCapture(gameId));
        return b;
    }

    private void requestNotifPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                    != PackageManager.PERMISSION_GRANTED) {
                requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, REQ_NOTIF);
            }
        }
    }

    private void requestOverlay() {
        if (Settings.canDrawOverlays(this)) {
            status.setText("悬浮窗权限已授予");
            return;
        }
        Intent intent = new Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                Uri.parse("package:" + getPackageName()));
        startActivity(intent);
    }

    /** 选择某个游戏后请求屏幕采集授权，授权成功即启动对应游戏的采集。 */
    private void startCapture(String gameId) {
        if (!Settings.canDrawOverlays(this)) {
            status.setText("请先授予悬浮窗权限");
            return;
        }
        pendingGame = gameId;
        status.setText("请求屏幕采集授权（" + gameName(gameId) + "）…");
        Intent intent = mpm.createScreenCaptureIntent();
        startActivityForResult(intent, REQ_CAPTURE);
    }

    private void stopCapture() {
        Intent svc = new Intent(this, CaptureService.class);
        svc.setAction(CaptureService.ACTION_STOP);
        startService(svc);
        status.setText("已请求停止采集");
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQ_CAPTURE) {
            if (resultCode == RESULT_OK && data != null) {
                String gameId = pendingGame == null ? "hlddz_mobile" : pendingGame;
                Intent svc = new Intent(this, CaptureService.class);
                svc.setAction(CaptureService.ACTION_START);
                svc.putExtra(CaptureService.EXTRA_CODE, resultCode);
                svc.putExtra(CaptureService.EXTRA_DATA, data);
                svc.putExtra(CaptureService.EXTRA_GAME, gameId);
                startForegroundService(svc);
                status.setText("采集中（" + gameName(gameId) + "），悬浮窗应已出现在屏幕顶部");
            } else {
                status.setText("用户取消了屏幕采集授权");
            }
        }
    }

    private static String gameName(String id) {
        if ("wzryddz".equals(id)) {
            return "王者荣耀斗地主";
        }
        if ("hlddz_mobile".equals(id)) {
            return "小程序欢乐斗地主";
        }
        return id;
    }

    private void refreshStatus() {
        boolean overlay = Settings.canDrawOverlays(this);
        status.setText(overlay ? "悬浮窗权限已授予，点下面游戏按钮开始" : "请先点“授权悬浮窗”");
    }
}

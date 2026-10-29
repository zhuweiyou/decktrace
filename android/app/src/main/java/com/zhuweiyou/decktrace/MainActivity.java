package com.zhuweiyou.decktrace;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.media.projection.MediaProjectionManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.view.Gravity;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * 入口页：
 * 第一组：悬浮窗授权 + 停止；
 * 第二组：选择游戏（小程序欢乐斗地主 / 王者荣耀斗地主）并开始截屏。
 */
public class MainActivity extends Activity {
    private static final int REQ_CAPTURE = 1001;
    private static final int REQ_NOTIF = 1002;

    private TextView status;
    private MediaProjectionManager mpm;
    /** 等待屏幕采集授权期间记住用户选的游戏。 */
    private String pendingGame = null;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        mpm = (MediaProjectionManager) getSystemService(MEDIA_PROJECTION_SERVICE);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setGravity(Gravity.CENTER_HORIZONTAL);
        root.setPadding(48, 48, 48, 48);
        root.setBackgroundColor(0xFF10141C);

        TextView title = new TextView(this);
        title.setText("斗地主记牌器");
        title.setTextColor(0xFFFFFFFF);
        title.setTextSize(22);
        title.setPadding(0, 0, 0, 24);
        root.addView(title);

        status = new TextView(this);
        status.setTextColor(0xFFB8C4D8);
        status.setTextSize(15);
        status.setPadding(0, 0, 0, 24);
        root.addView(status, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // 第一组：悬浮窗授权 + 停止
        root.addView(groupLabel("① 悬浮窗 / 停止"));
        LinearLayout row1 = new LinearLayout(this);
        row1.setOrientation(LinearLayout.HORIZONTAL);
        row1.setGravity(Gravity.CENTER_HORIZONTAL);
        row1.addView(makeButton("授权悬浮窗", v -> requestOverlay(), 240));
        row1.addView(makeButton("停止", v -> stopCapture(), 240));
        root.addView(row1);

        // 第二组：选择游戏开始截屏
        root.addView(groupLabel("② 选择游戏开始截屏"));
        root.addView(makeButton("小程序欢乐斗地主", v -> startCapture("hlddz_mobile"), 520));
        root.addView(makeButton("王者荣耀斗地主", v -> startCapture("wzryddz"), 520));

        setContentView(root);
        requestNotifPermission();
        refreshStatus();
    }

    private TextView groupLabel(String text) {
        TextView tv = new TextView(this);
        tv.setText(text);
        tv.setTextColor(0xFF8FA3C0);
        tv.setTextSize(13);
        tv.setPadding(0, 24, 0, 8);
        return tv;
    }

    private Button makeButton(String text, android.view.View.OnClickListener l, int width) {
        Button b = new Button(this);
        b.setText(text);
        b.setTextSize(16);
        b.setOnClickListener(l);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                width, ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.setMargins(6, 12, 6, 12);
        b.setLayoutParams(lp);
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

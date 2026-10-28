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

/** 入口：授权悬浮窗 + 请求屏幕采集 + 启动/停止采集服务。 */
public class MainActivity extends Activity {
    private static final int REQ_CAPTURE = 1001;
    private static final int REQ_NOTIF = 1002;

    private TextView status;
    private MediaProjectionManager mpm;

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

        root.addView(makeButton("1. 授权悬浮窗", v -> requestOverlay()));
        root.addView(makeButton("2. 开始采集", v -> startCapture()));

        setContentView(root);
        requestNotifPermission();
        refreshStatus();
    }

    private Button makeButton(String text, android.view.View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(text);
        b.setTextSize(16);
        b.setOnClickListener(l);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                520, ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.setMargins(0, 12, 0, 12);
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

    private void startCapture() {
        if (!Settings.canDrawOverlays(this)) {
            status.setText("请先授予悬浮窗权限");
            return;
        }
        Intent intent = mpm.createScreenCaptureIntent();
        startActivityForResult(intent, REQ_CAPTURE);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQ_CAPTURE) {
            if (resultCode == RESULT_OK && data != null) {
                Intent svc = new Intent(this, CaptureService.class);
                svc.setAction(CaptureService.ACTION_START);
                svc.putExtra(CaptureService.EXTRA_CODE, resultCode);
                svc.putExtra(CaptureService.EXTRA_DATA, data);
                startForegroundService(svc);
                status.setText("采集中，悬浮窗应已出现在屏幕顶部");
            } else {
                status.setText("用户取消了屏幕采集授权");
            }
        }
    }

    private void refreshStatus() {
        boolean overlay = Settings.canDrawOverlays(this);
        status.setText(overlay ? "悬浮窗权限已授予，点“开始采集”" : "请先点“授权悬浮窗”");
    }
}

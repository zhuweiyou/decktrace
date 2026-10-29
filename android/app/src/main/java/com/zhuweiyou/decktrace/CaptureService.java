package com.zhuweiyou.decktrace;

import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.graphics.PixelFormat;
import android.hardware.display.DisplayManager;
import android.hardware.display.VirtualDisplay;
import android.media.Image;
import android.media.ImageReader;
import android.media.projection.MediaProjection;
import android.media.projection.MediaProjectionManager;
import android.os.Build;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.IBinder;
import android.os.SystemClock;
import android.util.DisplayMetrics;
import android.view.Display;
import android.view.WindowManager;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.ByteBuffer;

/** 前台服务：MediaProjection 抓全屏 -> JNI 识别 -> 更新悬浮窗。 */
public class CaptureService extends Service {
    public static final String ACTION_START = "com.zhuweiyou.decktrace.START";
    public static final String ACTION_STOP = "com.zhuweiyou.decktrace.STOP";
    public static final String ACTION_RESET = "com.zhuweiyou.decktrace.RESET";
    public static final String EXTRA_CODE = "code";
    public static final String EXTRA_DATA = "data";
    public static final String EXTRA_GAME = "game";

    /**
     * 调试用：把采集原始帧落盘到 filesDir/frame.raw（供 adb run-as 取出核对）。
     * 默认关闭；只在排查采集/识别问题时临时改 true，正式使用不要开。
     */
    private static final boolean DEBUG_DUMP_FRAME = false;

    private MediaProjection projection;
    private VirtualDisplay virtualDisplay;
    private ImageReader reader;
    private HandlerThread thread;
    private Handler handler;
    private OverlayController overlay;
    private DisplayManager displayManager;
    private DisplayManager.DisplayListener displayListener;
    private WindowManager windowManager;
    private long handle = 0;
    private long lastProcessMs = 0;
    private int frameCount = 0;
    private long capStartMs = 0;
    private int capCount = 0;
    private int width, height, density;
    private boolean capturing = false;
    private volatile boolean running = false;

    @Override
    public void onCreate() {
        super.onCreate();
        createChannel();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            return START_NOT_STICKY;
        }
        String action = intent.getAction();
        android.util.Log.d("decktrace", "onStartCommand action=" + action);
        if (ACTION_STOP.equals(action)) {
            stopEverything();
            return START_NOT_STICKY;
        }
        if (ACTION_RESET.equals(action)) {
            if (handle != 0) {
                NativeBridge.reset(handle);
            }
            return START_STICKY;
        }
        if (ACTION_START.equals(action)) {
            startCapture(intent);
        }
        return START_STICKY;
    }

    private void startCapture(Intent intent) {
        if (running) {
            return;
        }
        Notification notification = buildNotification();
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION);
        } else {
            startForeground(1, notification);
        }

        int code = intent.getIntExtra(EXTRA_CODE, Activity.RESULT_CANCELED);
        Intent data = getParcelable(intent);
        MediaProjectionManager mpm =
                (MediaProjectionManager) getSystemService(Context.MEDIA_PROJECTION_SERVICE);
        projection = mpm.getMediaProjection(code, data);
        if (projection == null) {
            stopEverything();
            return;
        }

        String gameId = intent.getStringExtra(EXTRA_GAME);
        if (gameId == null || gameId.trim().isEmpty()) {
            gameId = "wzryddz";
        }
        String assetsRoot = new File(getFilesDir(), gameId).getAbsolutePath();
        File templatesDir = new File(assetsRoot, "templates");
        copyAssetDir("templates", templatesDir);
        templatesDir.mkdirs();
        handle = NativeBridge.create(assetsRoot, gameId);
        android.util.Log.d("decktrace", "create game=" + gameId + " assets=" + assetsRoot);
        if (handle == 0) {
            stopEverything();
            return;
        }

        thread = new HandlerThread("capture");
        thread.start();
        handler = new Handler(thread.getLooper());
        windowManager = (WindowManager) getSystemService(WINDOW_SERVICE);
        overlay = new OverlayController(this);

        projection.registerCallback(new MediaProjection.Callback() {
            @Override
            public void onStop() {
                stopEverything();
            }
        }, handler);

        watchRotation();
        refreshCaptureState();
        running = true;
    }

    /**
     * 只在横屏采集：游戏都是横屏玩，竖屏说明不在游戏中，直接停采并隐藏悬浮窗。
     */
    private void refreshCaptureState() {
        if (projection == null || windowManager == null || handler == null) {
            return;
        }
        DisplayMetrics dm = new DisplayMetrics();
        windowManager.getDefaultDisplay().getRealMetrics(dm);
        boolean landscape = dm.widthPixels >= dm.heightPixels;
        android.util.Log.d("decktrace", "refresh w=" + dm.widthPixels + " h=" + dm.heightPixels
                + " land=" + landscape + " capturing=" + capturing);
        if (landscape) {
            if (!capturing || dm.widthPixels != width || dm.heightPixels != height
                    || dm.densityDpi != density) {
                setupCapture(dm.widthPixels, dm.heightPixels, dm.densityDpi);
            }
            if (overlay != null) {
                overlay.show();
            }
        } else {
            if (capturing) {
                teardownCapture();
            }
            if (overlay != null) {
                overlay.hide();
            }
        }
    }

    private void teardownCapture() {
        capturing = false;
        try {
            if (reader != null) {
                reader.close();
                reader = null;
            }
            if (virtualDisplay != null) {
                virtualDisplay.release();
                virtualDisplay = null;
            }
        } catch (Throwable ignored) {
        }
    }

    /** 建立/重建 VirtualDisplay 与 ImageReader（旋转或尺寸变化时调用）。 */
    private void setupCapture(int w, int h, int d) {
        width = w;
        height = h;
        density = d;
        android.util.Log.d("decktrace", "setupCapture " + w + "x" + h + " d=" + d);
        if (reader != null) {
            try {
                reader.close();
            } catch (Throwable ignored) {
            }
            reader = null;
        }
        reader = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 2);
        reader.setOnImageAvailableListener(onImageAvailable, handler);
        if (virtualDisplay == null) {
            virtualDisplay = projection.createVirtualDisplay(
                    "decktrace", w, h, d,
                    DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                    reader.getSurface(), null, handler);
        } else {
            virtualDisplay.resize(w, h, d);
            virtualDisplay.setSurface(reader.getSurface());
        }
        capturing = true;
    }

    /** 监听显示旋转/尺寸变化，重建采集面，避免横竖屏切换后画面变形。 */
    private void watchRotation() {
        displayManager = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        displayListener = new DisplayManager.DisplayListener() {
            @Override
            public void onDisplayAdded(int displayId) {
            }

            @Override
            public void onDisplayRemoved(int displayId) {
            }

            @Override
            public void onDisplayChanged(int displayId) {
                android.util.Log.d("decktrace", "onDisplayChanged " + displayId);
                if (displayId != Display.DEFAULT_DISPLAY || handler == null) {
                    return;
                }
                handler.post(CaptureService.this::refreshCaptureState);
            }
        };
        displayManager.registerDisplayListener(displayListener, handler);
    }

    private Intent getParcelable(Intent intent) {
        if (Build.VERSION.SDK_INT >= 33) {
            return intent.getParcelableExtra(EXTRA_DATA, Intent.class);
        }
        return intent.getParcelableExtra(EXTRA_DATA);
    }

    private final ImageReader.OnImageAvailableListener onImageAvailable =
            new ImageReader.OnImageAvailableListener() {
        @Override
        public void onImageAvailable(ImageReader r) {
            Image image = null;
            try {
                image = r.acquireLatestImage();
                if (image == null) {
                    return;
                }
                long now = SystemClock.elapsedRealtime();
                capCount++;
                if (capStartMs == 0) {
                    capStartMs = now;
                }
                if (now - capStartMs >= 2000) {
                    double sec = (now - capStartMs) / 1000.0;
                    android.util.Log.d("decktrace", "capture fps=" + (capCount / sec)
                            + " frames=" + capCount + " cost=" + sec + "s");
                    capStartMs = now;
                    capCount = 0;
                }
                if (now - lastProcessMs < 80) { // 约 12fps，识别要占 CPU
                    return;
                }
                lastProcessMs = now;
                Image.Plane plane = image.getPlanes()[0];
                ByteBuffer buffer = plane.getBuffer();
                int rowStride = plane.getRowStride();
                byte[] bytes = new byte[buffer.remaining()];
                buffer.get(bytes);
                String snapshot = NativeBridge.pushFrame(
                        handle, bytes, image.getWidth(), image.getHeight(), rowStride);
                frameCount++;
                int mean = meanOf(bytes);
                if (frameCount % 15 == 1) {
                    if (DEBUG_DUMP_FRAME) {
                        dumpFrame(bytes, image.getWidth(), image.getHeight(), rowStride,
                                plane.getPixelStride(), snapshot);
                    }
                    android.util.Log.d("decktrace", "frame " + image.getWidth() + "x"
                            + image.getHeight() + " stride=" + rowStride + " mean=" + mean + " snap="
                            + (snapshot == null ? "null"
                            : snapshot.substring(0, Math.min(70, snapshot.length()))));
                }
                if (snapshot != null) {
                    overlay.update(snapshot);
                }
            } catch (Throwable ignored) {
                // 单帧异常不中断采集
            } finally {
                if (image != null) {
                    image.close();
                }
            }
        }
    };

    private void stopEverything() {
        running = false;
        capturing = false;
        try {
            if (displayManager != null && displayListener != null) {
                displayManager.unregisterDisplayListener(displayListener);
            }
        } catch (Throwable ignored) {
        }
        displayListener = null;
        try {
            if (virtualDisplay != null) {
                virtualDisplay.release();
                virtualDisplay = null;
            }
            if (reader != null) {
                reader.close();
                reader = null;
            }
            if (projection != null) {
                projection.stop();
                projection = null;
            }
        } catch (Throwable ignored) {
        }
        if (overlay != null) {
            overlay.hide();
            overlay = null;
        }
        if (handle != 0) {
            NativeBridge.destroy(handle);
            handle = 0;
        }
        if (thread != null) {
            thread.quitSafely();
            thread = null;
        }
        stopForeground(true);
        stopSelf();
    }

    @Override
    public void onDestroy() {
        stopEverything();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void createChannel() {
        NotificationManager nm = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        NotificationChannel channel = new NotificationChannel(
                "capture", "屏幕采集", NotificationManager.IMPORTANCE_LOW);
        nm.createNotificationChannel(channel);
    }

    private Notification buildNotification() {
        return new Notification.Builder(this, "capture")
                .setContentTitle("记牌器运行中")
                .setContentText("正在采集屏幕并识别牌面")
                .setSmallIcon(android.R.drawable.ic_menu_camera)
                .setOngoing(true)
                .build();
    }

    private static int meanOf(byte[] bytes) {
        int n = Math.min(bytes.length, 400000);
        if (n == 0) {
            return 0;
        }
        long sum = 0;
        for (int i = 0; i < n; i++) {
            sum += bytes[i] & 0xff;
        }
        return (int) (sum / n);
    }

    /** 调试落盘：覆盖写同一个 frame.raw/frame.txt（不会累积增长），仅 DEBUG_DUMP_FRAME 打开时调用。 */
    private void dumpFrame(byte[] bytes, int w, int h, int stride, int pixelStride, String meta) {
        try {
            try (FileOutputStream out = new FileOutputStream(new File(getFilesDir(), "frame.raw"))) {
                out.write(bytes);
            }
            try (FileOutputStream out = new FileOutputStream(new File(getFilesDir(), "frame.txt"))) {
                out.write((w + "x" + h + " stride=" + stride + " pixel=" + pixelStride + "\n"
                        + (meta == null ? "null" : meta)).getBytes());
            }
        } catch (IOException ignored) {
        }
    }

    /** 递归把 assets/<assetPath> 复制到 dest。 */
    private void copyAssetDir(String assetPath, File dest) {
        try {
            String[] children = getAssets().list(assetPath);
            if (children == null || children.length == 0) {
                if (dest.getParentFile() != null) {
                    dest.getParentFile().mkdirs();
                }
                try (InputStream in = getAssets().open(assetPath);
                     OutputStream out = new FileOutputStream(dest)) {
                    byte[] buf = new byte[8192];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        out.write(buf, 0, n);
                    }
                }
                return;
            }
            dest.mkdirs();
            for (String child : children) {
                copyAssetDir(assetPath + "/" + child, new File(dest, child));
            }
        } catch (IOException ignored) {
        }
    }
}

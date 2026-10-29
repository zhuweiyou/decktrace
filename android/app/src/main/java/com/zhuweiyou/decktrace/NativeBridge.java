package com.zhuweiyou.decktrace;

/** 与 Rust 端 libjni_bridge.so 对应的 JNI 桥。 */
public final class NativeBridge {
    static {
        System.loadLibrary("jni_bridge");
    }

    private NativeBridge() {}

    /** 初始化指定游戏的识别器与记牌状态机，返回句柄（0 表示失败）。 */
    public static native long create(String assetsDir, String gameId);

    /** 送入一帧 RGBA_8888，返回快照字符串（null 表示失败）。 */
    public static native String pushFrame(long handle, byte[] frame, int width, int height, int rowStride);

    /** 重置本局。 */
    public static native void reset(long handle);

    /** 释放。 */
    public static native void destroy(long handle);
}

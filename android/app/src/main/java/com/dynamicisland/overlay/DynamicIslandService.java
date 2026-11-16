package com.dynamicisland.overlay;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.res.Configuration;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.hardware.camera2.CameraManager;
import android.media.AudioManager;
import android.media.AudioRecordingConfiguration;
import android.os.BatteryManager;
import android.os.Build;
import android.os.IBinder;
import android.view.Choreographer;
import android.view.DisplayCutout;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.SurfaceView;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;

import java.util.List;

/**
 * Persistent Android ForegroundService hosting the Dynamic Island overlay window
 * with TYPE_APPLICATION_OVERLAY, FLAG_NOT_FOCUSABLE, WindowInsets cutout detection,
 * 120 Hz Choreographer frame callbacks, and system broadcast listeners.
 */
public final class DynamicIslandService extends Service implements Choreographer.FrameCallback {
    static {
        System.loadLibrary("dynamic_island");
    }

    private static final String CHANNEL_ID = "dynamic_island_persistent_overlay";
    private static final int NOTIFICATION_ID = 101;

    private WindowManager windowManager;
    private SurfaceView overlayView;
    private WindowManager.LayoutParams layoutParams;

    public static native void nativeInit(
        float screenWidth,
        float screenHeight,
        float statusBarHeight,
        float cutoutLeft,
        float cutoutTop,
        float cutoutRight,
        float cutoutBottom,
        int rotation
    );

    public static native void nativeOnWindowInsetsChanged(
        float screenWidth,
        float screenHeight,
        float statusBarHeight,
        float cutoutLeft,
        float cutoutTop,
        float cutoutRight,
        float cutoutBottom,
        int rotation
    );

    public static native float nativeOnScreenRotation(int rotation);
    public static native void nativeAttachSurface(long nativeWindowPtr, int width, int height);
    public static native void nativeDetachSurface();
    public static native int nativeOnFrameTick(long frameTimeNanos);
    public static native boolean nativeOnTouchEvent(int action, float x, float y, long timestampMs);
    public static native int nativeOnSystemEvent(int eventCode, int arg);

    private final BroadcastReceiver systemReceiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            if (intent == null || intent.getAction() == null) {
                return;
            }
            String action = intent.getAction();
            if (Intent.ACTION_POWER_CONNECTED.equals(action) || Intent.ACTION_BATTERY_CHANGED.equals(action)) {
                int status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1);
                boolean isCharging = status == BatteryManager.BATTERY_STATUS_CHARGING
                    || status == BatteryManager.BATTERY_STATUS_FULL;
                if (isCharging) {
                    int level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, 0);
                    int scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, 100);
                    int pct = scale > 0 ? (level * 100) / scale : level;
                    nativeOnSystemEvent(1, pct);
                }
            } else if (AudioManager.RINGER_MODE_CHANGED_ACTION.equals(action)) {
                int ringerMode = intent.getIntExtra(AudioManager.EXTRA_RINGER_MODE, AudioManager.RINGER_MODE_NORMAL);
                nativeOnSystemEvent(2, ringerMode);
            }
        }
    };

    private final AudioManager.AudioRecordingCallback audioRecordingCallback =
        new AudioManager.AudioRecordingCallback() {
            @Override
            public void onRecordingConfigChanged(List<AudioRecordingConfiguration> configs) {
                boolean micActive = configs != null && !configs.isEmpty();
                nativeOnSystemEvent(5, micActive ? 1 : 0);
            }
        };

    private final CameraManager.AvailabilityCallback cameraAvailabilityCallback =
        new CameraManager.AvailabilityCallback() {
            @Override
            public void onCameraUnavailable(String cameraId) {
                nativeOnSystemEvent(6, 1);
            }

            @Override
            public void onCameraAvailable(String cameraId) {
                nativeOnSystemEvent(6, 0);
            }
        };

    @Override
    public void onCreate() {
        super.onCreate();
        startPersistentForegroundNotification();
        setupOverlayWindow();
        registerSystemBroadcastListeners();
        registerPrivacySensorListeners();
        Choreographer.getInstance().postFrameCallback(this);
    }

    private void startPersistentForegroundNotification() {
        NotificationManager nm = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && nm != null) {
            NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID,
                "Dynamic Island Overlay Service",
                NotificationManager.IMPORTANCE_MIN
            );
            nm.createNotificationChannel(channel);
        }
        Notification notification = new Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.sym_def_app_icon)
            .setContentTitle("Dynamic Island")
            .setContentText("Persistent overlay active")
            .setOngoing(true)
            .build();
        startForeground(NOTIFICATION_ID, notification);
    }

    private void setupOverlayWindow() {
        windowManager = (WindowManager) getSystemService(Context.WINDOW_SERVICE);
        overlayView = new SurfaceView(this);

        int flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
            | WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
            | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
            | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
            | WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH
            | WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED
            | WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED;

        layoutParams = new WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            flags,
            PixelFormat.TRANSLUCENT
        );
        layoutParams.gravity = Gravity.TOP | Gravity.CENTER_HORIZONTAL;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            layoutParams.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS;
        }
        layoutParams.preferredRefreshRate = 120.0f;

        overlayView.setOnApplyWindowInsetsListener((View v, WindowInsets insets) -> {
            DisplayCutout cutout = insets.getDisplayCutout();
            float screenW = getResources().getDisplayMetrics().widthPixels;
            float screenH = getResources().getDisplayMetrics().heightPixels;
            float statusBarH = insets.getSystemWindowInsetTop();
            int rotation = windowManager.getDefaultDisplay().getRotation();

            if (cutout != null) {
                List<Rect> rects = cutout.getBoundingRects();
                if (!rects.isEmpty()) {
                    Rect topCutout = rects.get(0);
                    nativeOnWindowInsetsChanged(
                        screenW,
                        screenH,
                        statusBarH,
                        topCutout.left,
                        topCutout.top,
                        topCutout.right,
                        topCutout.bottom,
                        rotation
                    );
                }
            }
            return insets;
        });

        overlayView.setOnTouchListener((View v, MotionEvent event) ->
            nativeOnTouchEvent(
                event.getActionMasked(),
                event.getRawX(),
                event.getRawY(),
                event.getEventTime()
            )
        );

        float screenW = getResources().getDisplayMetrics().widthPixels;
        float screenH = getResources().getDisplayMetrics().heightPixels;
        nativeInit(screenW, screenH, 96.0f, screenW * 0.5f - 18.0f, 24.0f, screenW * 0.5f + 18.0f, 60.0f, 0);
        windowManager.addView(overlayView, layoutParams);
    }

    private void registerSystemBroadcastListeners() {
        IntentFilter filter = new IntentFilter();
        filter.addAction(Intent.ACTION_POWER_CONNECTED);
        filter.addAction(Intent.ACTION_BATTERY_CHANGED);
        filter.addAction(AudioManager.RINGER_MODE_CHANGED_ACTION);
        registerReceiver(systemReceiver, filter);
    }

    private void registerPrivacySensorListeners() {
        AudioManager audioManager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
        if (audioManager != null) {
            audioManager.registerAudioRecordingCallback(audioRecordingCallback, null);
        }
        CameraManager cameraManager = (CameraManager) getSystemService(Context.CAMERA_SERVICE);
        if (cameraManager != null) {
            cameraManager.registerAvailabilityCallback(cameraAvailabilityCallback, null);
        }
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        if (windowManager != null) {
            int rotation = windowManager.getDefaultDisplay().getRotation();
            nativeOnScreenRotation(rotation);
        }
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        nativeOnFrameTick(frameTimeNanos);
        Choreographer.getInstance().postFrameCallback(this);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        unregisterReceiver(systemReceiver);
        AudioManager audioManager = (AudioManager) getSystemService(Context.AUDIO_SERVICE);
        if (audioManager != null) {
            audioManager.unregisterAudioRecordingCallback(audioRecordingCallback);
        }
        CameraManager cameraManager = (CameraManager) getSystemService(Context.CAMERA_SERVICE);
        if (cameraManager != null) {
            cameraManager.unregisterAvailabilityCallback(cameraAvailabilityCallback);
        }
        nativeDetachSurface();
        if (windowManager != null && overlayView != null) {
            windowManager.removeView(overlayView);
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}

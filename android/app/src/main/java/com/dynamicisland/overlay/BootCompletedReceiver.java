package com.dynamicisland.overlay;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.os.Build;

/**
 * Restarts the DynamicIslandService foreground service automatically after a device reboot.
 */
public final class BootCompletedReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("dynamic_island");
    }

    public static native void nativeOnBootCompleted();

    @Override
    public void onReceive(Context context, Intent intent) {
        if (intent == null || intent.getAction() == null) {
            return;
        }
        String action = intent.getAction();
        if (Intent.ACTION_BOOT_COMPLETED.equals(action)
                || Intent.ACTION_LOCKED_BOOT_COMPLETED.equals(action)
                || Intent.ACTION_MY_PACKAGE_REPLACED.equals(action)) {
            nativeOnBootCompleted();
            Intent serviceIntent = new Intent(context, DynamicIslandService.class);
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(serviceIntent);
            } else {
                context.startService(serviceIntent);
            }
        }
    }
}

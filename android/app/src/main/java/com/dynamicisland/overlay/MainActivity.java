package com.dynamicisland.overlay;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;

/**
 * Requests SYSTEM_ALERT_WINDOW and BIND_NOTIFICATION_LISTENER_SERVICE permissions
 * on first launch and starts the persistent DynamicIslandService foreground service.
 */
public final class MainActivity extends Activity {
    private static final int REQUEST_OVERLAY_PERMISSION = 1001;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        if (!Settings.canDrawOverlays(this)) {
            Intent overlayIntent = new Intent(
                Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                Uri.parse("package:" + getPackageName())
            );
            startActivityForResult(overlayIntent, REQUEST_OVERLAY_PERMISSION);
            return;
        }

        ensureNotificationListenerEnabled();
        startDynamicIslandForegroundService();
        finish();
    }

    private void ensureNotificationListenerEnabled() {
        String enabledListeners = Settings.Secure.getString(
            getContentResolver(),
            "enabled_notification_listeners"
        );
        ComponentName listenerComponent = new ComponentName(this, DynamicIslandNotificationListener.class);
        if (enabledListeners == null || !enabledListeners.contains(listenerComponent.flattenToString())) {
            Intent listenerSettings = new Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS);
            listenerSettings.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            startActivity(listenerSettings);
        }
    }

    private void startDynamicIslandForegroundService() {
        Intent serviceIntent = new Intent(this, DynamicIslandService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent);
        } else {
            startService(serviceIntent);
        }
    }
}

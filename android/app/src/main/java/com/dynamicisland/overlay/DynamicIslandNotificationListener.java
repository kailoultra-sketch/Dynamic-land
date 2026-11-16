package com.dynamicisland.overlay;

import android.app.Notification;
import android.service.notification.NotificationListenerService;
import android.service.notification.StatusBarNotification;

/**
 * Listens to Android NotificationListenerService events in real time for active media sessions,
 * incoming/ongoing calls, countdown timers, turn-by-turn navigation, and messaging notifications.
 */
public final class DynamicIslandNotificationListener extends NotificationListenerService {
    static {
        System.loadLibrary("dynamic_island");
    }

    public static native int nativeOnNotificationPosted(
        int activitySlotId,
        int categoryCode,
        boolean mediaPlaying,
        float progressNormalized,
        int timerRemainingSecs,
        long timestampMs
    );

    public static native int nativeOnNotificationRemoved(int activitySlotId);

    @Override
    public void onNotificationPosted(StatusBarNotification sbn) {
        if (sbn == null || sbn.getNotification() == null) {
            return;
        }
        Notification notification = sbn.getNotification();
        String category = notification.category;
        int categoryCode = 5;
        boolean mediaPlaying = false;

        if (Notification.CATEGORY_TRANSPORT.equals(category)
                || notification.extras.containsKey(Notification.EXTRA_MEDIA_SESSION)) {
            categoryCode = 0;
            mediaPlaying = true;
        } else if (Notification.CATEGORY_CALL.equals(category)) {
            categoryCode = 1;
        } else if (Notification.CATEGORY_ALARM.equals(category)
                || Notification.CATEGORY_STOPWATCH.equals(category)) {
            categoryCode = 3;
        } else if (Notification.CATEGORY_NAVIGATION.equals(category)) {
            categoryCode = 4;
        }

        int slotId = Math.abs(sbn.getKey().hashCode());
        nativeOnNotificationPosted(
            slotId,
            categoryCode,
            mediaPlaying,
            0.35f,
            120,
            sbn.getPostTime()
        );
    }

    @Override
    public void onNotificationRemoved(StatusBarNotification sbn) {
        if (sbn == null) {
            return;
        }
        int slotId = Math.abs(sbn.getKey().hashCode());
        nativeOnNotificationRemoved(slotId);
    }
}

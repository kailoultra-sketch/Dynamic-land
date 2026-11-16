//! JNI C-ABI bindings connecting Android `DynamicIslandService` (`ForegroundService`),
//! `DynamicIslandNotificationListener` (`NotificationListenerService`), `WindowInsets`,
//! `BroadcastReceiver`, and `Choreographer` (120 Hz) to the Rust [`DynamicIslandController`].
#![allow(non_camel_case_types)]

use crate::activity::RingerMode;
use crate::controller::DynamicIslandController;
use crate::geometry::{ScreenRotation, WindowInsetsMetrics};
use crate::ndk_overlay::ANativeWindow;
use crate::system_events::{AndroidNotificationEvent, NotificationCategory};
use crate::touch::MotionAction;
use crate::SystemEvent;
use libc::c_void;
use std::sync::{LazyLock, Mutex};

/// Standard JNI C-ABI primitive and pointer types (`jni.h`).
pub type jboolean = u8;
pub type jint = i32;
pub type jlong = i64;
pub type jfloat = f32;
pub type jobject = *mut c_void;
pub type jclass = *mut c_void;
pub type JNIEnv = c_void;

pub const JNI_FALSE: jboolean = 0;
pub const JNI_TRUE: jboolean = 1;

/// Thread-safe global [`DynamicIslandController`] instance accessed by JNI and NDK callbacks.
pub static GLOBAL_ISLAND: LazyLock<Mutex<DynamicIslandController>> =
    LazyLock::new(|| Mutex::new(DynamicIslandController::default()));

/// NDK `AChoreographer` 120 Hz frame callback entrypoint.
#[no_mangle]
pub extern "C" fn dynamic_island_choreographer_frame_callback(
    frame_time_nanos: i64,
    _data: *mut c_void,
) {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    let _frame = controller.tick_frame_nanos(frame_time_nanos.max(0) as u64);
    if controller.overlay_window.is_attached {
        controller.overlay_window.schedule_next_120hz_frame(
            dynamic_island_choreographer_frame_callback,
            std::ptr::null_mut(),
        );
    }
}

/// Initialize the Dynamic Island service and read startup `WindowInsets` display cutout metrics.
///
/// Java signature:
/// `package com.dynamicisland.overlay;`
/// `public static native void nativeInit(float screenW, float screenH, float statusBarH, float cutoutL, float cutoutT, float cutoutR, float cutoutB, int rotation);`
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeInit(
    _env: *mut JNIEnv,
    _class: jclass,
    screen_width: jfloat,
    screen_height: jfloat,
    status_bar_height: jfloat,
    cutout_left: jfloat,
    cutout_top: jfloat,
    cutout_right: jfloat,
    cutout_bottom: jfloat,
    rotation: jint,
) {
    let insets = WindowInsetsMetrics::new(
        screen_width,
        screen_height,
        status_bar_height,
        cutout_left,
        cutout_top,
        cutout_right,
        cutout_bottom,
        ScreenRotation::from_android_rotation(rotation),
    );
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    *controller = DynamicIslandController::new(insets);
}

/// Update `WindowInsets` front camera cutout coordinates at runtime.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnWindowInsetsChanged(
    _env: *mut JNIEnv,
    _class: jclass,
    screen_width: jfloat,
    screen_height: jfloat,
    status_bar_height: jfloat,
    cutout_left: jfloat,
    cutout_top: jfloat,
    cutout_right: jfloat,
    cutout_bottom: jfloat,
    rotation: jint,
) {
    let insets = WindowInsetsMetrics::new(
        screen_width,
        screen_height,
        status_bar_height,
        cutout_left,
        cutout_top,
        cutout_right,
        cutout_bottom,
        ScreenRotation::from_android_rotation(rotation),
    );
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    controller.update_window_insets(insets);
}

/// Handle screen rotation events and recalculate top center coordinates immediately.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnScreenRotation(
    _env: *mut JNIEnv,
    _class: jclass,
    rotation: jint,
) -> jfloat {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    let cutout = controller.on_screen_rotation(ScreenRotation::from_android_rotation(rotation));
    cutout.center_x
}

/// Attach the hardware-accelerated overlay `Surface` (`ANativeWindow`) and start the 120 Hz loop.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeAttachSurface(
    _env: *mut JNIEnv,
    _class: jclass,
    native_window_ptr: jlong,
    width: jint,
    height: jint,
) {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    controller.overlay_window.attach_surface(
        native_window_ptr as *mut ANativeWindow,
        width,
        height,
    );
    controller.overlay_window.schedule_next_120hz_frame(
        dynamic_island_choreographer_frame_callback,
        std::ptr::null_mut(),
    );
}

/// Detach the overlay `Surface` (`ANativeWindow`) when the overlay view is destroyed.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeDetachSurface(
    _env: *mut JNIEnv,
    _class: jclass,
) {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    controller.overlay_window.detach_surface();
}

/// Advance the 120 Hz spring animation and rendering pipeline from Android `Choreographer.FrameCallback`.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnFrameTick(
    _env: *mut JNIEnv,
    _class: jclass,
    frame_time_nanos: jlong,
) -> jint {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    let frame = controller.tick_frame_nanos(frame_time_nanos.max(0) as u64);
    frame.state as jint
}

/// Process a touch event from the Android overlay window (`onTouchEvent`).
/// Returns `JNI_TRUE` (`1`) if the Dynamic Island consumed the touch event, or `JNI_FALSE` (`0`)
/// to pass unused touches to underlying applications.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnTouchEvent(
    _env: *mut JNIEnv,
    _class: jclass,
    action: jint,
    x: jfloat,
    y: jfloat,
    timestamp_ms: jlong,
) -> jboolean {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    let motion_action = MotionAction::from_android_action(action);
    let (dispatch, _effect) =
        controller.on_touch_event(motion_action, x, y, timestamp_ms.max(0) as u64);
    if dispatch.consumed {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Dispatch an Android system broadcast or sensor privacy event into the Dynamic Island.
///
/// Event codes:
/// - `1`: `BatteryCharging` (`arg = battery percentage 0..=100`)
/// - `2`: `RingerModeChanged` (`arg = 0 for Silent, 1 for Vibrate, 2 for Normal/Ring`)
/// - `3`: `MediaPlaybackActive`
/// - `4`: `IncomingCall`
/// - `5`: `MicrophoneInUse` (`arg = 1 active, 0 inactive`)
/// - `6`: `CameraInUse` (`arg = 1 active, 0 inactive`)
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnSystemEvent(
    _env: *mut JNIEnv,
    _class: jclass,
    event_code: jint,
    arg: jint,
) -> jint {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    let state = match event_code {
        1 => controller.on_system_event(SystemEvent::BatteryCharging {
            percentage: arg.clamp(0, 100) as u8,
        }),
        2 => match arg {
            0 => controller.on_ringer_mode_changed(RingerMode::Silent),
            1 => controller.on_ringer_mode_changed(RingerMode::Vibrate),
            _ => controller.on_ringer_mode_changed(RingerMode::Normal),
        },
        3 => controller.on_system_event(SystemEvent::MediaPlaybackActive),
        4 => controller.on_system_event(SystemEvent::IncomingCall),
        5 => {
            if arg != 0 {
                controller.on_system_event(SystemEvent::MicrophoneInUse)
            } else {
                controller.system_dispatcher.privacy.microphone_in_use = false;
                controller.state()
            }
        }
        6 => {
            if arg != 0 {
                controller.on_system_event(SystemEvent::CameraInUse)
            } else {
                controller.system_dispatcher.privacy.camera_in_use = false;
                controller.state()
            }
        }
        _ => controller.state(),
    };
    state as jint
}

/// Receive a posted notification from Android `NotificationListenerService`.
///
/// Category codes:
/// - `0`: `MediaSession`
/// - `1`: `IncomingCall`
/// - `2`: `OngoingCall`
/// - `3`: `CountdownTimer`
/// - `4`: `TurnByTurnNavigation`
/// - `5`: `Message`
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandNotificationListener_nativeOnNotificationPosted(
    _env: *mut JNIEnv,
    _class: jclass,
    activity_slot_id: jint,
    category_code: jint,
    media_playing: jboolean,
    progress_normalized: jfloat,
    timer_remaining_secs: jint,
    timestamp_ms: jlong,
) -> jint {
    let category = match category_code {
        0 => NotificationCategory::MediaSession,
        1 => NotificationCategory::IncomingCall,
        2 => NotificationCategory::OngoingCall,
        3 => NotificationCategory::CountdownTimer,
        4 => NotificationCategory::TurnByTurnNavigation,
        _ => NotificationCategory::Message,
    };

    let event = AndroidNotificationEvent {
        notification_key: format!("notif_slot_{activity_slot_id}"),
        package_name: format!("com.android.app.slot_{activity_slot_id}"),
        category,
        title: format!("Live Activity #{activity_slot_id}"),
        text: "Active Status".to_string(),
        timestamp_ms: timestamp_ms.max(0) as u64,
        media_playing: media_playing != JNI_FALSE,
        progress_normalized,
        timer_remaining_secs: timer_remaining_secs.max(0) as u32,
        navigation_arrow: "↱".to_string(),
    };

    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    controller.on_notification_posted(event) as jint
}

/// Remove a dismissed notification from Android `NotificationListenerService`.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_DynamicIslandNotificationListener_nativeOnNotificationRemoved(
    _env: *mut JNIEnv,
    _class: jclass,
    activity_slot_id: jint,
) -> jint {
    let key = format!("notif_slot_{activity_slot_id}");
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    controller.on_notification_removed(&key) as jint
}

/// Handle `BOOT_COMPLETED` broadcast to restart the foreground service automatically after a device reboot.
#[no_mangle]
pub extern "C" fn Java_com_dynamicisland_overlay_BootCompletedReceiver_nativeOnBootCompleted(
    _env: *mut JNIEnv,
    _class: jclass,
) {
    let mut controller = GLOBAL_ISLAND.lock().unwrap_or_else(|e| e.into_inner());
    controller.on_boot_completed();
}

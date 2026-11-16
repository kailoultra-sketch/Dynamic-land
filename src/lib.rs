//! # Dynamic Island Android Application Specification (ASD-STE100)
//!
//! ## Purpose
//! Build a persistent Android application in Rust that renders and controls a Dynamic Island overlay.
//! Keep the Dynamic Island active permanently above all Android screens and applications.
//!
//! ## Hardware and Geometry Rules
//! - Query Android `WindowInsets` to locate the exact coordinates of the front camera cutout.
//! - Render a pure black (`#000000`) pill shape with semicircular ends and parallel horizontal edges.
//! - Surround the front camera cutout so physical hardware blends with black screen pixels.
//! - Position the Dynamic Island at the top center of the touch screen below the top bezel.
//! - Animate all dimension changes with spring physics at 120 Hz.
//!
//! ## Visual States
//! 1. **Idle**: Minimum pill size that covers only the front camera cutout.
//! 2. **Compact**: Wide pill that shows an icon on the left and live data on the right.
//! 3. **Split**: Two separate black shapes (a primary pill on the left and a circular island on the right) for two active tasks.
//! 4. **Expanded**: Large rounded rectangle that displays full media controls, sliders, or call actions.
//!
//! ## Touch Interactions
//! - **Single tap**: Launch the linked Android application.
//! - **Touch and hold (400 ms)**: Transition from compact state to expanded state.
//! - **Swipe inward**: Collapse active Live Activities into the idle state without stopping background tasks.
//! - **Swipe outward**: Restore collapsed Live Activities to the compact or split state.
//! - **Tap outside**: Dismiss the expanded state back to the compact state.
//!
//! ## System Integrations
//! - Listen to `NotificationListenerService` for media sessions, calls, timers, and messages.
//! - Listen to Android system broadcasts for battery charging, ringer mode, Bluetooth, and privacy sensor usage.
//! - Run inside an Android `ForegroundService` with `TYPE_APPLICATION_OVERLAY` and `FLAG_NOT_FOCUSABLE`.

use serde::{Deserialize, Serialize};

pub mod activity;
pub mod controller;
pub mod geometry;
pub mod jni_bridge;
pub mod ndk_overlay;
pub mod renderer;
pub mod spring;
pub mod system_events;
pub mod touch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum IslandState {
    Idle = 0,
    Compact = 1,
    Split = 2,
    Expanded = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CutoutGeometry {
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IslandDimensions {
    pub width: f32,
    pub height: f32,
    pub corner_radius: f32,
    pub split_gap: f32,
    pub circular_diameter: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchGesture {
    SingleTapPrimary,
    SingleTapSecondary,
    TouchAndHold,
    SwipeInward,
    SwipeOutward,
    TapOutside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemEvent {
    BatteryCharging { percentage: u8 },
    RingerModeChanged { silent: bool },
    MediaPlaybackActive,
    IncomingCall,
    MicrophoneInUse,
    CameraInUse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{ActivityPayload, LiveActivity, RingerMode, TransientSystemAlert};
    use crate::controller::DynamicIslandController;
    use crate::geometry::{ScreenRotation, WindowInsetsMetrics, MIN_TOP_SCREEN_STRIP_PX};
    use crate::ndk_overlay::{
        OverlayWindowFlags, LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS, TYPE_APPLICATION_OVERLAY,
    };
    use crate::renderer::{IslandRenderer, PURE_BLACK_HEX};
    use crate::spring::REFRESH_RATE_HZ;
    use crate::system_events::{
        COLOR_PRIVACY_CAMERA_GREEN, COLOR_PRIVACY_MICROPHONE_ORANGE, COLOR_PURE_BLACK,
    };
    use crate::touch::{GestureEffect, MotionAction, TOUCH_AND_HOLD_DURATION_MS};

    fn sample_activity(id: &str, pkg: &str, started_at_ms: u64) -> LiveActivity {
        LiveActivity {
            id: id.to_string(),
            package_name: pkg.to_string(),
            app_icon_id: format!("icon_{pkg}"),
            started_at_ms,
            payload: ActivityPayload::Timer {
                label: "Countdown".to_string(),
                remaining_seconds: 125,
                is_paused: false,
            },
        }
    }

    #[test]
    fn test_window_insets_cutout_and_top_center_alignment() {
        let insets = WindowInsetsMetrics::new(
            1080.0,
            2400.0,
            96.0,
            522.0,
            24.0,
            558.0,
            60.0,
            ScreenRotation::Rotation0,
        );
        let controller = DynamicIslandController::new(insets);
        let layout = controller.current_spatial_layout();

        // Positioned at top center of the touch screen.
        assert!((controller.cutout.center_x - 540.0).abs() < 0.01);
        assert!(
            (layout.primary_pill_rect.center_x() - controller.cutout.center_x).abs() < 0.01
        );
        assert!(
            (layout.primary_pill_rect.center_y() - controller.cutout.center_y).abs() < 0.01
        );

        // Keeps a thin strip of active screen pixels between the top screen edge and the Dynamic Island.
        assert!(layout.top_bezel_strip_px >= MIN_TOP_SCREEN_STRIP_PX);

        // Surrounds the front camera cutout completely inside the black area.
        assert!(layout.primary_pill_rect.contains_rect(&layout.cutout_keepout_rect));

        // Semicircular ends in Idle pill state: corner_radius == height / 2.0.
        let dims = controller.current_dimensions();
        assert!((dims.corner_radius - dims.height * 0.5).abs() < 0.01);
    }

    #[test]
    fn test_screen_rotation_and_fixed_scroll_position() {
        let mut controller = DynamicIslandController::default();
        let initial_layout = controller.current_spatial_layout();

        // Maintain fixed top position during vertical or horizontal screen scrolling.
        let scrolled_layout = controller.on_screen_scroll(150.0, 980.0);
        assert_eq!(initial_layout, scrolled_layout);

        // Detect screen rotation events and recalculate top center coordinates immediately.
        let rotated_cutout = controller.on_screen_rotation(ScreenRotation::Rotation90);
        assert!((rotated_cutout.center_x - 1200.0).abs() < 0.01);
    }

    #[test]
    fn test_120hz_spring_physics_transitions_across_four_states() {
        assert!((REFRESH_RATE_HZ - 120.0).abs() < f32::EPSILON);

        let mut controller = DynamicIslandController::default();
        assert_eq!(controller.state(), IslandState::Idle);
        let idle_dims = controller.current_dimensions();

        // 1 -> 2: Start first Live Activity -> Compact state
        controller.start_live_activity(sample_activity("act_1", "com.clock.timer", 100));
        assert_eq!(controller.state(), IslandState::Compact);
        for _ in 0..120 {
            controller.tick_120hz();
        }
        assert!(controller.animator.is_at_rest());
        let compact_dims = controller.current_dimensions();
        assert!(compact_dims.width > idle_dims.width);
        assert!((compact_dims.corner_radius - compact_dims.height * 0.5).abs() < 0.05);

        // 2 -> 3: Start second Live Activity -> Split state (primary pill on left + circular island on right)
        controller.start_live_activity(sample_activity("act_2", "com.spotify.music", 200));
        assert_eq!(controller.state(), IslandState::Split);
        for _ in 0..120 {
            controller.tick_120hz();
        }
        assert!(controller.animator.is_at_rest());
        let split_dims = controller.current_dimensions();
        let split_layout = controller.current_spatial_layout();
        assert!(split_dims.split_gap > 0.0);
        assert!(split_dims.circular_diameter > 0.0);
        let circle = split_layout
            .circular_island_rect
            .expect("Split state must have a detached circular island on the right");
        assert!(circle.left > split_layout.primary_pill_rect.right);

        // 3 -> 4: Touch and hold (400 ms) -> Expanded state
        controller.apply_touch_gesture(TouchGesture::TouchAndHold);
        assert_eq!(controller.state(), IslandState::Expanded);
        for _ in 0..120 {
            controller.tick_120hz();
        }
        assert!(controller.animator.is_at_rest());
        let expanded_dims = controller.current_dimensions();
        assert!(expanded_dims.width > compact_dims.width);
        assert!(expanded_dims.height > compact_dims.height);
    }

    #[test]
    fn test_dual_activity_split_and_merge_behavior() {
        let mut controller = DynamicIslandController::default();
        // Start earlier activity (Timer)
        controller.start_live_activity(sample_activity("timer_1", "com.google.android.deskclock", 100));
        assert_eq!(controller.state(), IslandState::Compact);

        // Start newest activity (Navigation) -> Split mode
        controller.start_live_activity(sample_activity("nav_2", "com.google.android.apps.maps", 200));
        assert_eq!(controller.state(), IslandState::Split);

        // Newest activity is in the primary pill; earlier activity is in the circular island.
        assert_eq!(
            controller.activities.primary_activity().unwrap().package_name,
            "com.google.android.apps.maps"
        );
        assert_eq!(
            controller.activities.secondary_activity().unwrap().package_name,
            "com.google.android.deskclock"
        );

        // Single tap on the circular island opens the second (earlier) application.
        let secondary_tap = controller.apply_touch_gesture(TouchGesture::SingleTapSecondary);
        assert_eq!(
            secondary_tap,
            GestureEffect::LaunchApplication {
                package_name: "com.google.android.deskclock".to_string(),
                activity_id: "timer_1".to_string(),
                from_circular_island: true,
            }
        );

        // Single tap on the primary pill opens the first (newest) application.
        let primary_tap = controller.apply_touch_gesture(TouchGesture::SingleTapPrimary);
        assert_eq!(
            primary_tap,
            GestureEffect::LaunchApplication {
                package_name: "com.google.android.apps.maps".to_string(),
                activity_id: "nav_2".to_string(),
                from_circular_island: false,
            }
        );

        // Ending one task merges the circular island back into the primary pill (Compact state).
        controller.end_live_activity("nav_2");
        assert_eq!(controller.state(), IslandState::Compact);
        assert_eq!(
            controller.activities.primary_activity().unwrap().package_name,
            "com.google.android.deskclock"
        );
        assert!(controller.activities.secondary_activity().is_none());

        // Ending the remaining task returns to Idle.
        controller.end_live_activity("timer_1");
        assert_eq!(controller.state(), IslandState::Idle);
    }

    #[test]
    fn test_all_five_touch_gestures_and_passthrough() {
        let mut controller = DynamicIslandController::default();
        controller.start_live_activity(sample_activity("act_1", "com.spotify.music", 10));
        for _ in 0..120 {
            controller.tick_120hz();
        }
        let layout = controller.current_spatial_layout();
        let cx = layout.primary_pill_rect.center_x();
        let cy = layout.primary_pill_rect.center_y();

        // Unused touch outside the overlay window when in Compact state is passed through to underlying apps.
        let (outside_dispatch, _) =
            controller.on_touch_event(MotionAction::Down, cx, cy + 500.0, 100);
        assert!(!outside_dispatch.consumed);

        // 1. Single tap on primary pill launches source app.
        controller.on_touch_event(MotionAction::Down, cx, cy, 200);
        let (up_dispatch, effect) = controller.on_touch_event(MotionAction::Up, cx, cy, 280);
        assert!(up_dispatch.consumed);
        assert_eq!(up_dispatch.gesture, Some(TouchGesture::SingleTapPrimary));
        assert_eq!(
            effect,
            GestureEffect::LaunchApplication {
                package_name: "com.spotify.music".to_string(),
                activity_id: "act_1".to_string(),
                from_circular_island: false,
            }
        );

        // 2. Touch and hold for 400 ms opens the expanded view.
        controller.on_touch_event(MotionAction::Down, cx, cy, 1000);
        let (hold_dispatch, hold_effect) = controller.on_touch_event(
            MotionAction::Move,
            cx + 2.0,
            cy,
            1000 + TOUCH_AND_HOLD_DURATION_MS,
        );
        assert!(hold_dispatch.consumed);
        assert_eq!(hold_dispatch.gesture, Some(TouchGesture::TouchAndHold));
        assert_eq!(
            hold_effect,
            GestureEffect::TransitionState(IslandState::Expanded)
        );
        controller.on_touch_event(MotionAction::Up, cx + 2.0, cy, 1450);
        assert_eq!(controller.state(), IslandState::Expanded);

        // 3. Tap outside the expanded view closes the expanded view and returns to Compact state.
        for _ in 0..120 {
            controller.tick_120hz();
        }
        let exp_layout = controller.current_spatial_layout();
        let (tap_out_dispatch, tap_out_effect) = controller.on_touch_event(
            MotionAction::Down,
            cx,
            exp_layout.primary_pill_rect.bottom + 80.0,
            2000,
        );
        assert!(tap_out_dispatch.consumed);
        assert_eq!(tap_out_dispatch.gesture, Some(TouchGesture::TapOutside));
        assert_eq!(
            tap_out_effect,
            GestureEffect::TransitionState(IslandState::Compact)
        );
        assert_eq!(controller.state(), IslandState::Compact);

        // 4. Horizontal swipe inward from outer edge toward center hides active Live Activities into Idle
        // while keeping the background task active.
        for _ in 0..120 {
            controller.tick_120hz();
        }
        let compact_layout = controller.current_spatial_layout();
        let outer_right_x = compact_layout.primary_pill_rect.right - 8.0;
        controller.on_touch_event(MotionAction::Down, outer_right_x, cy, 3000);
        let (swipe_in_dispatch, swipe_in_effect) =
            controller.on_touch_event(MotionAction::Up, cx, cy, 3120);
        assert_eq!(swipe_in_dispatch.gesture, Some(TouchGesture::SwipeInward));
        assert_eq!(
            swipe_in_effect,
            GestureEffect::TransitionState(IslandState::Idle)
        );
        assert_eq!(controller.state(), IslandState::Idle);
        assert_eq!(controller.activities.active_count(), 1);

        // 5. Horizontal swipe outward from center toward outer edge restores hidden Live Activities.
        controller.on_touch_event(MotionAction::Down, cx, cy, 4000);
        let (swipe_out_dispatch, swipe_out_effect) =
            controller.on_touch_event(MotionAction::Up, cx + 60.0, cy, 4120);
        assert_eq!(swipe_out_dispatch.gesture, Some(TouchGesture::SwipeOutward));
        assert_eq!(
            swipe_out_effect,
            GestureEffect::TransitionState(IslandState::Compact)
        );
        assert_eq!(controller.state(), IslandState::Compact);
    }

    #[test]
    fn test_system_events_battery_ringer_media_calls_and_privacy_dots() {
        let mut controller = DynamicIslandController::default();

        // Battery charging widens the primary pill for 3 seconds (3000 ms = 360 frames at 120 Hz).
        controller.on_system_event(SystemEvent::BatteryCharging { percentage: 88 });
        assert_eq!(controller.state(), IslandState::Compact);
        assert!(controller.activities.is_widened_for_battery());
        let frame_charging = controller.tick_120hz();
        assert_eq!(
            frame_charging.left_slot_icon.as_deref(),
            Some("ic_battery_charging_green")
        );
        assert_eq!(frame_charging.right_slot_content.as_deref(), Some("88%"));

        // Advance past 3 seconds -> battery alert expires and returns to Idle.
        for _ in 0..380 {
            controller.tick_120hz();
        }
        assert!(!controller.activities.is_widened_for_battery());
        assert_eq!(controller.state(), IslandState::Idle);

        // Ringer mode changes (Silent, Vibrate, Normal).
        controller.on_ringer_mode_changed(RingerMode::Vibrate);
        assert_eq!(
            controller.activities.transient_alert(),
            Some(&TransientSystemAlert::RingerMode {
                mode: RingerMode::Vibrate,
                remaining_ms: 2_500,
            })
        );

        // Privacy indicators: Orange dot for microphone, green dot for camera, inside pure #000000 black area.
        controller.on_system_event(SystemEvent::MicrophoneInUse);
        controller.on_system_event(SystemEvent::CameraInUse);
        let frame = controller.tick_120hz();
        assert_eq!(frame.background_hex, PURE_BLACK_HEX);
        assert_eq!(frame.border_width_px, 0.0);
        assert_eq!(frame.drop_shadow_opacity, 0.0);
        assert_eq!(
            frame.microphone_dot_color,
            Some(COLOR_PRIVACY_MICROPHONE_ORANGE)
        );
        assert_eq!(frame.camera_dot_color, Some(COLOR_PRIVACY_CAMERA_GREEN));

        // Verify front camera cutout center is pure #000000 black so physical hardware blends with pixels.
        let cutout_sample = IslandRenderer::sample_pixel_rgba(
            &frame,
            controller.cutout.center_x,
            controller.cutout.center_y,
        );
        assert_eq!(cutout_sample, COLOR_PURE_BLACK);

        // Media playback & Incoming call events.
        controller.on_system_event(SystemEvent::MediaPlaybackActive);
        assert_eq!(controller.state(), IslandState::Compact);

        controller.on_system_event(SystemEvent::IncomingCall);
        assert_eq!(controller.state(), IslandState::Expanded);
        let call_frame = controller.tick_120hz();
        assert!(!call_frame.expanded_widgets.is_empty());
    }

    #[test]
    fn test_overlay_window_flags_and_permissions() {
        let controller = DynamicIslandController::default();
        assert!(controller.overlay_window.params.validate());
        assert_eq!(
            controller.overlay_window.params.window_type,
            TYPE_APPLICATION_OVERLAY
        );
        assert!(controller
            .overlay_window
            .params
            .flags
            .contains(OverlayWindowFlags::FLAG_NOT_FOCUSABLE));
        assert_eq!(
            controller
                .overlay_window
                .params
                .layout_in_display_cutout_mode,
            LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
        );
        assert!(controller.permissions.all_ready());
    }

    #[test]
    fn test_jni_bindings_end_to_end() {
        use crate::jni_bridge::*;
        let env = std::ptr::null_mut();
        let cls = std::ptr::null_mut();

        Java_com_dynamicisland_overlay_DynamicIslandService_nativeInit(
            env, cls, 1080.0, 2400.0, 96.0, 522.0, 24.0, 558.0, 60.0, 0,
        );
        Java_com_dynamicisland_overlay_DynamicIslandService_nativeAttachSurface(
            env, cls, 0, 1080, 2400,
        );

        // Post two notifications via NotificationListenerService JNI -> Split state (2)
        let state_after_first =
            Java_com_dynamicisland_overlay_DynamicIslandNotificationListener_nativeOnNotificationPosted(
                env, cls, 101, 0, JNI_TRUE, 0.5, 0, 100,
            );
        assert_eq!(state_after_first, IslandState::Compact as i32);

        let state_after_second =
            Java_com_dynamicisland_overlay_DynamicIslandNotificationListener_nativeOnNotificationPosted(
                env, cls, 202, 3, JNI_FALSE, 0.0, 90, 200,
            );
        assert_eq!(state_after_second, IslandState::Split as i32);

        // Advance 120 Hz Choreographer frame tick
        let ticked_state = Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnFrameTick(
            env,
            cls,
            8_333_333,
        );
        assert_eq!(ticked_state, IslandState::Split as i32);

        // Remove second notification -> merges circular island back into primary pill (Compact = 1)
        let state_after_remove =
            Java_com_dynamicisland_overlay_DynamicIslandNotificationListener_nativeOnNotificationRemoved(
                env, cls, 202,
            );
        assert_eq!(state_after_remove, IslandState::Compact as i32);

        // Screen rotation via JNI
        let rotated_cx =
            Java_com_dynamicisland_overlay_DynamicIslandService_nativeOnScreenRotation(
                env, cls, 90,
            );
        assert!((rotated_cx - 1200.0).abs() < 0.01);

        // BootCompletedReceiver JNI
        Java_com_dynamicisland_overlay_BootCompletedReceiver_nativeOnBootCompleted(env, cls);
        Java_com_dynamicisland_overlay_DynamicIslandService_nativeDetachSurface(env, cls);
    }
}

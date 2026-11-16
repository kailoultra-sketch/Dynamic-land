//! Central Dynamic Island application controller integrating cutout geometry,
//! 120 Hz spring physics, visual states, touch gestures, split dual-activity mode,
//! Android system events, and hardware-accelerated NDK overlay rendering.

use crate::activity::{LiveActivity, LiveActivityManager, RingerMode};
use crate::geometry::{ScreenRotation, SpatialLayout, WindowInsetsMetrics};
use crate::ndk_overlay::NdkOverlayWindow;
use crate::renderer::{GraphicsBackend, IslandRenderer, RenderFrame};
use crate::spring::{IslandSpringAnimator, DT_120HZ, FRAME_INTERVAL_NANOS};
use crate::system_events::{AndroidNotificationEvent, SystemEventDispatcher};
use crate::touch::{
    GestureEffect, MotionAction, TouchDispatchResult, TouchGestureRecognizer,
};
use crate::{CutoutGeometry, IslandDimensions, IslandState, SystemEvent, TouchGesture};
use serde::{Deserialize, Serialize};

/// Required Android permissions and service states per Section 9 of the specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionStatus {
    /// `android.permission.SYSTEM_ALERT_WINDOW` requested on first launch.
    pub system_alert_window_granted: bool,
    /// `android.permission.BIND_NOTIFICATION_LISTENER_SERVICE` to read active media and notifications.
    pub notification_listener_bound: bool,
    /// `android.permission.FOREGROUND_SERVICE` to prevent the Android system from stopping the application.
    pub foreground_service_active: bool,
    /// `android.permission.RECEIVE_BOOT_COMPLETED` to restart the foreground service automatically after reboot.
    pub auto_restart_on_boot_enabled: bool,
}

impl Default for PermissionStatus {
    fn default() -> Self {
        Self {
            system_alert_window_granted: true,
            notification_listener_bound: true,
            foreground_service_active: true,
            auto_restart_on_boot_enabled: true,
        }
    }
}

impl PermissionStatus {
    /// Returns `true` when all required permissions and foreground service guarantees are active.
    pub fn all_ready(&self) -> bool {
        self.system_alert_window_granted
            && self.notification_listener_bound
            && self.foreground_service_active
            && self.auto_restart_on_boot_enabled
    }
}

/// Top-level persistent Dynamic Island engine driving state logic, animations, and rendering.
#[derive(Debug)]
pub struct DynamicIslandController {
    pub insets: WindowInsetsMetrics,
    pub cutout: CutoutGeometry,
    pub animator: IslandSpringAnimator,
    pub activities: LiveActivityManager,
    pub touch_recognizer: TouchGestureRecognizer,
    pub system_dispatcher: SystemEventDispatcher,
    pub overlay_window: NdkOverlayWindow,
    pub renderer: IslandRenderer,
    pub permissions: PermissionStatus,
    pub last_opened_package: Option<String>,
    pub last_frame_time_nanos: Option<u64>,
    pub current_time_ms: u64,
}

impl Default for DynamicIslandController {
    fn default() -> Self {
        Self::new(WindowInsetsMetrics::default())
    }
}

impl DynamicIslandController {
    /// Initialize the Dynamic Island controller with startup `WindowInsets` metrics.
    pub fn new(insets: WindowInsetsMetrics) -> Self {
        let cutout = insets.resolve_cutout_geometry();
        let initial_dims =
            cutout.target_dimensions_for_state(IslandState::Idle, insets.screen_width, false);
        let animator = IslandSpringAnimator::new(initial_dims, IslandState::Idle);
        let layout = cutout.compute_spatial_layout(&initial_dims);
        let mut overlay_window = NdkOverlayWindow::new(GraphicsBackend::OpenGlEs3);
        overlay_window.update_touchable_bounds(layout.overlay_window_rect);

        Self {
            insets,
            cutout,
            animator,
            activities: LiveActivityManager::new(),
            touch_recognizer: TouchGestureRecognizer::new(),
            system_dispatcher: SystemEventDispatcher::new(),
            overlay_window,
            renderer: IslandRenderer::new(GraphicsBackend::OpenGlEs3),
            permissions: PermissionStatus::default(),
            last_opened_package: None,
            last_frame_time_nanos: None,
            current_time_ms: 0,
        }
    }

    /// Current [`IslandState`] (`Idle`, `Compact`, `Split`, or `Expanded`).
    pub fn state(&self) -> IslandState {
        self.activities.state()
    }

    /// Current animated [`IslandDimensions`].
    pub fn current_dimensions(&self) -> IslandDimensions {
        self.animator.current_dimensions()
    }

    /// Current [`SpatialLayout`] of the primary pill, circular island, and front camera cutout.
    pub fn current_spatial_layout(&self) -> SpatialLayout {
        let dims = self.animator.current_dimensions();
        self.cutout.compute_spatial_layout(&dims)
    }

    /// Update display cutout metrics from Android `WindowInsets` and immediately recalculate
    /// the top center coordinates.
    pub fn update_window_insets(&mut self, insets: WindowInsetsMetrics) {
        self.insets = insets;
        self.cutout = self.insets.resolve_cutout_geometry();
        self.sync_spring_targets();
        let layout = self.current_spatial_layout();
        self.overlay_window
            .update_touchable_bounds(layout.overlay_window_rect);
    }

    /// Handle an Android screen rotation event (`Surface.ROTATION_*`) and recalculate
    /// the top center coordinates immediately.
    pub fn on_screen_rotation(&mut self, rotation: ScreenRotation) -> CutoutGeometry {
        self.insets = self.insets.rotated(rotation);
        self.cutout = self.insets.resolve_cutout_geometry();
        let target = self.cutout.target_dimensions_for_state(
            self.activities.state(),
            self.insets.screen_width,
            self.activities.is_widened_for_battery(),
        );
        self.animator.set_target(target, self.activities.state());
        let layout = self.current_spatial_layout();
        self.overlay_window
            .update_touchable_bounds(layout.overlay_window_rect);
        self.cutout
    }

    /// Handle vertical or horizontal screen scrolling in underlying applications.
    /// The Dynamic Island maintains a fixed top position regardless of scroll offsets.
    pub fn on_screen_scroll(&self, _scroll_x: f32, _scroll_y: f32) -> SpatialLayout {
        self.current_spatial_layout()
    }

    /// Start or update a continuous background [`LiveActivity`].
    /// Automatically transitions to `Compact` for 1 activity or `Split` when a second activity starts.
    pub fn start_live_activity(&mut self, activity: LiveActivity) -> IslandState {
        let new_state = self.activities.upsert_activity(activity);
        self.sync_spring_targets();
        new_state
    }

    /// Stop a background [`LiveActivity`] by `id` or `package_name`.
    /// Merges the circular island back into the primary pill when one task ends in `Split` mode.
    pub fn end_live_activity(&mut self, id_or_package: &str) -> IslandState {
        self.activities.remove_activity(id_or_package);
        self.sync_spring_targets();
        self.activities.state()
    }

    /// Dispatch a high-level [`TouchGesture`] and return the resulting [`GestureEffect`].
    pub fn apply_touch_gesture(&mut self, gesture: TouchGesture) -> GestureEffect {
        let effect = match gesture {
            TouchGesture::SingleTapPrimary => {
                if let Some(primary) = self.activities.primary_activity() {
                    let pkg = primary.package_name.clone();
                    let id = primary.id.clone();
                    self.last_opened_package = Some(pkg.clone());
                    GestureEffect::LaunchApplication {
                        package_name: pkg,
                        activity_id: id,
                        from_circular_island: false,
                    }
                } else {
                    GestureEffect::None
                }
            }
            TouchGesture::SingleTapSecondary => {
                // Route a single tap on the circular island to open the second (earlier) application.
                if let Some(secondary) = self.activities.secondary_activity() {
                    let pkg = secondary.package_name.clone();
                    let id = secondary.id.clone();
                    self.last_opened_package = Some(pkg.clone());
                    GestureEffect::LaunchApplication {
                        package_name: pkg,
                        activity_id: id,
                        from_circular_island: true,
                    }
                } else {
                    GestureEffect::None
                }
            }
            TouchGesture::TouchAndHold => {
                // Route a 400 ms touch and hold on the Dynamic Island to open the expanded view.
                let next_state = self.activities.expand_primary();
                GestureEffect::TransitionState(next_state)
            }
            TouchGesture::SwipeInward => {
                // Hide active Live Activities, keep background tasks active, and return to Idle state.
                let next_state = self.activities.collapse_to_idle_keep_tasks();
                GestureEffect::TransitionState(next_state)
            }
            TouchGesture::SwipeOutward => {
                // Restore hidden Live Activities to Compact or Split state.
                let next_state = self.activities.restore_from_swipe();
                GestureEffect::TransitionState(next_state)
            }
            TouchGesture::TapOutside => {
                // Close the expanded view and return to the compact state.
                let next_state = self.activities.dismiss_expanded_to_compact();
                GestureEffect::TransitionState(next_state)
            }
        };

        self.sync_spring_targets();
        effect
    }

    /// Process a raw touch screen motion event (`Down`, `Move`, `Up`, `Cancel`, `Outside`),
    /// automatically applying any recognized [`TouchGesture`].
    pub fn on_touch_event(
        &mut self,
        action: MotionAction,
        x: f32,
        y: f32,
        timestamp_ms: u64,
    ) -> (TouchDispatchResult, GestureEffect) {
        self.current_time_ms = timestamp_ms;
        let layout = self.current_spatial_layout();
        let state = self.activities.state();

        let dispatch =
            self.touch_recognizer
                .on_touch_event(action, x, y, timestamp_ms, &layout, state);
        let effect = if let Some(gesture) = dispatch.gesture {
            self.apply_touch_gesture(gesture)
        } else {
            GestureEffect::None
        };

        (dispatch, effect)
    }

    /// Dispatch an Android [`SystemEvent`] (battery charging, ringer mode, media playback,
    /// incoming call, microphone in use, or camera in use).
    pub fn on_system_event(&mut self, event: SystemEvent) -> IslandState {
        let next_state = self.system_dispatcher.handle_system_event(
            event,
            &mut self.activities,
            self.current_time_ms,
        );
        self.sync_spring_targets();
        next_state
    }

    /// Dispatch a ringer mode change (`Silent`, `Vibrate`, `Normal`).
    pub fn on_ringer_mode_changed(&mut self, mode: RingerMode) -> IslandState {
        let next_state = self.activities.show_ringer_mode(mode);
        self.sync_spring_targets();
        next_state
    }

    /// Dispatch a notification posted by `NotificationListenerService`.
    pub fn on_notification_posted(&mut self, event: AndroidNotificationEvent) -> IslandState {
        self.current_time_ms = self.current_time_ms.max(event.timestamp_ms);
        let next_state = self
            .system_dispatcher
            .on_notification_posted(event, &mut self.activities);
        self.sync_spring_targets();
        next_state
    }

    /// Dispatch a notification removed by `NotificationListenerService`.
    pub fn on_notification_removed(&mut self, notification_key: &str) -> IslandState {
        let next_state = self
            .system_dispatcher
            .on_notification_removed(notification_key, &mut self.activities);
        self.sync_spring_targets();
        next_state
    }

    /// Handle `BOOT_COMPLETED` broadcast to restart the foreground service automatically after reboot.
    pub fn on_boot_completed(&mut self) {
        self.permissions.foreground_service_active = true;
        self.permissions.auto_restart_on_boot_enabled = true;
        self.sync_spring_targets();
    }

    /// Advance the Dynamic Island simulation by one 120 Hz frame (`dt = 1.0 / 120.0` seconds)
    /// and produce the hardware-accelerated [`RenderFrame`].
    pub fn tick_120hz(&mut self) -> RenderFrame {
        let next_nanos = self
            .last_frame_time_nanos
            .map(|t| t + FRAME_INTERVAL_NANOS)
            .unwrap_or(FRAME_INTERVAL_NANOS);
        self.tick_frame_nanos(next_nanos)
    }

    /// Advance the Dynamic Island simulation using an Android `AChoreographer` timestamp in nanoseconds.
    pub fn tick_frame_nanos(&mut self, frame_time_nanos: u64) -> RenderFrame {
        let dt_secs = if let Some(prev_nanos) = self.last_frame_time_nanos {
            let delta_ns = frame_time_nanos.saturating_sub(prev_nanos);
            if delta_ns == 0 {
                DT_120HZ
            } else {
                (delta_ns as f32 / 1_000_000_000.0).clamp(0.001, 0.05)
            }
        } else {
            DT_120HZ
        };
        self.last_frame_time_nanos = Some(frame_time_nanos);

        let elapsed_ms = ((dt_secs * 1000.0).round() as u64).max(1);
        self.current_time_ms = self.current_time_ms.saturating_add(elapsed_ms);

        // 1. Check 400 ms touch-and-hold timer if a finger is currently pressing the primary pill.
        if let Some(hold_gesture) = self.touch_recognizer.poll_hold_timer(self.current_time_ms) {
            self.apply_touch_gesture(hold_gesture);
        }

        // 2. Advance transient system alert timers (e.g. 3-second battery charging expansion).
        let prev_state = self.activities.state();
        let prev_battery = self.activities.is_widened_for_battery();
        let new_state = self.activities.tick_timers(elapsed_ms);
        if new_state != prev_state || self.activities.is_widened_for_battery() != prev_battery {
            self.sync_spring_targets();
        }

        // 3. Step 120 Hz spring physics.
        let dims = self.animator.tick(dt_secs);
        let layout = self.cutout.compute_spatial_layout(&dims);

        // 4. Update Android overlay window touchable bounds.
        self.overlay_window
            .update_touchable_bounds(layout.overlay_window_rect);

        // 5. Build GPU uniform block and scene graph.
        self.renderer.build_frame(
            self.activities.state(),
            dims,
            layout,
            self.insets.screen_width,
            self.insets.screen_height,
            &self.activities,
            &self.system_dispatcher.privacy,
        )
    }

    /// Synchronize the 120 Hz spring animator targets with the current [`IslandState`].
    fn sync_spring_targets(&mut self) {
        let state = self.activities.state();
        let target_dims = self.cutout.target_dimensions_for_state(
            state,
            self.insets.screen_width,
            self.activities.is_widened_for_battery(),
        );
        self.animator.set_target(target_dims, state);
    }
}

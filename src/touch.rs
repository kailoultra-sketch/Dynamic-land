//! Touch screen gesture recognizer and Android overlay window hit-testing.
//!
//! This module implements Sections 6 and 7 of the specification:
//! - Captures touch events inside the **Android overlay window** bounds and passes unused touches
//!   to underlying applications.
//! - **Single tap**: Opens the source application in full screen when the user taps the
//!   **primary pill** (`SingleTapPrimary`) or **circular island** (`SingleTapSecondary`) once.
//! - **Touch and hold**: Opens the **expanded view** when the user presses and holds the
//!   **Dynamic Island** for 400 milliseconds (`TOUCH_AND_HOLD_DURATION_MS = 400`).
//! - **Horizontal swipe inward**: Hides active **Live Activities** when the user swipes from the
//!   outer edge toward the center, keeping background tasks active and returning to `Idle`.
//! - **Horizontal swipe outward**: Restores hidden **Live Activities** when the user swipes from
//!   the center toward the outer edge.
//! - **Tap outside**: Closes the **expanded view** and returns to the `Compact` state when the
//!   user touches any screen area below or outside the **expanded view**.

use crate::geometry::SpatialLayout;
use crate::{IslandState, TouchGesture};
use serde::{Deserialize, Serialize};

/// Exact press-and-hold duration required to transition into the Expanded state (400 ms).
pub const TOUCH_AND_HOLD_DURATION_MS: u64 = 400;

/// Maximum pointer displacement in pixels allowed for a gesture to remain a tap or long-press.
pub const TAP_TOUCH_SLOP_PX: f32 = 18.0;

/// Minimum horizontal displacement in pixels required to recognize an inward or outward swipe.
pub const MIN_HORIZONTAL_SWIPE_PX: f32 = 28.0;

/// Raw Android MotionEvent action codes (`AMOTION_EVENT_ACTION_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionAction {
    /// Finger touched the screen (`ACTION_DOWN = 0`).
    Down,
    /// Finger lifted from the screen (`ACTION_UP = 1`).
    Up,
    /// Finger moved across the screen (`ACTION_MOVE = 2`).
    Move,
    /// Touch gesture canceled by the system (`ACTION_CANCEL = 3`).
    Cancel,
    /// Touch occurred outside the window bounds (`ACTION_OUTSIDE = 4`).
    Outside,
}

impl MotionAction {
    /// Convert an Android `MotionEvent` / NDK `AMOTION_EVENT_ACTION_MASK` integer into [`MotionAction`].
    pub fn from_android_action(action: i32) -> Self {
        match action & 0xff {
            0 => Self::Down,
            1 => Self::Up,
            2 => Self::Move,
            3 => Self::Cancel,
            4 => Self::Outside,
            _ => Self::Cancel,
        }
    }
}

/// Which physical target on the touch screen received the initial touch-down event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitTarget {
    /// The main pill-shaped black container (newest Live Activity).
    PrimaryPill,
    /// The detached circular island on the right in Split mode (earlier Live Activity).
    CircularIsland,
    /// Screen area below or outside the Dynamic Island.
    OutsideOverlay,
}

/// Result of dispatching a raw touch event to the Android overlay window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchDispatchResult {
    /// Whether the Android overlay window consumed the touch event (`true`) or passed it through
    /// to the underlying application (`false`).
    pub consumed: bool,
    /// Recognized high-level [`TouchGesture`], if a gesture completed on this event.
    pub gesture: Option<TouchGesture>,
}

/// High-level effect executed by the Dynamic Island controller when a [`TouchGesture`] fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureEffect {
    /// Open the source Android application in full screen via `PackageManager` / `PendingIntent`.
    LaunchApplication {
        package_name: String,
        activity_id: String,
        from_circular_island: bool,
    },
    /// Transition the Dynamic Island into a new [`IslandState`].
    TransitionState(IslandState),
    /// No state or app launch effect required.
    None,
}

/// Stateful gesture recognizer tracking finger contact on the touch screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TouchGestureRecognizer {
    active_target: Option<HitTarget>,
    down_x: f32,
    down_y: f32,
    last_x: f32,
    last_y: f32,
    down_timestamp_ms: u64,
    hold_triggered: bool,
}

impl Default for TouchGestureRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchGestureRecognizer {
    /// Create a new idle [`TouchGestureRecognizer`].
    pub const fn new() -> Self {
        Self {
            active_target: None,
            down_x: 0.0,
            down_y: 0.0,
            last_x: 0.0,
            last_y: 0.0,
            down_timestamp_ms: 0,
            hold_triggered: false,
        }
    }

    /// Determine which [`HitTarget`] contains the touch screen coordinate `(x, y)`.
    pub fn hit_test(layout: &SpatialLayout, x: f32, y: f32) -> HitTarget {
        if let Some(circle_rect) = &layout.circular_island_rect {
            if circle_rect.contains(x, y) {
                return HitTarget::CircularIsland;
            }
        }
        if layout.primary_pill_rect.contains(x, y) {
            return HitTarget::PrimaryPill;
        }
        HitTarget::OutsideOverlay
    }

    /// Process a raw touch screen event (`action`, `x`, `y`, `timestamp_ms`) against the current
    /// [`SpatialLayout`] and [`IslandState`].
    ///
    /// Rules enforced:
    /// - Captures touch events inside the Android overlay window bounds and passes unused touches
    ///   to underlying applications (`consumed: false`).
    /// - When in `Expanded` state, touching any screen area below or outside the expanded view
    ///   triggers `TouchGesture::TapOutside` to return to the `Compact` state.
    pub fn on_touch_event(
        &mut self,
        action: MotionAction,
        x: f32,
        y: f32,
        timestamp_ms: u64,
        layout: &SpatialLayout,
        current_state: IslandState,
    ) -> TouchDispatchResult {
        match action {
            MotionAction::Outside => {
                self.reset();
                if current_state == IslandState::Expanded {
                    return TouchDispatchResult {
                        consumed: true,
                        gesture: Some(TouchGesture::TapOutside),
                    };
                }
                TouchDispatchResult {
                    consumed: false,
                    gesture: None,
                }
            }
            MotionAction::Down => {
                let target = Self::hit_test(layout, x, y);
                if target == HitTarget::OutsideOverlay {
                    self.reset();
                    // If the expanded view is open and the user touches below/outside it,
                    // close the expanded view and return to the compact state.
                    if current_state == IslandState::Expanded {
                        return TouchDispatchResult {
                            consumed: true,
                            gesture: Some(TouchGesture::TapOutside),
                        };
                    }
                    // Otherwise pass unused touches directly to the underlying Android application.
                    return TouchDispatchResult {
                        consumed: false,
                        gesture: None,
                    };
                }

                self.active_target = Some(target);
                self.down_x = x;
                self.down_y = y;
                self.last_x = x;
                self.last_y = y;
                self.down_timestamp_ms = timestamp_ms;
                self.hold_triggered = false;

                TouchDispatchResult {
                    consumed: true,
                    gesture: None,
                }
            }
            MotionAction::Move => {
                let Some(target) = self.active_target else {
                    return TouchDispatchResult {
                        consumed: false,
                        gesture: None,
                    };
                };

                self.last_x = x;
                self.last_y = y;

                let dx = x - self.down_x;
                let dy = y - self.down_y;
                let distance = (dx * dx + dy * dy).sqrt();
                let elapsed_ms = timestamp_ms.saturating_sub(self.down_timestamp_ms);

                // Check for 400 ms touch-and-hold on the primary pill while finger stays within slop.
                if !self.hold_triggered
                    && target == HitTarget::PrimaryPill
                    && distance <= TAP_TOUCH_SLOP_PX
                    && elapsed_ms >= TOUCH_AND_HOLD_DURATION_MS
                {
                    self.hold_triggered = true;
                    return TouchDispatchResult {
                        consumed: true,
                        gesture: Some(TouchGesture::TouchAndHold),
                    };
                }

                TouchDispatchResult {
                    consumed: true,
                    gesture: None,
                }
            }
            MotionAction::Up => {
                let Some(target) = self.active_target.take() else {
                    return TouchDispatchResult {
                        consumed: false,
                        gesture: None,
                    };
                };

                let was_hold_triggered = self.hold_triggered;
                self.hold_triggered = false;

                if was_hold_triggered {
                    return TouchDispatchResult {
                        consumed: true,
                        gesture: None,
                    };
                }

                let dx = x - self.down_x;
                let dy = y - self.down_y;
                let abs_dx = dx.abs();
                let abs_dy = dy.abs();
                let distance = (dx * dx + dy * dy).sqrt();
                let elapsed_ms = timestamp_ms.saturating_sub(self.down_timestamp_ms);

                // 1. Check horizontal swipe gestures (inward vs outward relative to island center_x).
                if abs_dx >= MIN_HORIZONTAL_SWIPE_PX && abs_dx > abs_dy * 1.25 {
                    let center_x = layout.primary_pill_rect.center_x();
                    let start_dist_from_center = (self.down_x - center_x).abs();
                    let end_dist_from_center = (x - center_x).abs();

                    // Swiping from outer edge toward the center (or crossing past the center from an outer edge):
                    let pill_half_width = (layout.primary_pill_rect.width() * 0.5).max(20.0);
                    let started_near_outer_edge = start_dist_from_center >= pill_half_width * 0.35;

                    let gesture = if end_dist_from_center < start_dist_from_center
                        || (started_near_outer_edge && (self.down_x - center_x) * (x - center_x) < 0.0)
                    {
                        TouchGesture::SwipeInward
                    } else {
                        TouchGesture::SwipeOutward
                    };

                    return TouchDispatchResult {
                        consumed: true,
                        gesture: Some(gesture),
                    };
                }

                // 2. Check if user held for >= 400 ms before lifting without prior Move firing.
                if distance <= TAP_TOUCH_SLOP_PX && elapsed_ms >= TOUCH_AND_HOLD_DURATION_MS {
                    return TouchDispatchResult {
                        consumed: true,
                        gesture: Some(TouchGesture::TouchAndHold),
                    };
                }

                // 3. Single tap inside the primary pill or circular island.
                if distance <= TAP_TOUCH_SLOP_PX {
                    let gesture = match target {
                        HitTarget::PrimaryPill => TouchGesture::SingleTapPrimary,
                        HitTarget::CircularIsland => TouchGesture::SingleTapSecondary,
                        HitTarget::OutsideOverlay => TouchGesture::TapOutside,
                    };
                    return TouchDispatchResult {
                        consumed: true,
                        gesture: Some(gesture),
                    };
                }

                TouchDispatchResult {
                    consumed: true,
                    gesture: None,
                }
            }
            MotionAction::Cancel => {
                let was_active = self.active_target.is_some();
                self.reset();
                TouchDispatchResult {
                    consumed: was_active,
                    gesture: None,
                }
            }
        }
    }

    /// Check if a 400 ms touch-and-hold threshold has elapsed while the finger is held stationary.
    pub fn poll_hold_timer(&mut self, current_timestamp_ms: u64) -> Option<TouchGesture> {
        let target = self.active_target?;
        if self.hold_triggered || target != HitTarget::PrimaryPill {
            return None;
        }
        let dx = self.last_x - self.down_x;
        let dy = self.last_y - self.down_y;
        let distance = (dx * dx + dy * dy).sqrt();
        let elapsed_ms = current_timestamp_ms.saturating_sub(self.down_timestamp_ms);

        if distance <= TAP_TOUCH_SLOP_PX && elapsed_ms >= TOUCH_AND_HOLD_DURATION_MS {
            self.hold_triggered = true;
            return Some(TouchGesture::TouchAndHold);
        }
        None
    }

    /// Reset internal touch tracking state.
    pub fn reset(&mut self) {
        self.active_target = None;
        self.hold_triggered = false;
    }
}

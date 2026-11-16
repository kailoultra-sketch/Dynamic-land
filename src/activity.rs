//! Live Activities manager and dual-activity Split state machine.
//!
//! This module implements Sections 5, 7, and 8 of the specification:
//! - Supports two simultaneous **Live Activities** at the same time.
//! - Places the **primary pill** on the left to show the newest activity.
//! - Places a detached **circular island** on the right to show the earlier activity.
//! - Merges the **circular island** back into the **primary pill** when one task ends.
//! - Collapses active **Live Activities** into the `Idle` state on inward swipe without stopping
//!   background tasks, and restores them to `Compact` or `Split` on outward swipe.

use crate::IslandState;
use serde::{Deserialize, Serialize};

/// Duration in milliseconds for the Battery Charging primary pill expansion (3 seconds).
pub const BATTERY_ALERT_DURATION_MS: u64 = 3_000;

/// Duration in milliseconds for the Ringer Mode banner inside the primary pill (2.5 seconds).
pub const RINGER_ALERT_DURATION_MS: u64 = 2_500;

/// Ringer mode states reported by Android `AudioManager.RINGER_MODE_CHANGED_ACTION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RingerMode {
    Silent,
    Vibrate,
    Normal,
}

impl RingerMode {
    /// Human-readable label displayed on the right side of the primary pill.
    pub fn display_text(self) -> &'static str {
        match self {
            Self::Silent => "Silent",
            Self::Vibrate => "Vibrate",
            Self::Normal => "Ring",
        }
    }
}

/// Media playback controls available inside the Expanded view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaControlButton {
    Previous,
    PlayPause,
    Next,
}

/// Incoming call action buttons available inside the Expanded view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallActionButton {
    Accept,
    Decline,
}

/// Detailed payload for an active Live Activity displayed inside the Dynamic Island.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActivityPayload {
    /// Media playback session from `NotificationListenerService` / `MediaSessionManager`.
    MediaPlayback {
        track_title: String,
        artist: String,
        album_art_id: String,
        is_playing: bool,
        /// Playback progress slider normalized in `0.0..=1.0`.
        progress: f32,
        /// 6-bar normalized audio waveform amplitudes in `0.0..=1.0` for the right side of the pill.
        waveform_bars: [f32; 6],
    },
    /// Incoming or active phone/VoIP call.
    IncomingCall {
        caller_name: String,
        caller_photo_id: String,
        is_ringing: bool,
        call_duration_secs: u32,
    },
    /// Live countdown timer.
    Timer {
        label: String,
        remaining_seconds: u32,
        is_paused: bool,
    },
    /// Turn-by-turn navigation instruction.
    Navigation {
        direction_arrow: String,
        distance_text: String,
        street_name: String,
    },
    /// Generic real-time background task or messaging notification.
    GenericNotification {
        title: String,
        summary_text: String,
    },
}

impl ActivityPayload {
    /// Short status text rendered on the right side of the primary pill in the Compact state.
    pub fn compact_right_status(&self) -> String {
        match self {
            Self::MediaPlayback { is_playing, .. } => {
                if *is_playing {
                    "WAVEFORM".to_string()
                } else {
                    "Paused".to_string()
                }
            }
            Self::IncomingCall {
                is_ringing,
                call_duration_secs,
                ..
            } => {
                if *is_ringing {
                    "Incoming".to_string()
                } else {
                    let mins = call_duration_secs / 60;
                    let secs = call_duration_secs % 60;
                    format!("{mins:02}:{secs:02}")
                }
            }
            Self::Timer {
                remaining_seconds, ..
            } => {
                let mins = remaining_seconds / 60;
                let secs = remaining_seconds % 60;
                format!("{mins:02}:{secs:02}")
            }
            Self::Navigation {
                direction_arrow,
                distance_text,
                ..
            } => format!("{direction_arrow} {distance_text}"),
            Self::GenericNotification { summary_text, .. } => summary_text.clone(),
        }
    }
}

/// A continuous background task (Live Activity) tracked by the Dynamic Island.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveActivity {
    /// Unique identifier for this Live Activity.
    pub id: String,
    /// Android application package name that owns this activity (e.g. `"com.spotify.music"`).
    pub package_name: String,
    /// Left-side application icon identifier.
    pub app_icon_id: String,
    /// Monotonic timestamp (ms) when this activity started.
    pub started_at_ms: u64,
    /// Specific live data payload.
    pub payload: ActivityPayload,
}

/// Transient system overlay alert (battery charging or ringer mode change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransientSystemAlert {
    /// Widen the primary pill for 3 seconds to show a green charging icon and exact battery percentage.
    BatteryCharging {
        percentage: u8,
        remaining_ms: u64,
    },
    /// Show a bell icon and active ringer mode text (Silent, Vibrate, Ring).
    RingerMode {
        mode: RingerMode,
        remaining_ms: u64,
    },
}

/// Manages active Live Activities, dual-activity Split slots, transient system alerts,
/// and transitions across all four [`IslandState`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveActivityManager {
    /// Newest Live Activity rendered in the left **primary pill** (`slot 0`).
    primary_activity: Option<LiveActivity>,
    /// Earlier Live Activity rendered in the right detached **circular island** (`slot 1`).
    secondary_activity: Option<LiveActivity>,
    /// Current visual state of the Dynamic Island.
    state: IslandState,
    /// True when the user swiped inward horizontally to collapse active Live Activities into `Idle`
    /// while keeping the background tasks running.
    collapsed_by_swipe: bool,
    /// Optional transient system alert (e.g. 3-second battery charging indicator or ringer mode change).
    transient_alert: Option<TransientSystemAlert>,
}

impl Default for LiveActivityManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveActivityManager {
    /// Create a new [`LiveActivityManager`] starting in the `Idle` state.
    pub const fn new() -> Self {
        Self {
            primary_activity: None,
            secondary_activity: None,
            state: IslandState::Idle,
            collapsed_by_swipe: false,
            transient_alert: None,
        }
    }

    /// Current [`IslandState`] of the Dynamic Island.
    pub fn state(&self) -> IslandState {
        self.state
    }

    /// Returns `true` if active Live Activities are currently hidden in the `Idle` state via an inward swipe.
    pub fn is_collapsed_by_swipe(&self) -> bool {
        self.collapsed_by_swipe
    }

    /// Reference to the newest Live Activity displayed in the **primary pill**.
    pub fn primary_activity(&self) -> Option<&LiveActivity> {
        self.primary_activity.as_ref()
    }

    /// Reference to the earlier Live Activity displayed in the detached **circular island**.
    pub fn secondary_activity(&self) -> Option<&LiveActivity> {
        self.secondary_activity.as_ref()
    }

    /// Mutable reference to the newest Live Activity in the **primary pill**.
    pub fn primary_activity_mut(&mut self) -> Option<&mut LiveActivity> {
        self.primary_activity.as_mut()
    }

    /// Reference to any active transient alert (battery charging or ringer mode change).
    pub fn transient_alert(&self) -> Option<&TransientSystemAlert> {
        self.transient_alert.as_ref()
    }

    /// Returns `true` when the primary pill is currently widened for the 3-second battery charging alert.
    pub fn is_widened_for_battery(&self) -> bool {
        matches!(
            self.transient_alert,
            Some(TransientSystemAlert::BatteryCharging {
                remaining_ms,
                ..
            }) if remaining_ms > 0
        )
    }

    /// Count of active background Live Activities (`0`, `1`, or `2`).
    pub fn active_count(&self) -> usize {
        usize::from(self.primary_activity.is_some())
            + usize::from(self.secondary_activity.is_some())
    }

    /// Start or update a [`LiveActivity`].
    ///
    /// Dual-Activity Split Rules (Section 7):
    /// - If no Live Activity is running, places the activity in the **primary pill** and enters `Compact`
    ///   (or `Expanded` if it is an incoming ringing call).
    /// - If an activity with the same `id` is already present, updates it in place.
    /// - If one Live Activity is already running and a second background task starts, shifts the earlier
    ///   activity to the right detached **circular island**, places the newest activity in the left
    ///   **primary pill**, and transitions to the `Split` state.
    pub fn upsert_activity(&mut self, activity: LiveActivity) -> IslandState {
        let auto_expand = matches!(
            activity.payload,
            ActivityPayload::IncomingCall {
                is_ringing: true,
                ..
            }
        );

        self.collapsed_by_swipe = false;

        if let Some(existing_primary) = &self.primary_activity {
            if existing_primary.id == activity.id {
                self.primary_activity = Some(activity);
                if auto_expand {
                    self.state = IslandState::Expanded;
                } else if self.state == IslandState::Idle {
                    self.recalculate_default_state();
                }
                return self.state;
            }
        }

        if let Some(existing_secondary) = &self.secondary_activity {
            if existing_secondary.id == activity.id {
                self.secondary_activity = Some(activity);
                if self.state == IslandState::Idle {
                    self.recalculate_default_state();
                }
                return self.state;
            }
        }

        // A brand-new Live Activity is starting:
        // Shift any existing primary activity to the circular island on the right (earlier activity),
        // and place the newest activity in the primary pill on the left.
        if let Some(previous_primary) = self.primary_activity.take() {
            self.secondary_activity = Some(previous_primary);
            self.primary_activity = Some(activity);
            self.state = if auto_expand {
                IslandState::Expanded
            } else {
                IslandState::Split
            };
        } else {
            self.primary_activity = Some(activity);
            self.state = if auto_expand {
                IslandState::Expanded
            } else {
                IslandState::Compact
            };
        }

        self.state
    }

    /// End and remove a [`LiveActivity`] by its `id` or `package_name`.
    ///
    /// Rules enforced:
    /// - When one task ends in `Split` mode, merges the **circular island** back into the **primary pill**
    ///   (`Split -> Compact`).
    /// - When all tasks end, returns to the `Idle` state.
    pub fn remove_activity(&mut self, id_or_package: &str) -> bool {
        let removed_primary = self
            .primary_activity
            .as_ref()
            .map(|a| a.id == id_or_package || a.package_name == id_or_package)
            .unwrap_or(false);

        if removed_primary {
            // Promote the secondary activity from the circular island into the primary pill.
            self.primary_activity = self.secondary_activity.take();
            self.recalculate_default_state();
            return true;
        }

        let removed_secondary = self
            .secondary_activity
            .as_ref()
            .map(|a| a.id == id_or_package || a.package_name == id_or_package)
            .unwrap_or(false);

        if removed_secondary {
            self.secondary_activity = None;
            self.recalculate_default_state();
            return true;
        }

        false
    }

    /// Trigger a transient 3-second Battery Charging visual alert in the primary pill.
    pub fn show_battery_charging(&mut self, percentage: u8) -> IslandState {
        let clamped = percentage.min(100);
        self.transient_alert = Some(TransientSystemAlert::BatteryCharging {
            percentage: clamped,
            remaining_ms: BATTERY_ALERT_DURATION_MS,
        });
        if self.state == IslandState::Idle {
            self.state = IslandState::Compact;
        }
        self.state
    }

    /// Trigger a transient Ringer Mode visual alert in the primary pill.
    pub fn show_ringer_mode(&mut self, mode: RingerMode) -> IslandState {
        self.transient_alert = Some(TransientSystemAlert::RingerMode {
            mode,
            remaining_ms: RINGER_ALERT_DURATION_MS,
        });
        if self.state == IslandState::Idle {
            self.state = IslandState::Compact;
        }
        self.state
    }

    /// Advance transient timers by `elapsed_ms` milliseconds.
    pub fn tick_timers(&mut self, elapsed_ms: u64) -> IslandState {
        if let Some(alert) = &mut self.transient_alert {
            let remaining = match alert {
                TransientSystemAlert::BatteryCharging { remaining_ms, .. } => remaining_ms,
                TransientSystemAlert::RingerMode { remaining_ms, .. } => remaining_ms,
            };
            *remaining = remaining.saturating_sub(elapsed_ms);
            if *remaining == 0 {
                self.transient_alert = None;
                if self.state != IslandState::Expanded {
                    self.recalculate_default_state();
                }
            }
        }
        self.state
    }

    /// Transition to the `Expanded` state when the user performs a 400 ms touch-and-hold gesture.
    pub fn expand_primary(&mut self) -> IslandState {
        self.collapsed_by_swipe = false;
        self.state = IslandState::Expanded;
        self.state
    }

    /// Close the `Expanded` view and return to the `Compact` state when the user taps outside.
    pub fn dismiss_expanded_to_compact(&mut self) -> IslandState {
        if self.state == IslandState::Expanded {
            self.state = if self.active_count() == 0 && self.transient_alert.is_none() {
                IslandState::Idle
            } else {
                IslandState::Compact
            };
        }
        self.state
    }

    /// Collapse active Live Activities into the `Idle` state on a horizontal inward swipe
    /// while keeping background tasks running.
    pub fn collapse_to_idle_keep_tasks(&mut self) -> IslandState {
        self.collapsed_by_swipe = true;
        self.transient_alert = None;
        self.state = IslandState::Idle;
        self.state
    }

    /// Restore hidden Live Activities on a horizontal outward swipe to `Compact` or `Split`.
    pub fn restore_from_swipe(&mut self) -> IslandState {
        self.collapsed_by_swipe = false;
        self.recalculate_default_state();
        self.state
    }

    /// Explicitly set the visual state (used by external controllers or tests).
    pub fn set_state(&mut self, state: IslandState) {
        self.state = state;
    }

    /// Recompute the appropriate non-expanded visual state from active Live Activities and alerts.
    fn recalculate_default_state(&mut self) {
        if self.collapsed_by_swipe {
            self.state = IslandState::Idle;
            return;
        }
        match (
            self.primary_activity.is_some(),
            self.secondary_activity.is_some(),
        ) {
            (true, true) => self.state = IslandState::Split,
            (true, false) | (false, true) => self.state = IslandState::Compact,
            (false, false) => {
                if self.transient_alert.is_some() {
                    self.state = IslandState::Compact;
                } else {
                    self.state = IslandState::Idle;
                }
            }
        }
    }
}

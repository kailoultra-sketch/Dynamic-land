//! Android system broadcasts, NotificationListenerService handlers, and privacy indicators.
//!
//! This module implements Section 8 of the specification:
//! - Intercepts Android system broadcasts and `NotificationListenerService` events in real time.
//! - **Battery charging**: Widens the **primary pill** for 3 seconds to show a green charging icon
//!   and the exact battery percentage.
//! - **Ringer mode change**: Shows a bell icon and the active mode text (`Silent`, `Vibrate`, `Ring`).
//! - **Media playback**: Shows album art on the left and animated audio bars on the right when audio plays;
//!   provides play, pause, next, and previous buttons in the **expanded view**.
//! - **Incoming calls**: Expands the **primary pill** to show caller name, caller photo, accept button,
//!   and decline button.
//! - **Timers and navigation**: Shows live countdown numbers or turn-by-turn navigation arrows in the
//!   `Compact` state.
//! - **Privacy indicators**: Shows an orange dot (`#FF9500`) when an application accesses the microphone
//!   and a green dot (`#34C759`) when an application accesses the camera, placed inside the black area
//!   next to the **front camera cutout**.

use crate::activity::{ActivityPayload, LiveActivity, LiveActivityManager, RingerMode};
use crate::{IslandState, SystemEvent};
use serde::{Deserialize, Serialize};

/// Pure RGBA color `[r, g, b, a]` in normalized `0.0..=1.0` linear space.
pub type ColorRgba = [f32; 4];

/// Pure `#000000` black color for the Dynamic Island background.
pub const COLOR_PURE_BLACK: ColorRgba = [0.0, 0.0, 0.0, 1.0];

/// Orange privacy indicator dot color (`#FF9500`) when an application accesses the microphone.
pub const COLOR_PRIVACY_MICROPHONE_ORANGE: ColorRgba = [1.0, 0.584, 0.0, 1.0];

/// Green privacy indicator dot color (`#34C759`) when an application accesses the camera.
pub const COLOR_PRIVACY_CAMERA_GREEN: ColorRgba = [0.204, 0.780, 0.349, 1.0];

/// Green battery charging icon color (`#30D158`).
pub const COLOR_BATTERY_CHARGING_GREEN: ColorRgba = [0.188, 0.820, 0.345, 1.0];

/// Privacy sensor usage state tracked via Android `AppOpsManager` / `SensorPrivacyManager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PrivacyIndicatorState {
    /// True when any Android application is actively recording from the microphone (renders an orange dot).
    pub microphone_in_use: bool,
    /// True when any Android application is actively capturing from the camera (renders a green dot).
    pub camera_in_use: bool,
}

impl PrivacyIndicatorState {
    /// Returns the orange dot color if the microphone is active.
    pub fn microphone_dot_color(&self) -> Option<ColorRgba> {
        self.microphone_in_use
            .then_some(COLOR_PRIVACY_MICROPHONE_ORANGE)
    }

    /// Returns the green dot color if the camera is active.
    pub fn camera_dot_color(&self) -> Option<ColorRgba> {
        self.camera_in_use.then_some(COLOR_PRIVACY_CAMERA_GREEN)
    }
}

/// Bluetooth connection broadcast payload (`BluetoothDevice.ACTION_ACL_CONNECTED` / `HEADSET_STATE_CHANGED`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothConnectionEvent {
    pub device_name: String,
    pub battery_level: Option<u8>,
    pub connected: bool,
}

/// Structured notification posted by Android `NotificationListenerService`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AndroidNotificationEvent {
    pub notification_key: String,
    pub package_name: String,
    pub category: NotificationCategory,
    pub title: String,
    pub text: String,
    pub timestamp_ms: u64,
    pub media_playing: bool,
    pub progress_normalized: f32,
    pub timer_remaining_secs: u32,
    pub navigation_arrow: String,
}

/// Categories recognized from `Notification.CATEGORY_*` and `Notification.MediaStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationCategory {
    MediaSession,
    IncomingCall,
    OngoingCall,
    CountdownTimer,
    TurnByTurnNavigation,
    Message,
}

/// Dispatcher that routes [`SystemEvent`]s and [`AndroidNotificationEvent`]s into the
/// [`LiveActivityManager`] and [`PrivacyIndicatorState`].
#[derive(Debug, Clone, Default)]
pub struct SystemEventDispatcher {
    pub privacy: PrivacyIndicatorState,
}

impl SystemEventDispatcher {
    /// Create a new [`SystemEventDispatcher`].
    pub const fn new() -> Self {
        Self {
            privacy: PrivacyIndicatorState {
                microphone_in_use: false,
                camera_in_use: false,
            },
        }
    }

    /// Handle a core [`SystemEvent`] from Android broadcast receivers or system callbacks.
    pub fn handle_system_event(
        &mut self,
        event: SystemEvent,
        activities: &mut LiveActivityManager,
        timestamp_ms: u64,
    ) -> IslandState {
        match event {
            SystemEvent::BatteryCharging { percentage } => {
                activities.show_battery_charging(percentage)
            }
            SystemEvent::RingerModeChanged { silent } => {
                let mode = if silent {
                    RingerMode::Silent
                } else {
                    RingerMode::Normal
                };
                activities.show_ringer_mode(mode)
            }
            SystemEvent::MediaPlaybackActive => {
                let media_activity = LiveActivity {
                    id: "system.media.session".to_string(),
                    package_name: "com.android.mediaplayer".to_string(),
                    app_icon_id: "ic_album_art".to_string(),
                    started_at_ms: timestamp_ms,
                    payload: ActivityPayload::MediaPlayback {
                        track_title: "Now Playing".to_string(),
                        artist: "Active Media Session".to_string(),
                        album_art_id: "album_cover_default".to_string(),
                        is_playing: true,
                        progress: 0.35,
                        waveform_bars: [0.45, 0.85, 0.65, 0.95, 0.55, 0.75],
                    },
                };
                activities.upsert_activity(media_activity)
            }
            SystemEvent::IncomingCall => {
                let call_activity = LiveActivity {
                    id: "system.telecom.call".to_string(),
                    package_name: "com.google.android.dialer".to_string(),
                    app_icon_id: "ic_caller_photo".to_string(),
                    started_at_ms: timestamp_ms,
                    payload: ActivityPayload::IncomingCall {
                        caller_name: "Incoming Call".to_string(),
                        caller_photo_id: "caller_avatar_default".to_string(),
                        is_ringing: true,
                        call_duration_secs: 0,
                    },
                };
                activities.upsert_activity(call_activity)
            }
            SystemEvent::MicrophoneInUse => {
                self.privacy.microphone_in_use = true;
                activities.state()
            }
            SystemEvent::CameraInUse => {
                self.privacy.camera_in_use = true;
                activities.state()
            }
        }
    }

    /// Update microphone and camera privacy indicator flags explicitly.
    pub fn set_privacy_indicators(&mut self, microphone_in_use: bool, camera_in_use: bool) {
        self.privacy.microphone_in_use = microphone_in_use;
        self.privacy.camera_in_use = camera_in_use;
    }

    /// Handle a rich [`AndroidNotificationEvent`] received from `NotificationListenerService`.
    pub fn on_notification_posted(
        &mut self,
        event: AndroidNotificationEvent,
        activities: &mut LiveActivityManager,
    ) -> IslandState {
        let payload = match event.category {
            NotificationCategory::MediaSession => ActivityPayload::MediaPlayback {
                track_title: event.title,
                artist: event.text,
                album_art_id: format!("album_{}", event.package_name),
                is_playing: event.media_playing,
                progress: event.progress_normalized.clamp(0.0, 1.0),
                waveform_bars: if event.media_playing {
                    [0.5, 0.9, 0.7, 1.0, 0.6, 0.8]
                } else {
                    [0.15; 6]
                },
            },
            NotificationCategory::IncomingCall => ActivityPayload::IncomingCall {
                caller_name: event.title,
                caller_photo_id: format!("caller_{}", event.package_name),
                is_ringing: true,
                call_duration_secs: 0,
            },
            NotificationCategory::OngoingCall => ActivityPayload::IncomingCall {
                caller_name: event.title,
                caller_photo_id: format!("caller_{}", event.package_name),
                is_ringing: false,
                call_duration_secs: event.timer_remaining_secs,
            },
            NotificationCategory::CountdownTimer => ActivityPayload::Timer {
                label: event.title,
                remaining_seconds: event.timer_remaining_secs,
                is_paused: false,
            },
            NotificationCategory::TurnByTurnNavigation => ActivityPayload::Navigation {
                direction_arrow: if event.navigation_arrow.is_empty() {
                    "↱".to_string()
                } else {
                    event.navigation_arrow
                },
                distance_text: event.text,
                street_name: event.title,
            },
            NotificationCategory::Message => ActivityPayload::GenericNotification {
                title: event.title,
                summary_text: event.text,
            },
        };

        let live_activity = LiveActivity {
            id: event.notification_key,
            package_name: event.package_name.clone(),
            app_icon_id: format!("icon_{}", event.package_name),
            started_at_ms: event.timestamp_ms,
            payload,
        };

        activities.upsert_activity(live_activity)
    }

    /// Handle removal of a notification from `NotificationListenerService`.
    pub fn on_notification_removed(
        &mut self,
        notification_key: &str,
        activities: &mut LiveActivityManager,
    ) -> IslandState {
        activities.remove_activity(notification_key);
        activities.state()
    }
}

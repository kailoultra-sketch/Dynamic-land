//! Persistent Android Dynamic Island foreground daemon entrypoint.

use dynamic_island::activity::{ActivityPayload, LiveActivity};
use dynamic_island::controller::DynamicIslandController;
use dynamic_island::geometry::{ScreenRotation, WindowInsetsMetrics};
use dynamic_island::{SystemEvent, TouchGesture};

fn main() {
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
    let mut controller = DynamicIslandController::new(insets);

    // Tick initial Idle frame at 120 Hz.
    let idle_frame = controller.tick_120hz();
    println!(
        "[Dynamic Island] Initialized in {:?} state | Cutout=({:.1}, {:.1}) | Pill={:.1}x{:.1} (r={:.1}, bg={})",
        idle_frame.state,
        controller.cutout.center_x,
        controller.cutout.center_y,
        idle_frame.dimensions.width,
        idle_frame.dimensions.height,
        idle_frame.dimensions.corner_radius,
        idle_frame.background_hex
    );

    // Start first Live Activity (Media Playback) -> Compact state.
    controller.start_live_activity(LiveActivity {
        id: "media_spotify".to_string(),
        package_name: "com.spotify.music".to_string(),
        app_icon_id: "ic_spotify".to_string(),
        started_at_ms: 100,
        payload: ActivityPayload::MediaPlayback {
            track_title: "Midnight City".to_string(),
            artist: "M83".to_string(),
            album_art_id: "art_m83".to_string(),
            is_playing: true,
            progress: 0.42,
            waveform_bars: [0.4, 0.8, 0.6, 0.95, 0.5, 0.7],
        },
    });
    for _ in 0..60 {
        controller.tick_120hz();
    }

    // Start second Live Activity (Countdown Timer) -> Split state (primary pill + circular island).
    controller.start_live_activity(LiveActivity {
        id: "timer_workout".to_string(),
        package_name: "com.google.android.deskclock".to_string(),
        app_icon_id: "ic_timer".to_string(),
        started_at_ms: 600,
        payload: ActivityPayload::Timer {
            label: "Timer".to_string(),
            remaining_seconds: 185,
            is_paused: false,
        },
    });
    let split_frame = controller.tick_120hz();
    println!(
        "[Dynamic Island] Dual-activity {:?} state | Primary width={:.1}, Gap={:.1}, Circular diameter={:.1}",
        split_frame.state,
        split_frame.dimensions.width,
        split_frame.dimensions.split_gap,
        split_frame.dimensions.circular_diameter
    );

    // Touch and hold (400 ms) -> Expanded view.
    controller.apply_touch_gesture(TouchGesture::TouchAndHold);
    controller.on_system_event(SystemEvent::MicrophoneInUse);
    controller.on_system_event(SystemEvent::CameraInUse);
    for _ in 0..60 {
        controller.tick_120hz();
    }
    let expanded_frame = controller.tick_120hz();
    println!(
        "[Dynamic Island] Expanded view {:?} | Size={:.1}x{:.1} (r={:.1}) | Widgets={}",
        expanded_frame.state,
        expanded_frame.dimensions.width,
        expanded_frame.dimensions.height,
        expanded_frame.dimensions.corner_radius,
        expanded_frame.expanded_widgets.len()
    );
}

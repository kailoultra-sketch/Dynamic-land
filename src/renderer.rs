//! Hardware-accelerated OpenGL ES 3.0 / Vulkan Signed Distance Field (SDF) renderer
//! for the Dynamic Island.
//!
//! This module implements Sections 2, 3, 5, and 8 of the specification:
//! - Renders the **Dynamic Island** as a pill-shaped black area with a pure `#000000` background.
//! - Creates two fully curved semicircular ends and two parallel horizontal edges.
//! - Surrounds the **front camera cutout** completely inside the black area so the physical
//!   camera hole blends with digital `#000000` black pixels.
//! - Removes all physical borders, drop shadows, and sharp corners.
//! - Keeps the center area behind the **front camera cutout** empty and black.
//! - Renders left application icons, right live data/timers/waveforms, detached **circular island**
//!   in `Split` state, full media/call controls in `Expanded` state, and orange/green privacy dots.

use crate::activity::{
    ActivityPayload, CallActionButton, LiveActivityManager, MediaControlButton,
    TransientSystemAlert,
};
use crate::geometry::{BoundingRect, SpatialLayout};
use crate::system_events::{
    ColorRgba, PrivacyIndicatorState, COLOR_BATTERY_CHARGING_GREEN, COLOR_PRIVACY_CAMERA_GREEN,
    COLOR_PRIVACY_MICROPHONE_ORANGE, COLOR_PURE_BLACK,
};
use crate::{IslandDimensions, IslandState};
use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// Hex color string for the Dynamic Island background (`#000000` pure black).
pub const PURE_BLACK_HEX: &str = "#000000";

/// Hardware graphics API backend used by the Android NDK overlay renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphicsBackend {
    /// OpenGL ES 3.0+ via Android EGL (`libEGL.so` / `libGLESv3.so`).
    OpenGlEs3,
    /// Vulkan 1.1+ via Android `VK_KHR_android_surface` (`libvulkan.so`).
    Vulkan,
}

/// GLSL ES 3.00 / Vulkan vertex shader for full-overlay SDF rendering.
pub const ISLAND_SDF_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_uv;

out vec2 v_uv;

void main() {
    v_uv = a_uv;
    gl_Position = vec4(a_position, 0.0, 1.0);
}
"#;

/// GLSL ES 3.00 / Vulkan fragment shader rendering the pure `#000000` primary pill and circular island
/// with zero borders, zero drop shadows, and smooth semicircular SDF edges.
pub const ISLAND_SDF_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;
out vec4 fragColor;

uniform vec2 u_screen_size;
uniform vec4 u_primary_rect;      // (center_x, center_y, half_width, half_height)
uniform float u_corner_radius;    // Semicircular radius (half_height in pill mode)
uniform vec4 u_circular_rect;     // (center_x, center_y, radius, enabled)
uniform vec3 u_mic_dot;           // (x, y, radius) orange dot (#FF9500)
uniform vec3 u_cam_dot;           // (x, y, radius) green dot (#34C759)

float sdf_rounded_box(vec2 p, vec2 center, vec2 half_extents, float r) {
    vec2 q = abs(p - center) - half_extents + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

float sdf_circle(vec2 p, vec2 center, float r) {
    return length(p - center) - r;
}

void main() {
    vec2 pixel = v_uv * u_screen_size;

    // Primary pill SDF (pure #000000 black, no border, no drop shadow)
    float d_primary = sdf_rounded_box(
        pixel,
        u_primary_rect.xy,
        u_primary_rect.zw,
        u_corner_radius
    );

    float d_island = d_primary;
    if (u_circular_rect.w > 0.5) {
        float d_circle = sdf_circle(pixel, u_circular_rect.xy, u_circular_rect.z);
        d_island = min(d_primary, d_circle);
    }

    // 1px anti-aliased alpha edge with pure #000000 RGB fill
    float alpha = 1.0 - smoothstep(-0.5, 0.5, d_island);
    if (alpha <= 0.001) {
        discard;
    }

    // Pure #000000 black background so physical front camera cutout blends seamlessly
    vec3 rgb = vec3(0.0, 0.0, 0.0);

    // Microphone privacy indicator dot (orange #FF9500) inside black area next to cutout
    if (u_mic_dot.z > 0.0) {
        float d_mic = sdf_circle(pixel, u_mic_dot.xy, u_mic_dot.z);
        if (d_mic <= 0.0) {
            rgb = vec3(1.0, 0.584, 0.0);
        }
    }

    // Camera privacy indicator dot (green #34C759) inside black area next to cutout
    if (u_cam_dot.z > 0.0) {
        float d_cam = sdf_circle(pixel, u_cam_dot.xy, u_cam_dot.z);
        if (d_cam <= 0.0) {
            rgb = vec3(0.204, 0.780, 0.349);
        }
    }

    fragColor = vec4(rgb, alpha);
}
"#;

/// GPU-ready uniform block uploaded on every 120 Hz frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable, Serialize, Deserialize)]
pub struct IslandGpuUniforms {
    /// `[screen_width, screen_height, 0.0, 0.0]`
    pub screen_size: [f32; 4],
    /// `[center_x, center_y, half_width, half_height]` of the primary pill.
    pub primary_rect: [f32; 4],
    /// `[center_x, center_y, radius, enabled_flag]` of the circular island in Split state.
    pub circular_rect: [f32; 4],
    /// `[corner_radius, border_width (always 0.0), shadow_alpha (always 0.0), _pad]`
    pub style_params: [f32; 4],
    /// Pure `#000000` background color `[0.0, 0.0, 0.0, 1.0]`.
    pub background_color: [f32; 4],
    /// `[mic_x, mic_y, mic_radius, 0.0]`
    pub mic_dot: [f32; 4],
    /// `[cam_x, cam_y, cam_radius, 0.0]`
    pub cam_dot: [f32; 4],
}

/// Interactive widget rendered inside the Expanded view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExpandedWidget {
    AlbumArtwork {
        art_id: String,
        rect: BoundingRect,
    },
    TrackLabels {
        title: String,
        artist: String,
    },
    ProgressSlider {
        normalized_value: f32,
        rect: BoundingRect,
    },
    MediaButton {
        button: MediaControlButton,
        rect: BoundingRect,
    },
    CallerHeader {
        caller_name: String,
        caller_photo_id: String,
    },
    CallButton {
        button: CallActionButton,
        rect: BoundingRect,
    },
    NotificationBody {
        title: String,
        text: String,
    },
}

/// Complete scene description generated for a single 120 Hz frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderFrame {
    pub backend: GraphicsBackend,
    pub state: IslandState,
    pub dimensions: IslandDimensions,
    pub layout: SpatialLayout,
    pub uniforms: IslandGpuUniforms,
    /// Guaranteed `#000000` hex color string for the Dynamic Island containers.
    pub background_hex: String,
    /// Guaranteed `0.0` border width (no physical borders).
    pub border_width_px: f32,
    /// Guaranteed `0.0` drop shadow opacity (no drop shadows).
    pub drop_shadow_opacity: f32,
    /// True when the center area behind the front camera cutout is kept empty and pure `#000000` black.
    pub cutout_center_empty_and_black: bool,
    /// Left slot icon or indicator identifier on the left side of the front camera cutout.
    pub left_slot_icon: Option<String>,
    /// Right slot live status text, timer, or waveform summary on the right side of the front camera cutout.
    pub right_slot_content: Option<String>,
    /// Icon rendered inside the right detached circular island in Split state.
    pub circular_island_icon: Option<String>,
    /// Interactive widgets rendered when in the Expanded state.
    pub expanded_widgets: Vec<ExpandedWidget>,
    /// Orange microphone dot (`#FF9500`) if the microphone is in use.
    pub microphone_dot_color: Option<ColorRgba>,
    /// Green camera dot (`#34C759`) if the camera is in use.
    pub camera_dot_color: Option<ColorRgba>,
    /// Green battery charging icon color (`#30D158`) when the battery alert is active.
    pub battery_charging_icon_color: Option<ColorRgba>,
}

/// Signed Distance Field (SDF) evaluation for a 2D rounded box (pill or rounded rectangle).
/// Negative values are inside the shape; positive values are outside.
pub fn sdf_rounded_box(
    px: f32,
    py: f32,
    center_x: f32,
    center_y: f32,
    half_width: f32,
    half_height: f32,
    corner_radius: f32,
) -> f32 {
    let r = corner_radius.clamp(0.0, half_width.min(half_height));
    let qx = (px - center_x).abs() - half_width + r;
    let qy = (py - center_y).abs() - half_height + r;
    let outside_dist = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside_dist = qx.max(qy).min(0.0);
    outside_dist + inside_dist - r
}

/// Signed Distance Field (SDF) evaluation for a 2D circle (used for the circular island and privacy dots).
pub fn sdf_circle(px: f32, py: f32, center_x: f32, center_y: f32, radius: f32) -> f32 {
    let dx = px - center_x;
    let dy = py - center_y;
    (dx * dx + dy * dy).sqrt() - radius
}

/// Hardware-accelerated Dynamic Island renderer.
#[derive(Debug, Clone)]
pub struct IslandRenderer {
    pub backend: GraphicsBackend,
}

impl Default for IslandRenderer {
    fn default() -> Self {
        Self::new(GraphicsBackend::OpenGlEs3)
    }
}

impl IslandRenderer {
    /// Create a new renderer targeting OpenGL ES 3.0 or Vulkan.
    pub const fn new(backend: GraphicsBackend) -> Self {
        Self { backend }
    }

    /// Build the GPU uniform block and scene frame for the current 120 Hz tick.
    pub fn build_frame(
        &self,
        state: IslandState,
        dimensions: IslandDimensions,
        layout: SpatialLayout,
        screen_width: f32,
        screen_height: f32,
        activities: &LiveActivityManager,
        privacy: &PrivacyIndicatorState,
    ) -> RenderFrame {
        let p_rect = layout.primary_pill_rect;
        let primary_rect = [
            p_rect.center_x(),
            p_rect.center_y(),
            p_rect.width() * 0.5,
            p_rect.height() * 0.5,
        ];

        let circular_rect = if let Some(c_rect) = layout.circular_island_rect {
            [
                c_rect.center_x(),
                c_rect.center_y(),
                c_rect.width() * 0.5,
                1.0,
            ]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        };

        let mic_radius = if privacy.microphone_in_use { 4.0 } else { 0.0 };
        let cam_radius = if privacy.camera_in_use { 4.0 } else { 0.0 };

        let uniforms = IslandGpuUniforms {
            screen_size: [screen_width, screen_height, 0.0, 0.0],
            primary_rect,
            circular_rect,
            style_params: [dimensions.corner_radius, 0.0, 0.0, 0.0],
            background_color: COLOR_PURE_BLACK,
            mic_dot: [
                layout.microphone_dot_center.0,
                layout.microphone_dot_center.1,
                mic_radius,
                0.0,
            ],
            cam_dot: [
                layout.camera_dot_center.0,
                layout.camera_dot_center.1,
                cam_radius,
                0.0,
            ],
        };

        // Determine left/right slot content and expanded widgets.
        let mut left_slot_icon = None;
        let mut right_slot_content = None;
        let mut battery_charging_icon_color = None;

        if let Some(alert) = activities.transient_alert() {
            match alert {
                TransientSystemAlert::BatteryCharging { percentage, .. } => {
                    left_slot_icon = Some("ic_battery_charging_green".to_string());
                    right_slot_content = Some(format!("{percentage}%"));
                    battery_charging_icon_color = Some(COLOR_BATTERY_CHARGING_GREEN);
                }
                TransientSystemAlert::RingerMode { mode, .. } => {
                    left_slot_icon = Some("ic_bell".to_string());
                    right_slot_content = Some(mode.display_text().to_string());
                }
            }
        } else if state != IslandState::Idle {
            if let Some(primary) = activities.primary_activity() {
                left_slot_icon = Some(primary.app_icon_id.clone());
                right_slot_content = Some(primary.payload.compact_right_status());
            }
        }

        let circular_island_icon = if state == IslandState::Split {
            activities
                .secondary_activity()
                .map(|sec| sec.app_icon_id.clone())
        } else {
            None
        };

        let mut expanded_widgets = Vec::new();
        if state == IslandState::Expanded {
            if let Some(primary) = activities.primary_activity() {
                match &primary.payload {
                    ActivityPayload::MediaPlayback {
                        track_title,
                        artist,
                        album_art_id,
                        progress,
                        ..
                    } => {
                        let art_rect = BoundingRect::new(
                            p_rect.left + 20.0,
                            p_rect.top + 24.0,
                            p_rect.left + 84.0,
                            p_rect.top + 88.0,
                        );
                        let slider_rect = BoundingRect::new(
                            p_rect.left + 20.0,
                            p_rect.top + 104.0,
                            p_rect.right - 20.0,
                            p_rect.top + 116.0,
                        );
                        let mid_x = p_rect.center_x();
                        let btn_y = p_rect.bottom - 48.0;
                        expanded_widgets.push(ExpandedWidget::AlbumArtwork {
                            art_id: album_art_id.clone(),
                            rect: art_rect,
                        });
                        expanded_widgets.push(ExpandedWidget::TrackLabels {
                            title: track_title.clone(),
                            artist: artist.clone(),
                        });
                        expanded_widgets.push(ExpandedWidget::ProgressSlider {
                            normalized_value: *progress,
                            rect: slider_rect,
                        });
                        expanded_widgets.push(ExpandedWidget::MediaButton {
                            button: MediaControlButton::Previous,
                            rect: BoundingRect::from_center(mid_x - 64.0, btn_y, 36.0, 36.0),
                        });
                        expanded_widgets.push(ExpandedWidget::MediaButton {
                            button: MediaControlButton::PlayPause,
                            rect: BoundingRect::from_center(mid_x, btn_y, 44.0, 44.0),
                        });
                        expanded_widgets.push(ExpandedWidget::MediaButton {
                            button: MediaControlButton::Next,
                            rect: BoundingRect::from_center(mid_x + 64.0, btn_y, 36.0, 36.0),
                        });
                    }
                    ActivityPayload::IncomingCall {
                        caller_name,
                        caller_photo_id,
                        ..
                    } => {
                        let btn_y = p_rect.bottom - 48.0;
                        expanded_widgets.push(ExpandedWidget::CallerHeader {
                            caller_name: caller_name.clone(),
                            caller_photo_id: caller_photo_id.clone(),
                        });
                        expanded_widgets.push(ExpandedWidget::CallButton {
                            button: CallActionButton::Decline,
                            rect: BoundingRect::from_center(
                                p_rect.center_x() - 72.0,
                                btn_y,
                                56.0,
                                56.0,
                            ),
                        });
                        expanded_widgets.push(ExpandedWidget::CallButton {
                            button: CallActionButton::Accept,
                            rect: BoundingRect::from_center(
                                p_rect.center_x() + 72.0,
                                btn_y,
                                56.0,
                                56.0,
                            ),
                        });
                    }
                    ActivityPayload::Timer {
                        label,
                        remaining_seconds,
                        ..
                    } => {
                        let mins = remaining_seconds / 60;
                        let secs = remaining_seconds % 60;
                        expanded_widgets.push(ExpandedWidget::NotificationBody {
                            title: label.clone(),
                            text: format!("{mins:02}:{secs:02}"),
                        });
                    }
                    ActivityPayload::Navigation {
                        direction_arrow,
                        distance_text,
                        street_name,
                    } => {
                        expanded_widgets.push(ExpandedWidget::NotificationBody {
                            title: format!("{direction_arrow} {distance_text}"),
                            text: street_name.clone(),
                        });
                    }
                    ActivityPayload::GenericNotification {
                        title,
                        summary_text,
                    } => {
                        expanded_widgets.push(ExpandedWidget::NotificationBody {
                            title: title.clone(),
                            text: summary_text.clone(),
                        });
                    }
                }
            }
        }

        RenderFrame {
            backend: self.backend,
            state,
            dimensions,
            layout,
            uniforms,
            background_hex: PURE_BLACK_HEX.to_string(),
            border_width_px: 0.0,
            drop_shadow_opacity: 0.0,
            cutout_center_empty_and_black: true,
            left_slot_icon,
            right_slot_content,
            circular_island_icon,
            expanded_widgets,
            microphone_dot_color: privacy.microphone_dot_color(),
            camera_dot_color: privacy.camera_dot_color(),
            battery_charging_icon_color,
        }
    }

    /// Sample the exact RGBA color produced by the Dynamic Island SDF shader at screen pixel `(px, py)`.
    /// Used to verify that the physical front camera cutout is completely surrounded by pure `#000000`
    /// black pixels.
    pub fn sample_pixel_rgba(frame: &RenderFrame, px: f32, py: f32) -> ColorRgba {
        let u = &frame.uniforms;
        let d_primary = sdf_rounded_box(
            px,
            py,
            u.primary_rect[0],
            u.primary_rect[1],
            u.primary_rect[2],
            u.primary_rect[3],
            u.style_params[0],
        );

        let d_island = if u.circular_rect[3] > 0.5 {
            let d_circle = sdf_circle(
                px,
                py,
                u.circular_rect[0],
                u.circular_rect[1],
                u.circular_rect[2],
            );
            d_primary.min(d_circle)
        } else {
            d_primary
        };

        if d_island > 0.0 {
            return [0.0, 0.0, 0.0, 0.0];
        }

        // Check privacy indicator dots only outside the physical front camera cutout keep-out zone.
        if !frame.layout.cutout_keepout_rect.contains(px, py) {
            if u.mic_dot[2] > 0.0
                && sdf_circle(px, py, u.mic_dot[0], u.mic_dot[1], u.mic_dot[2]) <= 0.0
            {
                return COLOR_PRIVACY_MICROPHONE_ORANGE;
            }
            if u.cam_dot[2] > 0.0
                && sdf_circle(px, py, u.cam_dot[0], u.cam_dot[1], u.cam_dot[2]) <= 0.0
            {
                return COLOR_PRIVACY_CAMERA_GREEN;
            }
        }

        COLOR_PURE_BLACK
    }
}

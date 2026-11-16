//! Hardware cutout detection, WindowInsets parsing, and spatial layout for the Dynamic Island.
//!
//! This module implements Sections 3, 4, and 5 of the specification:
//! - Reads display cutout metrics from the Android `WindowInsets` API at startup.
//! - Places the **Dynamic Island** at the top center of the **touch screen**.
//! - Aligns the vertical center of the **primary pill** with the **front camera cutout**.
//! - Keeps a thin strip of active screen pixels between the top screen edge and the **Dynamic Island**.
//! - Maintains a fixed top position during vertical or horizontal screen scrolling.
//! - Detects screen rotation events and recalculates the top center coordinates immediately.

use crate::{CutoutGeometry, IslandDimensions, IslandState};
use serde::{Deserialize, Serialize};

/// Minimum strip of active screen pixels kept between the top edge of the touch screen
/// and the top edge of the Dynamic Island.
pub const MIN_TOP_SCREEN_STRIP_PX: f32 = 12.0;

/// Symmetrical padding around the physical front camera cutout in the Idle state.
pub const IDLE_CUTOUT_MARGIN_PX: f32 = 8.0;

/// Horizontal width expansion of the primary pill in the Compact state.
pub const COMPACT_MIN_WIDTH_PX: f32 = 212.0;

/// Horizontal width of the primary pill when widened for battery charging feedback.
pub const BATTERY_ALERT_WIDTH_PX: f32 = 248.0;

/// Primary pill width on the left side in the Split state.
pub const SPLIT_PRIMARY_WIDTH_PX: f32 = 168.0;

/// Clear visible gap between the primary pill and the circular island in the Split state.
pub const SPLIT_GAP_PX: f32 = 12.0;

/// Width ratio of the expanded view relative to the touch screen width.
pub const EXPANDED_WIDTH_RATIO: f32 = 0.92;

/// Maximum width of the expanded view in pixels.
pub const EXPANDED_MAX_WIDTH_PX: f32 = 384.0;

/// Height of the expanded view in pixels.
pub const EXPANDED_HEIGHT_PX: f32 = 188.0;

/// Corner radius of the large rounded rectangle in the Expanded state.
pub const EXPANDED_CORNER_RADIUS_PX: f32 = 42.0;

/// Screen rotation angles reported by the Android Display / WindowManager API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenRotation {
    /// 0 degrees (natural portrait orientation).
    Rotation0,
    /// 90 degrees (landscape orientation).
    Rotation90,
    /// 180 degrees (reverse portrait orientation).
    Rotation180,
    /// 270 degrees (reverse landscape orientation).
    Rotation270,
}

impl ScreenRotation {
    /// Convert an Android `Surface.ROTATION_*` integer constant (0..=3) or degree value into [`ScreenRotation`].
    pub fn from_android_rotation(value: i32) -> Self {
        match value {
            0 => Self::Rotation0,
            1 | 90 => Self::Rotation90,
            2 | 180 => Self::Rotation180,
            3 | 270 => Self::Rotation270,
            _ => Self::Rotation0,
        }
    }

    /// Returns `true` when the touch screen is in a landscape orientation.
    pub fn is_landscape(self) -> bool {
        matches!(self, Self::Rotation90 | Self::Rotation270)
    }
}

/// Raw bounding rectangle in touch screen pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl BoundingRect {
    /// Construct a new bounding rectangle from edges.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Construct a bounding rectangle from center coordinates and dimensions.
    pub fn from_center(center_x: f32, center_y: f32, width: f32, height: f32) -> Self {
        let half_w = (width * 0.5).max(0.0);
        let half_h = (height * 0.5).max(0.0);
        Self {
            left: center_x - half_w,
            top: center_y - half_h,
            right: center_x + half_w,
            bottom: center_y + half_h,
        }
    }

    pub fn width(&self) -> f32 {
        (self.right - self.left).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }

    pub fn center_x(&self) -> f32 {
        (self.left + self.right) * 0.5
    }

    pub fn center_y(&self) -> f32 {
        (self.top + self.bottom) * 0.5
    }

    /// Check whether a touch screen point `(x, y)` lies inside this rectangle.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.width() > 0.0
            && self.height() > 0.0
            && x >= self.left
            && x <= self.right
            && y >= self.top
            && y <= self.bottom
    }

    /// Check whether this rectangle completely encloses `inner`.
    pub fn contains_rect(&self, inner: &BoundingRect) -> bool {
        self.left <= inner.left
            && self.top <= inner.top
            && self.right >= inner.right
            && self.bottom >= inner.bottom
    }
}

/// Display metrics and cutout data read from Android `WindowInsets` and `DisplayCutout`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowInsetsMetrics {
    /// Total horizontal width of the touch screen in pixels.
    pub screen_width: f32,
    /// Total vertical height of the touch screen in pixels.
    pub screen_height: f32,
    /// Height of the Android status bar in pixels.
    pub status_bar_height: f32,
    /// Bounding box of the physical front camera cutout reported by `DisplayCutout.getBoundingRectTop()`.
    pub cutout_rect: BoundingRect,
    /// Active screen rotation state.
    pub rotation: ScreenRotation,
}

impl WindowInsetsMetrics {
    /// Create standard portrait metrics from `WindowInsets` cutout bounds.
    pub fn new(
        screen_width: f32,
        screen_height: f32,
        status_bar_height: f32,
        cutout_left: f32,
        cutout_top: f32,
        cutout_right: f32,
        cutout_bottom: f32,
        rotation: ScreenRotation,
    ) -> Self {
        Self {
            screen_width: screen_width.max(240.0),
            screen_height: screen_height.max(320.0),
            status_bar_height: status_bar_height.max(48.0),
            cutout_rect: BoundingRect::new(cutout_left, cutout_top, cutout_right, cutout_bottom),
            rotation,
        }
    }

    /// Extract the normalized [`CutoutGeometry`] aligned to the top center of the touch screen.
    ///
    /// Rules enforced:
    /// - Places the Dynamic Island at the top center of the touch screen.
    /// - Aligns the vertical center of the primary pill with the front camera cutout.
    /// - Keeps a thin strip of active screen pixels (`>= MIN_TOP_SCREEN_STRIP_PX`) between the
    ///   top screen edge and the Dynamic Island.
    pub fn resolve_cutout_geometry(&self) -> CutoutGeometry {
        let raw_w = self.cutout_rect.width().clamp(24.0, 180.0);
        let raw_h = self.cutout_rect.height().clamp(24.0, 64.0);

        // Top center of the touch screen horizontally.
        let top_center_x = self.screen_width * 0.5;

        // Ensure a thin strip of active screen pixels remains above the Dynamic Island even in Idle state.
        let idle_half_height = raw_h * 0.5 + IDLE_CUTOUT_MARGIN_PX;
        let min_center_y = MIN_TOP_SCREEN_STRIP_PX + idle_half_height;
        let raw_center_y = self.cutout_rect.center_y();
        let center_y = raw_center_y.max(min_center_y);

        CutoutGeometry {
            center_x: top_center_x,
            center_y,
            width: raw_w,
            height: raw_h,
        }
    }

    /// Recalculate display metrics and front camera cutout coordinates immediately on screen rotation.
    pub fn rotated(&self, new_rotation: ScreenRotation) -> Self {
        let swapping_axes = self.rotation.is_landscape() != new_rotation.is_landscape();
        let (new_screen_w, new_screen_h) = if swapping_axes {
            (self.screen_height, self.screen_width)
        } else {
            (self.screen_width, self.screen_height)
        };

        let cutout_w = self.cutout_rect.width().clamp(24.0, 180.0);
        let cutout_h = self.cutout_rect.height().clamp(24.0, 64.0);
        let top_offset = self
            .cutout_rect
            .top
            .max(MIN_TOP_SCREEN_STRIP_PX + IDLE_CUTOUT_MARGIN_PX);
        let new_center_x = new_screen_w * 0.5;

        let new_cutout_rect = BoundingRect::new(
            new_center_x - cutout_w * 0.5,
            top_offset,
            new_center_x + cutout_w * 0.5,
            top_offset + cutout_h,
        );

        Self {
            screen_width: new_screen_w,
            screen_height: new_screen_h,
            status_bar_height: self.status_bar_height,
            cutout_rect: new_cutout_rect,
            rotation: new_rotation,
        }
    }
}

impl Default for WindowInsetsMetrics {
    fn default() -> Self {
        // Default 1080x2400 Android display with a centered 36x36 px punch-hole front camera cutout at y=24..60.
        Self::new(
            1080.0,
            2400.0,
            96.0,
            522.0,
            24.0,
            558.0,
            60.0,
            ScreenRotation::Rotation0,
        )
    }
}

/// Complete spatial layout of the Dynamic Island for a single frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpatialLayout {
    /// Bounding box of the primary pill (or expanded view when in Expanded state).
    pub primary_pill_rect: BoundingRect,
    /// Bounding box of the detached circular island on the right in Split state (`None` when not split).
    pub circular_island_rect: Option<BoundingRect>,
    /// Bounding box of the physical front camera cutout that must remain pure `#000000` black.
    pub cutout_keepout_rect: BoundingRect,
    /// Content area to the left of the front camera cutout (application icon / album artwork).
    pub left_content_rect: BoundingRect,
    /// Content area to the right of the front camera cutout (status text, timer, or waveform).
    pub right_content_rect: BoundingRect,
    /// Overall interactive bounding box for the Android overlay window.
    pub overlay_window_rect: BoundingRect,
    /// Center coordinates of the orange microphone privacy dot `(x, y)` and radius.
    pub microphone_dot_center: (f32, f32),
    /// Center coordinates of the green camera privacy dot `(x, y)` and radius.
    pub camera_dot_center: (f32, f32),
    /// Thin strip of active screen pixels above the Dynamic Island (`top_edge_y`).
    pub top_bezel_strip_px: f32,
}

impl CutoutGeometry {
    /// Compute the target [`IslandDimensions`] for a given [`IslandState`] and screen width.
    ///
    /// Rules enforced:
    /// - **Idle**: Minimum pill dimensions covering only the front camera cutout plus a small symmetrical margin.
    ///   Two fully curved semicircular ends (`corner_radius = height / 2.0`) and parallel horizontal edges.
    /// - **Compact**: Increased horizontal width of the primary pill (`corner_radius = height / 2.0`).
    /// - **Split**: Primary pill on the left and a detached circular island on the right separated by `split_gap`.
    /// - **Expanded**: Large rounded rectangle at the top of the touch screen.
    pub fn target_dimensions_for_state(
        &self,
        state: IslandState,
        screen_width: f32,
        widen_for_battery: bool,
    ) -> IslandDimensions {
        let base_pill_height = (self.height + IDLE_CUTOUT_MARGIN_PX * 2.0).max(36.0);
        let idle_width = (self.width + IDLE_CUTOUT_MARGIN_PX * 2.0).max(base_pill_height * 2.2);

        match state {
            IslandState::Idle => IslandDimensions {
                width: idle_width,
                height: base_pill_height,
                corner_radius: base_pill_height * 0.5,
                split_gap: 0.0,
                circular_diameter: 0.0,
            },
            IslandState::Compact => {
                let target_w = if widen_for_battery {
                    BATTERY_ALERT_WIDTH_PX.max(idle_width + 96.0)
                } else {
                    COMPACT_MIN_WIDTH_PX.max(idle_width + 72.0)
                };
                IslandDimensions {
                    width: target_w,
                    height: base_pill_height,
                    corner_radius: base_pill_height * 0.5,
                    split_gap: 0.0,
                    circular_diameter: 0.0,
                }
            }
            IslandState::Split => {
                let primary_w = SPLIT_PRIMARY_WIDTH_PX.max(idle_width + 44.0);
                IslandDimensions {
                    width: primary_w,
                    height: base_pill_height,
                    corner_radius: base_pill_height * 0.5,
                    split_gap: SPLIT_GAP_PX,
                    circular_diameter: base_pill_height,
                }
            }
            IslandState::Expanded => {
                let expanded_w = (screen_width * EXPANDED_WIDTH_RATIO)
                    .min(EXPANDED_MAX_WIDTH_PX)
                    .max(idle_width + 120.0);
                let expanded_h = EXPANDED_HEIGHT_PX.max(base_pill_height * 3.2);
                IslandDimensions {
                    width: expanded_w,
                    height: expanded_h,
                    corner_radius: EXPANDED_CORNER_RADIUS_PX.min(expanded_h * 0.5),
                    split_gap: 0.0,
                    circular_diameter: 0.0,
                }
            }
        }
    }

    /// Compute the exact [`SpatialLayout`] for the current animated [`IslandDimensions`].
    ///
    /// Rules enforced:
    /// - Aligns the vertical center of the primary pill with the front camera cutout (in pill states),
    ///   and keeps the top edge anchored below the thin strip of active screen pixels in the Expanded state
    ///   so the front camera cutout remains completely surrounded by the black area.
    /// - Surrounds the front camera cutout completely inside the pure `#000000` black area.
    /// - Places privacy indicator dots inside the black area next to the front camera cutout.
    /// - Ignores vertical or horizontal scroll offsets so the Dynamic Island maintains a fixed top position.
    pub fn compute_spatial_layout(&self, dims: &IslandDimensions) -> SpatialLayout {
        let base_pill_height = (self.height + IDLE_CUTOUT_MARGIN_PX * 2.0).max(36.0);
        let pill_top = (self.center_y - base_pill_height * 0.5).max(MIN_TOP_SCREEN_STRIP_PX);

        // When height grows beyond base_pill_height (into the Expanded view), anchor the top edge at pill_top
        // so the top strip of active screen pixels remains constant and the island expands downward.
        let primary_top = if dims.height > base_pill_height + 1.0 {
            pill_top
        } else {
            (self.center_y - dims.height * 0.5).max(MIN_TOP_SCREEN_STRIP_PX)
        };
        let primary_bottom = primary_top + dims.height;

        // Ensure the primary pill horizontally centers on `self.center_x` and always covers the front camera cutout.
        let cutout_rect =
            BoundingRect::from_center(self.center_x, self.center_y, self.width, self.height);

        let primary_left = (self.center_x - dims.width * 0.5).min(cutout_rect.left - 4.0);
        let primary_right = (self.center_x + dims.width * 0.5).max(cutout_rect.right + 4.0);
        let primary_pill_rect =
            BoundingRect::new(primary_left, primary_top, primary_right, primary_bottom);

        // Detached circular island on the right when in Split state (or while split_gap/circular_diameter > 0.5).
        let circular_island_rect = if dims.circular_diameter > 0.5 {
            let circle_left = primary_pill_rect.right + dims.split_gap.max(2.0);
            let circle_cy = self.center_y;
            let r = dims.circular_diameter * 0.5;
            Some(BoundingRect::new(
                circle_left,
                circle_cy - r,
                circle_left + dims.circular_diameter,
                circle_cy + r,
            ))
        } else {
            None
        };

        // Left and right content slots around the empty black front camera cutout area.
        let inner_pad = 10.0;
        let left_slot_left = (primary_pill_rect.left + inner_pad).min(cutout_rect.left - 4.0);
        let left_slot_right = (cutout_rect.left - 6.0).max(left_slot_left);
        let left_content_rect = BoundingRect::new(
            left_slot_left,
            primary_pill_rect.top + 6.0,
            left_slot_right,
            primary_pill_rect.bottom - 6.0,
        );

        let right_slot_left = (cutout_rect.right + 6.0).min(primary_pill_rect.right - inner_pad);
        let right_slot_right = (primary_pill_rect.right - inner_pad).max(right_slot_left);
        let right_content_rect = BoundingRect::new(
            right_slot_left,
            primary_pill_rect.top + 6.0,
            right_slot_right,
            primary_pill_rect.bottom - 6.0,
        );

        // Privacy indicator dots placed inside the black area immediately next to the front camera cutout.
        let mic_dot_x = (cutout_rect.right + 5.0).min(primary_pill_rect.right - 6.0);
        let cam_dot_x = (cutout_rect.left - 5.0).max(primary_pill_rect.left + 6.0);
        let dot_y = self.center_y;

        let overlay_right = circular_island_rect
            .map(|c| c.right)
            .unwrap_or(primary_pill_rect.right);
        let overlay_window_rect = BoundingRect::new(
            primary_pill_rect.left,
            primary_pill_rect.top,
            overlay_right,
            primary_pill_rect.bottom,
        );

        SpatialLayout {
            primary_pill_rect,
            circular_island_rect,
            cutout_keepout_rect: cutout_rect,
            left_content_rect,
            right_content_rect,
            overlay_window_rect,
            microphone_dot_center: (mic_dot_x, dot_y),
            camera_dot_center: (cam_dot_x, dot_y),
            top_bezel_strip_px: primary_pill_rect.top,
        }
    }
}

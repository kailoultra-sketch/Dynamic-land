//! Android NDK overlay window (`ANativeWindow`), `AChoreographer` 120 Hz frame loop,
//! and `WindowManager.LayoutParams` configuration.
//!
//! This module implements Sections 2, 3, 4, 6, and 9 of the specification:
//! - Runs inside an Android `ForegroundService` with `TYPE_APPLICATION_OVERLAY` and `FLAG_NOT_FOCUSABLE`.
//! - Keeps the **Dynamic Island** active permanently above the lock screen, home screen, and all applications.
//! - Captures touch events inside the **Android overlay window** bounds and passes unused touches
//!   to underlying applications via `FLAG_NOT_TOUCH_MODAL` and `FLAG_WATCH_OUTSIDE_TOUCH`.
//! - Drives 120 Hz spring animations using the Android NDK `AChoreographer` frame callback API.

use crate::geometry::BoundingRect;
use crate::renderer::GraphicsBackend;
use bitflags::bitflags;
use libc::c_void;
use serde::{Deserialize, Serialize};

/// Android `WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY` constant (API 26+).
pub const TYPE_APPLICATION_OVERLAY: i32 = 2038;

/// Android `PixelFormat.TRANSLUCENT` (`-3`) / `WINDOW_FORMAT_RGBA_8888` (`1`).
pub const PIXEL_FORMAT_TRANSLUCENT: i32 = -3;
pub const AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM: i32 = 1;

/// Android `WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS` (API 30+).
/// Required so the Android overlay window extends directly into the front camera cutout region.
pub const LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS: i32 = 3;

/// Gravity flags (`Gravity.TOP | Gravity.CENTER_HORIZONTAL`) placing the overlay window at the top center.
pub const GRAVITY_TOP_CENTER_HORIZONTAL: i32 = 0x30 | 0x01;

bitflags! {
    /// Android `WindowManager.LayoutParams` flags for the persistent Dynamic Island overlay window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct OverlayWindowFlags: u32 {
        /// Prevents the overlay window from stealing input focus or keyboard focus from underlying apps.
        const FLAG_NOT_FOCUSABLE = 0x0000_0008;
        /// Passes pointer touches outside the overlay window bounds directly to underlying applications.
        const FLAG_NOT_TOUCH_MODAL = 0x0000_0020;
        /// Places the window within the entire touch screen, ignoring status bar decorations.
        const FLAG_LAYOUT_IN_SCREEN = 0x0000_0100;
        /// Allows the overlay window to extend into the status bar and cutout area without clipping.
        const FLAG_LAYOUT_NO_LIMITS = 0x0000_0200;
        /// Delivers `AMOTION_EVENT_ACTION_OUTSIDE` (4) when the user touches outside the Expanded view.
        const FLAG_WATCH_OUTSIDE_TOUCH = 0x0004_0000;
        /// Keeps the Dynamic Island visible above the Android lock screen.
        const FLAG_SHOW_WHEN_LOCKED = 0x0008_0000;
        /// Enables hardware-accelerated rendering (Vulkan / OpenGL ES) for the overlay surface.
        const FLAG_HARDWARE_ACCELERATED = 0x0100_0000;
    }
}

impl Serialize for OverlayWindowFlags {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.bits().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OverlayWindowFlags {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u32::deserialize(deserializer)?;
        Ok(Self::from_bits_retain(bits))
    }
}

impl Default for OverlayWindowFlags {
    fn default() -> Self {
        Self::FLAG_NOT_FOCUSABLE
            | Self::FLAG_NOT_TOUCH_MODAL
            | Self::FLAG_LAYOUT_IN_SCREEN
            | Self::FLAG_LAYOUT_NO_LIMITS
            | Self::FLAG_WATCH_OUTSIDE_TOUCH
            | Self::FLAG_SHOW_WHEN_LOCKED
            | Self::FLAG_HARDWARE_ACCELERATED
    }
}

/// Complete configuration for the Android overlay window managed by the ForegroundService.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OverlayWindowParams {
    /// Window type (`TYPE_APPLICATION_OVERLAY = 2038`).
    pub window_type: i32,
    /// Combined `WindowManager.LayoutParams` flags.
    pub flags: OverlayWindowFlags,
    /// Display cutout mode (`LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS = 3`).
    pub layout_in_display_cutout_mode: i32,
    /// Pixel format (`PIXEL_FORMAT_TRANSLUCENT = -3`).
    pub pixel_format: i32,
    /// Window gravity (`GRAVITY_TOP_CENTER_HORIZONTAL`).
    pub gravity: i32,
    /// Preferred display refresh rate in Hz (`120.0`).
    pub preferred_refresh_rate_hz: f32,
    /// Active touchable window bounds updated each frame so unused screen areas pass touches through.
    pub touchable_bounds: BoundingRect,
}

impl Default for OverlayWindowParams {
    fn default() -> Self {
        Self {
            window_type: TYPE_APPLICATION_OVERLAY,
            flags: OverlayWindowFlags::default(),
            layout_in_display_cutout_mode: LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS,
            pixel_format: PIXEL_FORMAT_TRANSLUCENT,
            gravity: GRAVITY_TOP_CENTER_HORIZONTAL,
            preferred_refresh_rate_hz: 120.0,
            touchable_bounds: BoundingRect::new(486.0, 16.0, 594.0, 68.0),
        }
    }
}

impl OverlayWindowParams {
    /// Verify that the overlay window parameters satisfy all Android security, focus, and cutout rules.
    pub fn validate(&self) -> bool {
        self.window_type == TYPE_APPLICATION_OVERLAY
            && self.flags.contains(OverlayWindowFlags::FLAG_NOT_FOCUSABLE)
            && self.flags.contains(OverlayWindowFlags::FLAG_NOT_TOUCH_MODAL)
            && self
                .flags
                .contains(OverlayWindowFlags::FLAG_WATCH_OUTSIDE_TOUCH)
            && self
                .flags
                .contains(OverlayWindowFlags::FLAG_SHOW_WHEN_LOCKED)
            && self.layout_in_display_cutout_mode == LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
            && (self.preferred_refresh_rate_hz - 120.0).abs() < f32::EPSILON
    }
}

/// Opaque Android NDK `ANativeWindow` pointer type.
#[repr(C)]
pub struct ANativeWindow {
    _private: [u8; 0],
}

/// Opaque Android NDK `AChoreographer` pointer type.
#[repr(C)]
pub struct AChoreographer {
    _private: [u8; 0],
}

/// Callback signature for `AChoreographer_postFrameCallback64` receiving frame time in nanoseconds.
pub type AChoreographerFrameCallback64 = extern "C" fn(frame_time_nanos: i64, data: *mut c_void);

#[cfg(target_os = "android")]
#[link(name = "android")]
extern "C" {
    pub fn ANativeWindow_acquire(window: *mut ANativeWindow);
    pub fn ANativeWindow_release(window: *mut ANativeWindow);
    pub fn ANativeWindow_getWidth(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_getHeight(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_setBuffersGeometry(
        window: *mut ANativeWindow,
        width: i32,
        height: i32,
        format: i32,
    ) -> i32;
    pub fn AChoreographer_getInstance() -> *mut AChoreographer;
    pub fn AChoreographer_postFrameCallback64(
        choreographer: *mut AChoreographer,
        callback: AChoreographerFrameCallback64,
        data: *mut c_void,
    );
}

/// Safe Rust wrapper around the Android NDK overlay surface (`ANativeWindow`) and `AChoreographer`.
#[derive(Debug)]
pub struct NdkOverlayWindow {
    native_window_ptr: usize,
    pub params: OverlayWindowParams,
    pub graphics_backend: GraphicsBackend,
    pub surface_width: i32,
    pub surface_height: i32,
    pub is_attached: bool,
}

impl Default for NdkOverlayWindow {
    fn default() -> Self {
        Self::new(GraphicsBackend::OpenGlEs3)
    }
}

impl NdkOverlayWindow {
    /// Create a new unattached NDK overlay window controller.
    pub fn new(graphics_backend: GraphicsBackend) -> Self {
        Self {
            native_window_ptr: 0,
            params: OverlayWindowParams::default(),
            graphics_backend,
            surface_width: 1080,
            surface_height: 2400,
            is_attached: false,
        }
    }

    /// Attach an Android `ANativeWindow` surface acquired from `ANativeWindow_fromSurface(env, surface)`.
    pub fn attach_surface(&mut self, window_ptr: *mut ANativeWindow, width: i32, height: i32) {
        #[cfg(target_os = "android")]
        unsafe {
            if !window_ptr.is_null() {
                ANativeWindow_acquire(window_ptr);
                ANativeWindow_setBuffersGeometry(
                    window_ptr,
                    width,
                    height,
                    AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM,
                );
            }
        }
        self.native_window_ptr = window_ptr as usize;
        self.surface_width = width.max(1);
        self.surface_height = height.max(1);
        self.is_attached = true;
    }

    /// Update the active touchable bounds of the Android overlay window so unused screen areas
    /// pass touches directly to underlying applications.
    pub fn update_touchable_bounds(&mut self, bounds: BoundingRect) {
        self.params.touchable_bounds = bounds;
    }

    /// Detach and release the `ANativeWindow` surface cleanly.
    pub fn detach_surface(&mut self) {
        #[cfg(target_os = "android")]
        unsafe {
            let ptr = self.native_window_ptr as *mut ANativeWindow;
            if !ptr.is_null() {
                ANativeWindow_release(ptr);
            }
        }
        self.native_window_ptr = 0;
        self.is_attached = false;
    }

    /// Schedule the next 120 Hz VSYNC frame callback via Android `AChoreographer`.
    pub fn schedule_next_120hz_frame(
        &self,
        _callback: AChoreographerFrameCallback64,
        _user_data: *mut c_void,
    ) {
        #[cfg(target_os = "android")]
        unsafe {
            let choreographer = AChoreographer_getInstance();
            if !choreographer.is_null() {
                AChoreographer_postFrameCallback64(choreographer, _callback, _user_data);
            }
        }
    }
}

impl Drop for NdkOverlayWindow {
    fn drop(&mut self) {
        self.detach_surface();
    }
}

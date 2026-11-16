//! 120 Hz spring physics engine for Dynamic Island shape and dimension transitions.
//!
//! This module implements Section 3 of the specification:
//! - Animates all shape and size transitions with smooth spring physics at 120 frames per second.
//! - Preserves semicircular ends (`corner_radius = height * 0.5`) in pill states (`Idle`, `Compact`, `Split`)
//!   and smoothly transitions to the rounded rectangle radius in the `Expanded` state.

use crate::{IslandDimensions, IslandState};
use serde::{Deserialize, Serialize};

/// Target display refresh rate for all Dynamic Island spring animations (120 frames per second).
pub const REFRESH_RATE_HZ: f32 = 120.0;

/// Fixed integration timestep at 120 Hz (`1.0 / 120.0` seconds = `8.333333` ms).
pub const DT_120HZ: f32 = 1.0 / REFRESH_RATE_HZ;

/// Frame interval in nanoseconds for Android `AChoreographer` 120 Hz frame callbacks.
pub const FRAME_INTERVAL_NANOS: u64 = 8_333_333;

/// Default spring stiffness (`k`) tuned for responsive, fluid Dynamic Island morphing.
pub const DEFAULT_SPRING_STIFFNESS: f32 = 340.0;

/// Default spring damping coefficient (`c`) tuned for slightly underdamped / near-critically-damped motion.
pub const DEFAULT_SPRING_DAMPING: f32 = 30.0;

/// Position and velocity threshold below which a spring snaps cleanly to its target.
pub const SPRING_REST_EPSILON: f32 = 0.05;

/// Single 1D physical spring following Hooke's law with viscous damping:
/// $$m \ddot{x} + c \dot{x} + k (x - x_{\text{target}}) = 0$$
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpringValue {
    pub current: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl SpringValue {
    /// Create a new spring initialized at rest at `initial`.
    pub const fn new(initial: f32) -> Self {
        Self {
            current: initial,
            velocity: 0.0,
            target: initial,
            stiffness: DEFAULT_SPRING_STIFFNESS,
            damping: DEFAULT_SPRING_DAMPING,
            mass: 1.0,
        }
    }

    /// Create a spring with custom stiffness and damping parameters.
    pub const fn with_params(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            current: initial,
            velocity: 0.0,
            target: initial,
            stiffness,
            damping,
            mass: 1.0,
        }
    }

    /// Set a new target value for the spring to animate toward.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Immediately snap the spring to `value` with zero velocity.
    pub fn snap_to(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Returns `true` when the spring has settled at its target within [`SPRING_REST_EPSILON`].
    pub fn is_at_rest(&self) -> bool {
        (self.current - self.target).abs() <= SPRING_REST_EPSILON
            && self.velocity.abs() <= SPRING_REST_EPSILON
    }

    /// Advance the spring by one 120 Hz frame (`dt = 1.0 / 120.0` s).
    pub fn step_120hz(&mut self) -> f32 {
        self.step(DT_120HZ)
    }

    /// Advance the spring by an arbitrary timestep `dt` in seconds using sub-stepped symplectic Euler
    /// for unconditional stability at 120 Hz.
    pub fn step(&mut self, dt: f32) -> f32 {
        if self.is_at_rest() {
            self.current = self.target;
            self.velocity = 0.0;
            return self.current;
        }

        let clamped_dt = dt.clamp(0.0001, 0.05);
        // Sub-step into 4 micro-steps for high accuracy with stiff springs.
        let substeps = 4;
        let sub_dt = clamped_dt / (substeps as f32);
        let inv_mass = 1.0 / self.mass.max(0.001);

        for _ in 0..substeps {
            let displacement = self.current - self.target;
            let spring_force = -self.stiffness * displacement;
            let damping_force = -self.damping * self.velocity;
            let acceleration = (spring_force + damping_force) * inv_mass;

            self.velocity += acceleration * sub_dt;
            self.current += self.velocity * sub_dt;
        }

        if self.is_at_rest() {
            self.current = self.target;
            self.velocity = 0.0;
        }

        self.current
    }
}

/// Multi-dimensional 120 Hz spring animator for [`IslandDimensions`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IslandSpringAnimator {
    pub width: SpringValue,
    pub height: SpringValue,
    pub corner_radius: SpringValue,
    pub split_gap: SpringValue,
    pub circular_diameter: SpringValue,
    pub active_state: IslandState,
    pub frame_count: u64,
}

impl IslandSpringAnimator {
    /// Initialize the spring animator at rest with the given [`IslandDimensions`] and [`IslandState`].
    pub fn new(initial: IslandDimensions, state: IslandState) -> Self {
        Self {
            width: SpringValue::new(initial.width),
            height: SpringValue::new(initial.height),
            corner_radius: SpringValue::new(initial.corner_radius),
            split_gap: SpringValue::new(initial.split_gap),
            circular_diameter: SpringValue::new(initial.circular_diameter),
            active_state: state,
            frame_count: 0,
        }
    }

    /// Update the target [`IslandDimensions`] and target [`IslandState`] for 120 Hz spring interpolation.
    pub fn set_target(&mut self, target: IslandDimensions, state: IslandState) {
        self.active_state = state;
        self.width.set_target(target.width);
        self.height.set_target(target.height);
        self.corner_radius.set_target(target.corner_radius);
        self.split_gap.set_target(target.split_gap);
        self.circular_diameter.set_target(target.circular_diameter);
    }

    /// Immediately snap all dimensions to `dims` without animation (used on initial boot or screen rotation).
    pub fn snap_to(&mut self, dims: IslandDimensions, state: IslandState) {
        self.active_state = state;
        self.width.snap_to(dims.width);
        self.height.snap_to(dims.height);
        self.corner_radius.snap_to(dims.corner_radius);
        self.split_gap.snap_to(dims.split_gap);
        self.circular_diameter.snap_to(dims.circular_diameter);
    }

    /// Advance all dimension springs by one 120 Hz frame (`8.333` ms) and return the current [`IslandDimensions`].
    pub fn tick_120hz(&mut self) -> IslandDimensions {
        self.tick(DT_120HZ)
    }

    /// Advance all dimension springs by `dt` seconds and return the current [`IslandDimensions`].
    pub fn tick(&mut self, dt: f32) -> IslandDimensions {
        self.frame_count = self.frame_count.wrapping_add(1);

        let w = self.width.step(dt).max(24.0);
        let h = self.height.step(dt).max(24.0);
        let raw_r = self.corner_radius.step(dt).max(12.0);
        let gap = self.split_gap.step(dt).max(0.0);
        let circ = self.circular_diameter.step(dt).max(0.0);

        // In pill states (Idle, Compact, Split) when height has settled near target, enforce exact
        // semicircular ends (corner_radius = height * 0.5). During Expanded transition, clamp radius <= height * 0.5.
        let effective_radius = match self.active_state {
            IslandState::Idle | IslandState::Compact | IslandState::Split
                if self.height.is_at_rest() =>
            {
                h * 0.5
            }
            _ => raw_r.min(h * 0.5),
        };

        IslandDimensions {
            width: w,
            height: h,
            corner_radius: effective_radius,
            split_gap: gap,
            circular_diameter: circ,
        }
    }

    /// Current instantaneous [`IslandDimensions`] without advancing the simulation clock.
    pub fn current_dimensions(&self) -> IslandDimensions {
        let h = self.height.current.max(24.0);
        let r = match self.active_state {
            IslandState::Idle | IslandState::Compact | IslandState::Split
                if self.height.is_at_rest() =>
            {
                h * 0.5
            }
            _ => self.corner_radius.current.clamp(12.0, h * 0.5),
        };
        IslandDimensions {
            width: self.width.current.max(24.0),
            height: h,
            corner_radius: r,
            split_gap: self.split_gap.current.max(0.0),
            circular_diameter: self.circular_diameter.current.max(0.0),
        }
    }

    /// Target [`IslandDimensions`] that the spring animator is converging toward.
    pub fn target_dimensions(&self) -> IslandDimensions {
        IslandDimensions {
            width: self.width.target,
            height: self.height.target,
            corner_radius: self.corner_radius.target,
            split_gap: self.split_gap.target,
            circular_diameter: self.circular_diameter.target,
        }
    }

    /// Returns `true` when all five dimension springs have settled at their targets.
    pub fn is_at_rest(&self) -> bool {
        self.width.is_at_rest()
            && self.height.is_at_rest()
            && self.corner_radius.is_at_rest()
            && self.split_gap.is_at_rest()
            && self.circular_diameter.is_at_rest()
    }
}

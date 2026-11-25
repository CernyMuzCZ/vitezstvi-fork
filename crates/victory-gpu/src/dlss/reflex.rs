//! NVIDIA Streamline Reflex Low Latency & PC Latency (PCL) marker integration.
//!
//! Streamline v2.14 splits low-latency sleep/options (`sl.reflex.dll`) and
//! latency markers (`sl.pcl.dll`, `slPCLSetMarker`). DLSS-G requires Reflex
//! active and matching frame tokens across `slSetConstants`, `PresentStart`,
//! and `PresentEnd`.

use super::ffi::*;
use super::streamline::{FeatureFns, FrameTracker, StreamlineRuntime, config};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflexModeConfig {
    Off,
    On,
    Boost,
}

impl ReflexModeConfig {
    /// Parses `VICTORY_REFLEX=off|on|boost`. Whenever DLSS-G is enabled,
    /// Reflex is forced to at least `On` because DLSS-G fails with
    /// `eFailReflexNotDetectedAtRuntime` otherwise.
    pub fn parse(raw: Option<&str>, fg_enabled: bool) -> Self {
        let parsed = match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("boost" | "on+boost" | "low_latency_boost" | "2") => Some(Self::Boost),
            Some("on" | "1" | "true" | "low_latency") => Some(Self::On),
            Some("off" | "0" | "false") => Some(Self::Off),
            _ => None,
        };
        match (parsed, fg_enabled) {
            (Some(Self::Boost), _) => Self::Boost,
            (Some(Self::On), _) => Self::On,
            (Some(Self::Off), true) => Self::On,
            (None, true) => Self::On,
            (Some(Self::Off) | None, false) => Self::Off,
        }
    }

    pub fn to_sl_mode(self) -> i32 {
        match self {
            Self::Off => REFLEX_MODE_OFF,
            Self::On => REFLEX_MODE_LOW_LATENCY,
            Self::Boost => REFLEX_MODE_LOW_LATENCY_WITH_BOOST,
        }
    }
}

/// Applies initial PCL and Reflex options right after `slSetD3DDevice` and logs
/// `ReflexState` capabilities.
pub fn initialize_after_device(rt: &StreamlineRuntime) {
    let Some(fns) = rt.feature_fns() else { return };
    let cfg = config();

    if let Some(pcl_set) = fns.pcl_set_options {
        let pcl_opts = PclOptions::default();
        // SAFETY: `pcl_opts` lives for the call.
        let r = unsafe { pcl_set(&pcl_opts) };
        if !r.is_ok() {
            tracing::warn!(result = r.name(), code = r.0, "dlss: slPCLSetOptions failed");
        }
    }

    if let Some(reflex_set) = fns.reflex_set_options {
        let opts = ReflexOptions {
            mode: cfg.reflex_mode.to_sl_mode(),
            frame_limit_us: cfg.reflex_frame_limit_us,
            use_markers_to_optimize: false,
            virtual_key: 0,
            id_thread: 0,
            ..ReflexOptions::default()
        };
        // SAFETY: `opts` lives for the call.
        let r = unsafe { reflex_set(&opts) };
        tracing::info!(
            mode = ?cfg.reflex_mode,
            frame_limit_us = cfg.reflex_frame_limit_us,
            result = r.name(),
            "dlss: slReflexSetOptions applied"
        );
    }

    if let Some(reflex_get) = fns.reflex_get_state {
        let mut state = Box::<ReflexState>::default();
        // SAFETY: boxed `ReflexState` is valid and aligned for the call.
        let r = unsafe { reflex_get(&mut *state) };
        if r.is_ok() {
            tracing::info!(
                low_latency_available = state.low_latency_available,
                latency_report_available = state.latency_report_available,
                flash_driver_controlled = state.flash_indicator_driver_controlled,
                "dlss: Reflex state"
            );
        } else {
            tracing::warn!(result = r.name(), code = r.0, "dlss: slReflexGetState failed");
        }
    }
}

/// Called once per new `FrameToken`: invokes `slReflexSleep` (in all modes per
/// the Reflex integration guide) and emits `PCLMarker::eSimulationStart`.
pub fn on_frame_start(fns: &FeatureFns, token: *const FrameToken, tracker: &mut FrameTracker) {
    let addr = token as usize;
    if let Some(sleep) = fns.reflex_sleep {
        // SAFETY: `token` is a live Streamline frame token.
        let _ = unsafe { sleep(token) };
    }
    if tracker.sim_started_token != Some(addr) {
        emit_marker(fns, PCL_MARKER_SIMULATION_START, token);
        tracker.sim_started_token = Some(addr);
    }
}

pub fn on_render_submit_start(rt: &StreamlineRuntime) {
    let Some(fns) = rt.feature_fns() else { return };
    let Some(token) = rt.ensure_frame_token() else { return };
    let addr = token as usize;
    let mut tracker = rt.frames.lock().unwrap_or_else(|e| e.into_inner());
    if tracker.sim_ended_token != Some(addr) {
        emit_marker(fns, PCL_MARKER_SIMULATION_END, token);
        tracker.sim_ended_token = Some(addr);
    }
    if tracker.render_submit_started_token != Some(addr) {
        emit_marker(fns, PCL_MARKER_RENDER_SUBMIT_START, token);
        tracker.render_submit_started_token = Some(addr);
    }
}

pub fn on_render_submit_end(rt: &StreamlineRuntime) {
    let Some(fns) = rt.feature_fns() else { return };
    let mut tracker = rt.frames.lock().unwrap_or_else(|e| e.into_inner());
    let Some(token) = tracker.token_ptr() else { return };
    let addr = token as usize;
    if tracker.render_submit_ended_token != Some(addr) {
        emit_marker(fns, PCL_MARKER_RENDER_SUBMIT_END, token);
        tracker.render_submit_ended_token = Some(addr);
    }
}

pub fn on_present_start(rt: &StreamlineRuntime, token: *const FrameToken) {
    let Some(fns) = rt.feature_fns() else { return };
    let addr = token as usize;
    {
        let mut tracker = rt.frames.lock().unwrap_or_else(|e| e.into_inner());
        if tracker.sim_ended_token != Some(addr) {
            emit_marker(fns, PCL_MARKER_SIMULATION_END, token);
            tracker.sim_ended_token = Some(addr);
        }
    }
    emit_marker(fns, PCL_MARKER_PRESENT_START, token);
}

pub fn on_present_end(rt: &StreamlineRuntime, token: *const FrameToken) {
    let Some(fns) = rt.feature_fns() else { return };
    emit_marker(fns, PCL_MARKER_PRESENT_END, token);
}

fn emit_marker(fns: &FeatureFns, marker: u32, token: *const FrameToken) {
    if let Some(set_marker) = fns.pcl_set_marker {
        // SAFETY: `token` is a live Streamline frame token.
        let _ = unsafe { set_marker(marker, token) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflex_mode_parser_enforces_on_when_dlss_fg_is_enabled() {
        assert_eq!(ReflexModeConfig::parse(None, false), ReflexModeConfig::Off);
        assert_eq!(ReflexModeConfig::parse(Some("off"), false), ReflexModeConfig::Off);
        assert_eq!(ReflexModeConfig::parse(Some("on"), false), ReflexModeConfig::On);
        assert_eq!(ReflexModeConfig::parse(Some("boost"), false), ReflexModeConfig::Boost);
        assert_eq!(ReflexModeConfig::parse(Some("on+boost"), false), ReflexModeConfig::Boost);
        // DLSS-G requires Reflex active even when unset or explicitly off.
        assert_eq!(ReflexModeConfig::parse(None, true), ReflexModeConfig::On);
        assert_eq!(ReflexModeConfig::parse(Some("off"), true), ReflexModeConfig::On);
        assert_eq!(ReflexModeConfig::parse(Some("boost"), true), ReflexModeConfig::Boost);
    }
}

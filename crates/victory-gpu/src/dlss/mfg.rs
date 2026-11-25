//! NVIDIA Streamline DLSS-G Multi Frame Generation (2x / 3x / 4x / Dynamic).
//!
//! Snapshots depth, motion vectors, and (when format-compatible) post-upscale
//! HUD-less color during `ffxDispatch` into dedicated host textures kept in
//! `D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE` through `Present`, tags
//! them with the active `FrameToken`, and auto-pauses (`DLSSGMode::eOff` with
//! `eRetainResourcesWhenOff`) on non-gameplay frames (boot/menus/loading) and
//! before `ResizeBuffers`.

use std::collections::VecDeque;
use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

use crate::host::{HostList, host, transition};

use super::ffi::*;
use super::streamline::{HudlessMode, MAIN_VIEWPORT, StreamlineRuntime, config};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FgModeConfig {
    Off,
    /// Fixed multiplier: `num_frames_to_generate` is `1` (2x), `2` (3x),
    /// `3` (4x), `4` (5x), or `5` (6x), clamped at runtime to
    /// `DLSSGState::numFramesToGenerateMax`.
    Fixed(u32),
    Dynamic,
    Auto,
}

impl FgModeConfig {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("2x" | "2" | "on" | "1") => Self::Fixed(1),
            Some("3x" | "3") => Self::Fixed(2),
            Some("4x" | "4") => Self::Fixed(3),
            Some("5x" | "5") => Self::Fixed(4),
            Some("6x" | "6") => Self::Fixed(5),
            Some("dynamic" | "dyn" | "dmfg") => Self::Dynamic,
            Some("auto") => Self::Auto,
            _ => Self::Off,
        }
    }

    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Desired `DLSSGOptions` tuple computed by [`FgStateMachine`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectiveFgDecision {
    pub mode: u32,
    pub num_frames_to_generate: u32,
    pub flags: u32,
    pub dynamic_target_fps: f32,
    pub tag_inputs: bool,
}

impl EffectiveFgDecision {
    pub fn paused() -> Self {
        Self {
            mode: DLSSG_MODE_OFF,
            num_frames_to_generate: 1,
            flags: DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF,
            dynamic_target_fps: 0.0,
            tag_inputs: false,
        }
    }
}

/// Pure, unit-testable DLSS-G activation & auto-pause state machine.
#[derive(Clone, Debug)]
pub struct FgStateMachine {
    pub requested: FgModeConfig,
    pub target_fps: f32,
    pub supported: bool,
    pub max_generated: u32,
    pub dynamic_mfg_supported: bool,
    pub min_width_or_height: u32,
    pub vsync_supported: bool,
    pub disabled_after_error: bool,
    pub last_applied: Option<(u32, u32, u32, u32)>,
    pub tags_non_null: bool,
}

impl FgStateMachine {
    pub fn new(requested: FgModeConfig, target_fps: f32) -> Self {
        Self {
            requested,
            target_fps,
            supported: false,
            max_generated: 1,
            dynamic_mfg_supported: false,
            min_width_or_height: 0,
            vsync_supported: false,
            disabled_after_error: false,
            last_applied: None,
            tags_non_null: false,
        }
    }

    pub fn apply_capabilities(&mut self, state: &DlssgState) {
        self.supported = true;
        self.max_generated = state.num_frames_to_generate_max.max(1);
        self.dynamic_mfg_supported = state.b_is_dynamic_mfg_supported == SL_TRUE;
        self.min_width_or_height = state.min_width_or_height;
        self.vsync_supported = state.b_is_vsync_support_available == SL_TRUE;
    }

    /// Computes what DLSS-G mode should be active for this `Present`, plus
    /// whether `slDLSSGSetOptions` must be called (only on real state transitions).
    pub fn decide_for_present(
        &mut self,
        has_gameplay_inputs: bool,
        swap_width: u32,
        swap_height: u32,
        swap_format_supported: bool,
        dynamic_res: bool,
    ) -> (EffectiveFgDecision, bool) {
        let decision = if !self.supported
            || self.disabled_after_error
            || !self.requested.is_enabled()
            || !has_gameplay_inputs
            || !swap_format_supported
            || swap_width < self.min_width_or_height
            || swap_height < self.min_width_or_height
        {
            EffectiveFgDecision::paused()
        } else {
            let mut flags = DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF;
            if dynamic_res {
                flags |= DLSSG_FLAG_DYNAMIC_RESOLUTION_ENABLED;
            }
            match self.requested {
                FgModeConfig::Off => EffectiveFgDecision::paused(),
                FgModeConfig::Fixed(n) => EffectiveFgDecision {
                    mode: DLSSG_MODE_ON,
                    num_frames_to_generate: n.clamp(1, self.max_generated),
                    flags,
                    dynamic_target_fps: 0.0,
                    tag_inputs: true,
                },
                FgModeConfig::Dynamic => {
                    if self.dynamic_mfg_supported {
                        EffectiveFgDecision {
                            mode: DLSSG_MODE_DYNAMIC,
                            num_frames_to_generate: self.max_generated,
                            flags,
                            dynamic_target_fps: self.target_fps,
                            tag_inputs: true,
                        }
                    } else {
                        // Graceful fallback to fixed 3x/2x (clamped to hardware max).
                        EffectiveFgDecision {
                            mode: DLSSG_MODE_ON,
                            num_frames_to_generate: 2u32.clamp(1, self.max_generated),
                            flags,
                            dynamic_target_fps: 0.0,
                            tag_inputs: true,
                        }
                    }
                }
                FgModeConfig::Auto => EffectiveFgDecision {
                    mode: DLSSG_MODE_AUTO,
                    num_frames_to_generate: 1,
                    flags,
                    dynamic_target_fps: 0.0,
                    tag_inputs: true,
                },
            }
        };

        let key = (
            decision.mode,
            decision.num_frames_to_generate,
            decision.flags,
            decision.dynamic_target_fps.to_bits(),
        );
        let changed = self.last_applied != Some(key);
        if changed {
            self.last_applied = Some(key);
        }
        (decision, changed)
    }

    pub fn force_pause(&mut self) -> (EffectiveFgDecision, bool) {
        let decision = EffectiveFgDecision::paused();
        let key = (
            decision.mode,
            decision.num_frames_to_generate,
            decision.flags,
            decision.dynamic_target_fps.to_bits(),
        );
        let changed = self.last_applied != Some(key);
        if changed {
            self.last_applied = Some(key);
        }
        (decision, changed)
    }
}

/// DLSS-G supports SDR UNORM and HDR10 R10G10B10A2 backbuffers, NOT FP16 scRGB
/// (`ProgrammingGuideDLSS_G.md` §11.0).
pub fn is_supported_swapchain_format(format: DXGI_FORMAT) -> bool {
    matches!(
        format,
        DXGI_FORMAT_R8G8B8A8_UNORM
            | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
            | DXGI_FORMAT_B8G8R8A8_UNORM
            | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
            | DXGI_FORMAT_R10G10B10A2_UNORM
    )
}

/// Whether HUD-less color should be tagged alongside depth & motion vectors.
/// Pre-tonemap linear HDR (`R16G16B16A16_FLOAT` / `R9G9B9E5`) does not match an
/// SDR `R8G8B8A8_UNORM` backbuffer in color space (`ProgrammingGuideDLSS_G.md` §5.1);
/// `Auto` tags HUD-less only when formats are in the same display space unless
/// explicitly overridden via `VICTORY_DLSS_FG_HUDLESS=on`.
pub fn should_tag_hudless(
    mode: HudlessMode,
    hudless_format: DXGI_FORMAT,
    swap_format: DXGI_FORMAT,
    hudless_size: [u32; 2],
    swap_size: [u32; 2],
) -> bool {
    if hudless_size != swap_size {
        return false;
    }
    match mode {
        HudlessMode::Off => false,
        HudlessMode::On => true,
        HudlessMode::Auto => {
            let linear_hdr = matches!(
                hudless_format,
                DXGI_FORMAT_R16G16B16A16_FLOAT
                    | DXGI_FORMAT_R11G11B10_FLOAT
                    | DXGI_FORMAT_R9G9B9E5_SHAREDEXP
            );
            !linear_hdr && hudless_format == swap_format
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TexKey {
    width: u64,
    height: u32,
    depth_or_array: u16,
    mip_levels: u16,
    format: i32,
}

struct SnapshotTex {
    key: TexKey,
    resource: ID3D12Resource,
}

// SAFETY: D3D12 resources are free-threaded COM objects.
unsafe impl Send for SnapshotTex {}

#[derive(Clone)]
struct PendingFrameInputs {
    consts: Constants,
    render_size: [u32; 2],
    upscale_size: [u32; 2],
    mv_display_res: bool,
    dynamic_res: bool,
    hudless_format: Option<DXGI_FORMAT>,
}

// SAFETY: `Constants::base.next` is always null in our constructed structs and
// every other field is a plain value (`f32`/`u32`/`u8`).
unsafe impl Send for PendingFrameInputs {}

/// Consecutive `Present`s without a new `ffxDispatch` before DLSS-G auto-pauses
/// (tolerates async recording/submission pipelining across frame token advances
/// while still pausing promptly on menus/loading screens).
const PAUSE_AFTER_MISSED_PRESENTS: u32 = 3;
const MAX_PENDING_QUEUE: usize = 4;

struct DispatchTracker {
    queue: VecDeque<PendingFrameInputs>,
    last_valid: Option<PendingFrameInputs>,
    missed_presents: u32,
}

impl DispatchTracker {
    fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(MAX_PENDING_QUEUE),
            last_valid: None,
            missed_presents: PAUSE_AFTER_MISSED_PRESENTS,
        }
    }

    fn push(&mut self, inputs: PendingFrameInputs) {
        if self.queue.len() >= MAX_PENDING_QUEUE {
            self.queue.pop_front();
        }
        self.queue.push_back(inputs);
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.last_valid = None;
        self.missed_presents = PAUSE_AFTER_MISSED_PRESENTS;
    }

    fn take_for_present(&mut self) -> Option<&PendingFrameInputs> {
        if let Some(next) = self.queue.pop_front() {
            self.missed_presents = 0;
            self.last_valid = Some(next);
        } else {
            self.missed_presents = self.missed_presents.saturating_add(1);
        }
        if self.missed_presents < PAUSE_AFTER_MISSED_PRESENTS {
            self.last_valid.as_ref()
        } else {
            None
        }
    }
}

struct MfgManager {
    machine: FgStateMachine,
    swap_buffers: u32,
    depth_snap: Option<SnapshotTex>,
    mvec_snap: Option<SnapshotTex>,
    hudless_snap: Option<SnapshotTex>,
    dispatches: DispatchTracker,
    logged_first_active: bool,
    logged_first_interpolated: bool,
    logged_hudless_decision: bool,
    present_frames_active: u64,
}

static MFG: Mutex<Option<MfgManager>> = Mutex::new(None);

fn with_mfg<R>(f: impl FnOnce(&mut MfgManager) -> R) -> R {
    let mut guard = MFG.lock().unwrap_or_else(|e| e.into_inner());
    let mgr = guard.get_or_insert_with(|| {
        let cfg = config();
        MfgManager {
            machine: FgStateMachine::new(cfg.fg_mode, cfg.fg_target_fps),
            swap_buffers: 0,
            depth_snap: None,
            mvec_snap: None,
            hudless_snap: None,
            dispatches: DispatchTracker::new(),
            logged_first_active: false,
            logged_first_interpolated: false,
            logged_hudless_decision: false,
            present_frames_active: 0,
        }
    });
    f(mgr)
}

/// True when Streamline's DLSS-G proxy swapchain is active, signalling `host.rs`
/// to disable `late_free_present` (`p.ahead`) so backbuffer copy lists finish
/// deterministically before `Present`.
pub fn fg_active_swapchain() -> bool {
    let cfg = config();
    if !cfg.fg_mode.is_enabled() {
        return false;
    }
    super::streamline::runtime()
        .and_then(|rt| rt.feature_fns())
        .is_some_and(|fns| fns.dlssg_supported)
}

pub fn initialize_after_device(rt: &StreamlineRuntime) {
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    let Some(get_state) = fns.dlssg_get_state else { return };
    let mut state = DlssgState::default();
    // SAFETY: querying initial capabilities with null options avoids VRAM estimate cost.
    let r = unsafe { get_state(&MAIN_VIEWPORT, &mut state, std::ptr::null()) };
    if !r.is_ok() {
        tracing::warn!(result = r.name(), code = r.0, "dlss: initial slDLSSGGetState failed");
        return;
    }
    with_mfg(|mgr| {
        mgr.machine.apply_capabilities(&state);
        tracing::info!(
            requested = ?mgr.machine.requested,
            max_generated = mgr.machine.max_generated,
            max_multiplier = mgr.machine.max_generated + 1,
            dynamic_mfg = mgr.machine.dynamic_mfg_supported,
            vsync_supported = mgr.machine.vsync_supported,
            min_dim = mgr.machine.min_width_or_height,
            status = state.status,
            "dlss: DLSS-G capabilities"
        );
    });
}

pub fn on_swapchain_created(
    rt: &StreamlineRuntime,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    buffers: u32,
) {
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    with_mfg(|mgr| {
        mgr.swap_buffers = buffers;
        // Start paused until real gameplay frames arrive with valid depth/MVs.
        let (decision, changed) = mgr.machine.force_pause();
        if changed {
            apply_dlssg_options(fns, decision, width, height, format, mgr.swap_buffers);
        }
    });
}

pub fn on_before_resize(rt: &StreamlineRuntime) {
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    let token = rt.ensure_frame_token();
    with_mfg(|mgr| {
        mgr.dispatches.clear();
        if mgr.machine.tags_non_null
            && let Some(tok) = token
        {
            clear_global_tags(rt, tok);
            mgr.machine.tags_non_null = false;
        }
        let (decision, changed) = mgr.machine.force_pause();
        if changed {
            apply_dlssg_options(fns, decision, 0, 0, DXGI_FORMAT_UNKNOWN, mgr.swap_buffers);
        }
    });
}

/// Computes an SRV- and `CopyResource`-compatible format + resource flags for
/// DLSS-G snapshot textures (handling concrete depth and planar depth-stencil
/// formats while never setting `DENY_SHADER_RESOURCE`).
pub fn snapshot_format_and_flags(
    format: DXGI_FORMAT,
    src_flags: D3D12_RESOURCE_FLAGS,
) -> (DXGI_FORMAT, D3D12_RESOURCE_FLAGS) {
    match format {
        DXGI_FORMAT_D32_FLOAT => (DXGI_FORMAT_R32_TYPELESS, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL),
        DXGI_FORMAT_D16_UNORM => (DXGI_FORMAT_R16_TYPELESS, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL),
        DXGI_FORMAT_D24_UNORM_S8_UINT | DXGI_FORMAT_R24G8_TYPELESS => (
            DXGI_FORMAT_R24G8_TYPELESS,
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        ),
        DXGI_FORMAT_D32_FLOAT_S8X24_UINT | DXGI_FORMAT_R32G8X24_TYPELESS => (
            DXGI_FORMAT_R32G8X24_TYPELESS,
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        ),
        other => (
            other,
            src_flags & D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        ),
    }
}

fn ensure_snapshot_tex<'a>(
    slot: &'a mut Option<SnapshotTex>,
    source: &ID3D12Resource,
) -> Option<&'a ID3D12Resource> {
    // SAFETY: querying description of a live D3D12 texture.
    let src_desc = unsafe { source.GetDesc() };
    if src_desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || src_desc.SampleDesc.Count > 1 {
        return None;
    }
    let (snap_format, snap_flags) = snapshot_format_and_flags(src_desc.Format, src_desc.Flags);
    let key = TexKey {
        width: src_desc.Width,
        height: src_desc.Height,
        depth_or_array: src_desc.DepthOrArraySize,
        mip_levels: src_desc.MipLevels,
        format: snap_format.0,
    };
    if slot.as_ref().is_some_and(|s| s.key == key) {
        return slot.as_ref().map(|s| &s.resource);
    }
    let device = &host()?.device;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: src_desc.Width,
        Height: src_desc.Height,
        DepthOrArraySize: src_desc.DepthOrArraySize,
        MipLevels: src_desc.MipLevels,
        Format: snap_format,
        SampleDesc: src_desc.SampleDesc,
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: snap_flags,
    };
    let mut created: Option<ID3D12Resource> = None;
    // SAFETY: valid committed texture description for copy + SRV read by Streamline.
    let res = unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            None,
            &mut created,
        )
    };
    match res {
        Ok(()) => {
            *slot = Some(SnapshotTex {
                key,
                resource: created?,
            });
            slot.as_ref().map(|s| &s.resource)
        }
        Err(error) => {
            static SNAP_ERR: std::sync::Once = std::sync::Once::new();
            SNAP_ERR.call_once(|| {
                tracing::warn!(%error, format = src_desc.Format.0, "dlss: failed to allocate DLSS-G snapshot texture");
            });
            None
        }
    }
}

fn copy_into_snapshot(
    list: &HostList,
    src: &ID3D12Resource,
    src_state: D3D12_RESOURCE_STATES,
    dst: &ID3D12Resource,
) {
    list.keep(dst);
    // SAFETY: `list.list` is open; `src` is in `src_state` and `dst` is in
    // `NON_PIXEL_SHADER_RESOURCE`, and both are restored after `CopyResource`.
    unsafe {
        list.list.ResourceBarrier(&[
            transition(src, src_state, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(
                dst,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                D3D12_RESOURCE_STATE_COPY_DEST,
            ),
        ]);
        list.list.CopyResource(dst, src);
        list.list.ResourceBarrier(&[
            transition(src, D3D12_RESOURCE_STATE_COPY_SOURCE, src_state),
            transition(
                dst,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            ),
        ]);
    }
}

/// Records GPU copies of depth, motion vectors, and (if enabled/compatible)
/// post-upscale HUD-less color during `ffxDispatch` so DLSS-G receives
/// uncorrupted snapshots at `Present`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_dispatch_snapshots(
    rt: &StreamlineRuntime,
    list: &HostList,
    consts: Constants,
    depth: &ID3D12Resource,
    mvec: &ID3D12Resource,
    hudless: Option<(&ID3D12Resource, D3D12_RESOURCE_STATES)>,
    render_size: [u32; 2],
    upscale_size: [u32; 2],
    create_flags: u32,
) {
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    let read_state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    let cfg = config();

    with_mfg(|mgr| {
        if !mgr.machine.supported || mgr.machine.disabled_after_error {
            return;
        }
        let Some(depth_dst) = ensure_snapshot_tex(&mut mgr.depth_snap, depth) else {
            return;
        };
        copy_into_snapshot(list, depth, read_state, depth_dst);

        let Some(mvec_dst) = ensure_snapshot_tex(&mut mgr.mvec_snap, mvec) else {
            return;
        };
        copy_into_snapshot(list, mvec, read_state, mvec_dst);

        let mut hudless_format = None;
        if cfg.fg_hudless != HudlessMode::Off
            && let Some((src, src_state)) = hudless
        {
            // SAFETY: live texture description query.
            let fmt = unsafe { src.GetDesc() }.Format;
            let should_copy = match cfg.fg_hudless {
                HudlessMode::On => true,
                HudlessMode::Off => false,
                HudlessMode::Auto => !matches!(
                    fmt,
                    DXGI_FORMAT_R16G16B16A16_FLOAT
                        | DXGI_FORMAT_R11G11B10_FLOAT
                        | DXGI_FORMAT_R9G9B9E5_SHAREDEXP
                ),
            };
            if should_copy && let Some(hud_dst) = ensure_snapshot_tex(&mut mgr.hudless_snap, src) {
                copy_into_snapshot(list, src, src_state, hud_dst);
                hudless_format = Some(fmt);
            } else {
                hudless_format = Some(fmt);
            }
        }

        mgr.dispatches.push(PendingFrameInputs {
            consts,
            render_size,
            upscale_size,
            mv_display_res: create_flags & super::sr::FFX_FLAG_DISPLAY_RES_MV != 0,
            dynamic_res: create_flags & super::sr::FFX_FLAG_DYNAMIC_RESOLUTION != 0,
            hudless_format,
        });
    });
}

/// Tags DLSS-G inputs and applies `slDLSSGSetOptions` right before `Present`.
pub fn on_before_present(
    rt: &StreamlineRuntime,
    token: *const FrameToken,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) {
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    let cfg = config();

    with_mfg(|mgr| {
        let active_inputs = mgr.dispatches.take_for_present().cloned();
        let has_matching_inputs = active_inputs.is_some()
            && mgr.depth_snap.is_some()
            && mgr.mvec_snap.is_some();

        let swap_ok = is_supported_swapchain_format(format);
        let dynamic_res = active_inputs.as_ref().is_some_and(|p| p.dynamic_res);
        let (decision, changed) =
            mgr.machine
                .decide_for_present(has_matching_inputs, width, height, swap_ok, dynamic_res);

        if decision.tag_inputs
            && let (Some(p), Some(depth_snap), Some(mvec_snap)) =
                (&active_inputs, &mgr.depth_snap, &mgr.mvec_snap)
        {
            // Ensure Streamline has `sl::Constants` bound to THIS exact `FrameToken`
            // even when `ffxDispatch` was recorded on a worker thread before the
            // previous frame's `advance_frame_token()`.
            rt.set_constants_once(token, &p.consts);
            let state_srv = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32;
            let render_ext = Extent {
                top: 0,
                left: 0,
                width: p.render_size[0],
                height: p.render_size[1],
            };
            let mv_ext = if p.mv_display_res {
                Extent {
                    top: 0,
                    left: 0,
                    width: p.upscale_size[0],
                    height: p.upscale_size[1],
                }
            } else {
                render_ext
            };

            let mut depth_res = Resource::tex2d(depth_snap.resource.as_raw(), state_srv);
            let mut mvec_res = Resource::tex2d(mvec_snap.resource.as_raw(), state_srv);
            let mut tags = vec![
                ResourceTag::new(
                    &raw mut depth_res,
                    BUFFER_TYPE_DEPTH,
                    LIFECYCLE_VALID_UNTIL_PRESENT,
                    render_ext,
                ),
                ResourceTag::new(
                    &raw mut mvec_res,
                    BUFFER_TYPE_MOTION_VECTORS,
                    LIFECYCLE_VALID_UNTIL_PRESENT,
                    mv_ext,
                ),
            ];

            let use_hudless = p.hudless_format.is_some_and(|hf| {
                should_tag_hudless(cfg.fg_hudless, hf, format, p.upscale_size, [width, height])
            });
            if !mgr.logged_hudless_decision {
                mgr.logged_hudless_decision = true;
                tracing::info!(
                    hudless_mode = ?cfg.fg_hudless,
                    hudless_format = p.hudless_format.map(|f| f.0),
                    swapchain_format = format.0,
                    upscale_size = ?p.upscale_size,
                    swapchain_size = ?[width, height],
                    tagging_hudless = use_hudless,
                    "dlss: DLSS-G HUD-less tagging decision"
                );
            }

            let mut hud_res_storage = mgr
                .hudless_snap
                .as_ref()
                .filter(|_| use_hudless)
                .map(|h| Resource::tex2d(h.resource.as_raw(), state_srv));
            if let Some(hud_res) = hud_res_storage.as_mut() {
                tags.push(ResourceTag::new(
                    hud_res,
                    BUFFER_TYPE_HUD_LESS_COLOR,
                    LIFECYCLE_VALID_UNTIL_PRESENT,
                    Extent { top: 0, left: 0, width, height },
                ));
            }

            // SAFETY: `tags` and `depth_res`/`mvec_res`/`hud_res_storage` live for the call.
            let r = unsafe {
                (rt.api.sl_set_tag_for_frame)(
                    token,
                    &MAIN_VIEWPORT,
                    tags.as_ptr(),
                    tags.len() as u32,
                    std::ptr::null_mut(),
                )
            };
            if r.is_ok() {
                mgr.machine.tags_non_null = true;
            } else {
                static TAG_ERR: std::sync::Once = std::sync::Once::new();
                TAG_ERR.call_once(|| {
                    tracing::warn!(result = r.name(), code = r.0, "dlss: slSetTagForFrame failed");
                });
            }
        } else if mgr.machine.tags_non_null {
            clear_global_tags(rt, token);
            mgr.machine.tags_non_null = false;
        }

        if changed {
            apply_dlssg_options(fns, decision, width, height, format, mgr.swap_buffers);
        }
    });
}

/// Checks `slDLSSGGetState` after `Present` (without requesting VRAM estimates)
/// and auto-disables on any non-OK runtime status to avoid pink error overlays.
pub fn on_after_present(rt: &StreamlineRuntime, present_ok: bool) {
    if !present_ok {
        return;
    }
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    let Some(get_state) = fns.dlssg_get_state else { return };

    with_mfg(|mgr| {
        let active = mgr
            .machine
            .last_applied
            .is_some_and(|(mode, ..)| mode != DLSSG_MODE_OFF);
        if !active {
            return;
        }
        let mut state = DlssgState::default();
        // SAFETY: passing null options avoids per-frame VRAM estimate overhead.
        let r = unsafe { get_state(&MAIN_VIEWPORT, &mut state, std::ptr::null()) };
        if !r.is_ok() {
            return;
        }
        if state.status != DLSSG_STATUS_OK {
            tracing::error!(
                status = format_args!("{:#x}", state.status),
                "dlss: DLSS-G reported non-OK runtime status; disabling DLSS-G to avoid corruption"
            );
            mgr.machine.disabled_after_error = true;
            let (decision, _) = mgr.machine.force_pause();
            apply_dlssg_options(fns, decision, 0, 0, DXGI_FORMAT_UNKNOWN, mgr.swap_buffers);
            return;
        }
        mgr.present_frames_active += 1;
        if !mgr.logged_first_active {
            mgr.logged_first_active = true;
            tracing::info!(
                presented_this_call = state.num_frames_actually_presented,
                max_generated = state.num_frames_to_generate_max,
                vsync_supported = state.b_is_vsync_support_available == SL_TRUE,
                dynamic_mfg = state.b_is_dynamic_mfg_supported == SL_TRUE,
                "dlss: DLSS-G Frame Generation active"
            );
        }
        if !mgr.logged_first_interpolated && state.num_frames_actually_presented > 1 {
            mgr.logged_first_interpolated = true;
            tracing::info!(
                presented_this_call = state.num_frames_actually_presented,
                active_present_seq = mgr.present_frames_active,
                "dlss: DLSS-G confirmed multi-frame interpolation"
            );
        }
    });
}

fn clear_global_tags(rt: &StreamlineRuntime, token: *const FrameToken) {
    let tags = [
        ResourceTag::new(
            std::ptr::null_mut(),
            BUFFER_TYPE_DEPTH,
            LIFECYCLE_VALID_UNTIL_PRESENT,
            Extent::default(),
        ),
        ResourceTag::new(
            std::ptr::null_mut(),
            BUFFER_TYPE_MOTION_VECTORS,
            LIFECYCLE_VALID_UNTIL_PRESENT,
            Extent::default(),
        ),
        ResourceTag::new(
            std::ptr::null_mut(),
            BUFFER_TYPE_HUD_LESS_COLOR,
            LIFECYCLE_VALID_UNTIL_PRESENT,
            Extent::default(),
        ),
    ];
    // SAFETY: null resource tags clear Streamline references safely without a command list.
    let _ = unsafe {
        (rt.api.sl_set_tag_for_frame)(
            token,
            &MAIN_VIEWPORT,
            tags.as_ptr(),
            tags.len() as u32,
            std::ptr::null_mut(),
        )
    };
}

fn apply_dlssg_options(
    fns: &super::streamline::FeatureFns,
    decision: EffectiveFgDecision,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    buffers: u32,
) {
    let Some(set_options) = fns.dlssg_set_options else { return };
    let opts = DlssgOptions {
        mode: decision.mode,
        num_frames_to_generate: decision.num_frames_to_generate,
        flags: decision.flags,
        num_back_buffers: buffers,
        color_width: width,
        color_height: height,
        color_buffer_format: format.0 as u32,
        dynamic_target_frame_rate: decision.dynamic_target_fps,
        ..DlssgOptions::default()
    };
    // SAFETY: `MAIN_VIEWPORT` and `opts` live for the call.
    let r = unsafe { set_options(&MAIN_VIEWPORT, &opts) };
    tracing::info!(
        mode = decision.mode,
        num_frames_to_generate = decision.num_frames_to_generate,
        dynamic_target_fps = decision.dynamic_target_fps,
        result = r.name(),
        "dlss: slDLSSGSetOptions transition"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dlssg_mode_parser_maps_multipliers_and_dynamic() {
        assert_eq!(FgModeConfig::parse(None), FgModeConfig::Off);
        assert_eq!(FgModeConfig::parse(Some("off")), FgModeConfig::Off);
        assert_eq!(FgModeConfig::parse(Some("2x")), FgModeConfig::Fixed(1));
        assert_eq!(FgModeConfig::parse(Some("3x")), FgModeConfig::Fixed(2));
        assert_eq!(FgModeConfig::parse(Some("4x")), FgModeConfig::Fixed(3));
        assert_eq!(FgModeConfig::parse(Some("dynamic")), FgModeConfig::Dynamic);
        assert_eq!(FgModeConfig::parse(Some("auto")), FgModeConfig::Auto);
    }

    #[test]
    fn dlssg_state_machine_auto_pauses_on_menus_and_clamps_to_hw_max() {
        let mut sm = FgStateMachine::new(FgModeConfig::Fixed(3), 0.0); // 4x requested
        let mut caps = DlssgState::default();
        caps.num_frames_to_generate_max = 3; // Blackwell 4x supported
        caps.min_width_or_height = 640;
        caps.b_is_dynamic_mfg_supported = SL_TRUE;
        sm.apply_capabilities(&caps);

        // Boot / menu frame (no ffxDispatch) -> paused, emits initial transition once.
        let (d0, changed0) = sm.decide_for_present(false, 3840, 2160, true, false);
        assert_eq!(d0.mode, DLSSG_MODE_OFF);
        assert_eq!(d0.flags, DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF);
        assert!(!d0.tag_inputs);
        assert!(changed0);

        // Second menu frame -> still paused, NO redundant slDLSSGSetOptions call.
        let (d1, changed1) = sm.decide_for_present(false, 3840, 2160, true, false);
        assert_eq!(d1.mode, DLSSG_MODE_OFF);
        assert!(!changed1);

        // First gameplay frame -> transitions cleanly to 4x (numFramesToGenerate = 3).
        let (d2, changed2) = sm.decide_for_present(true, 3840, 2160, true, false);
        assert_eq!(d2.mode, DLSSG_MODE_ON);
        assert_eq!(d2.num_frames_to_generate, 3);
        assert!(d2.tag_inputs);
        assert!(changed2);

        // Second gameplay frame -> stays active, NO redundant slDLSSGSetOptions call.
        let (d3, changed3) = sm.decide_for_present(true, 3840, 2160, true, false);
        assert_eq!(d3.mode, DLSSG_MODE_ON);
        assert!(!changed3);

        // Pause menu opens -> auto-pauses with retain-resources flag.
        let (d4, changed4) = sm.decide_for_present(false, 3840, 2160, true, false);
        assert_eq!(d4.mode, DLSSG_MODE_OFF);
        assert!(!d4.tag_inputs);
        assert!(changed4);

        // Hardware clamping on a 2x-only GPU (max = 1).
        let mut sm_2x = FgStateMachine::new(FgModeConfig::Fixed(3), 0.0);
        caps.num_frames_to_generate_max = 1;
        caps.b_is_dynamic_mfg_supported = SL_FALSE;
        sm_2x.apply_capabilities(&caps);
        let (clamped, _) = sm_2x.decide_for_present(true, 2560, 1440, true, false);
        assert_eq!(clamped.mode, DLSSG_MODE_ON);
        assert_eq!(clamped.num_frames_to_generate, 1);

        // Dynamic MFG fallback when unsupported by driver.
        let mut sm_dyn = FgStateMachine::new(FgModeConfig::Dynamic, 144.0);
        sm_dyn.apply_capabilities(&caps);
        let (fb, _) = sm_dyn.decide_for_present(true, 2560, 1440, true, false);
        assert_eq!(fb.mode, DLSSG_MODE_ON);
        assert_eq!(fb.num_frames_to_generate, 1);

        // Dynamic MFG active when supported.
        caps.num_frames_to_generate_max = 3;
        caps.b_is_dynamic_mfg_supported = SL_TRUE;
        let mut sm_dyn_ok = FgStateMachine::new(FgModeConfig::Dynamic, 144.0);
        sm_dyn_ok.apply_capabilities(&caps);
        let (dyn_dec, _) = sm_dyn_ok.decide_for_present(true, 3840, 2160, true, false);
        assert_eq!(dyn_dec.mode, DLSSG_MODE_DYNAMIC);
        assert_eq!(dyn_dec.dynamic_target_fps, 144.0);
    }

    #[test]
    fn hudless_auto_skips_linear_hdr_against_sdr_backbuffer() {
        assert!(!should_tag_hudless(
            HudlessMode::Auto,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            [3840, 2160],
            [3840, 2160],
        ));
        assert!(should_tag_hudless(
            HudlessMode::On,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            [3840, 2160],
            [3840, 2160],
        ));
        assert!(should_tag_hudless(
            HudlessMode::Auto,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            [3840, 2160],
            [3840, 2160],
        ));
        assert!(!should_tag_hudless(
            HudlessMode::On,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            [2560, 1440],
            [3840, 2160],
        ));
    }

    #[test]
    fn snapshot_format_promotes_depth_to_srv_compatible_typeless_and_strips_deny_srv() {
        let (fmt, flags) = snapshot_format_and_flags(
            DXGI_FORMAT_D32_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL | D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE,
        );
        assert_eq!(fmt, DXGI_FORMAT_R32_TYPELESS);
        assert_eq!(flags, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);

        let (fmt2, flags2) = snapshot_format_and_flags(
            DXGI_FORMAT_R24G8_TYPELESS,
            D3D12_RESOURCE_FLAG_NONE,
        );
        assert_eq!(fmt2, DXGI_FORMAT_R24G8_TYPELESS);
        assert_eq!(flags2, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);

        let (fmt3, flags3) = snapshot_format_and_flags(
            DXGI_FORMAT_R16G16_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS | D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        );
        assert_eq!(fmt3, DXGI_FORMAT_R16G16_FLOAT);
        assert_eq!(flags3, D3D12_RESOURCE_FLAG_NONE);
    }

    #[test]
    fn pipelined_dispatch_queue_survives_token_advance_and_pauses_after_misses() {
        let sample = |w: u32| PendingFrameInputs {
            consts: Constants::default(),
            render_size: [w, 1440],
            upscale_size: [3840, 2160],
            mv_display_res: false,
            dynamic_res: false,
            hudless_format: None,
        };
        let mut tracker = DispatchTracker::new();
        // Before any gameplay dispatch -> no inputs (paused on boot/menus).
        assert!(tracker.take_for_present().is_none());

        // Pipelined recording: two dispatches queued before Present pops them in FIFO order.
        tracker.push(sample(2560));
        tracker.push(sample(2400));
        assert_eq!(tracker.take_for_present().map(|p| p.render_size[0]), Some(2560));
        assert_eq!(tracker.take_for_present().map(|p| p.render_size[0]), Some(2400));

        // 1-2 frame hiccup without a new dispatch stays active via grace window.
        assert_eq!(tracker.take_for_present().map(|p| p.render_size[0]), Some(2400));
        assert_eq!(tracker.take_for_present().map(|p| p.render_size[0]), Some(2400));
        // 3rd consecutive miss (real menu/loading screen) -> auto-pauses.
        assert!(tracker.take_for_present().is_none());
    }
}

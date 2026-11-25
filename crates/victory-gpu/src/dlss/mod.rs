//! NVIDIA Streamline SDK v2.14.16 integration:
//! - DLSS 4.5 Super Resolution (Transformer presets K/L/M/J) via `ffx.rs`
//! - DLSS-G Multi Frame Generation (`2x`, `3x`, `4x`, `dynamic`, `auto`)
//! - NVIDIA Reflex Low Latency & PCL markers
//!
//! All Streamline DLLs are loaded dynamically from `VICTORY_STREAMLINE_DIR`
//! with zero link-time dependency and clean fallback to native D3D12 / AMD FFX.

pub mod ffi;
pub mod mfg;
pub mod reflex;
pub mod sr;
pub mod streamline;

use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_STATES, ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use windows::Win32::Graphics::Dxgi::DXGI_CREATE_FACTORY_FLAGS;
use windows::core::Interface;

use crate::host::HostList;

/// Whether any Streamline feature is configured (`VICTORY_STREAMLINE_DIR` + SR/FG/Reflex).
pub fn enabled() -> bool {
    streamline::config().wants_streamline()
}

/// Whether NVIDIA DLSS Super Resolution is requested (`VICTORY_DLSS_SR=1` + `VICTORY_STREAMLINE_DIR`).
pub fn sr_enabled() -> bool {
    let cfg = streamline::config();
    cfg.dir.is_some() && cfg.sr_requested
}

/// Whether NVIDIA DLSS-G Frame Generation is requested (`VICTORY_DLSS_FG != off` + `VICTORY_STREAMLINE_DIR`).
pub fn fg_enabled() -> bool {
    let cfg = streamline::config();
    cfg.dir.is_some() && cfg.fg_mode.is_enabled()
}

/// True when DLSS-G's proxy swapchain is active and `host.rs` must disable
/// `late_free_present` (`p.ahead`) for synchronized copy submission before `Present`.
pub fn fg_active_swapchain() -> bool {
    mfg::fg_active_swapchain()
}

/// Creates the host `ID3D12Device` through Streamline's interposer when enabled,
/// falling back cleanly to native `D3D12CreateDevice` otherwise.
pub fn create_d3d12_device(
    level: D3D_FEATURE_LEVEL,
    out: &mut Option<ID3D12Device>,
) -> windows::core::Result<()> {
    streamline::create_d3d12_device(level, out)
}

/// Creates a DXGI factory through Streamline's interposer when enabled so
/// `CreateSwapChainForHwnd` returns Streamline's DLSS-G proxy swapchain.
pub fn create_dxgi_factory2<T: Interface>(
    flags: DXGI_CREATE_FACTORY_FLAGS,
) -> windows::core::Result<T> {
    streamline::create_dxgi_factory2(flags)
}

/// Unwraps a Streamline `ID3D12Device` proxy to the underlying native
/// `ID3D12Device*` for 3rd-party SDKs (such as AMD FFX when paired with DLSS-G).
pub fn native_d3d12_device(device: &ID3D12Device) -> *mut std::ffi::c_void {
    streamline::native_d3d12_device(device)
}

pub fn on_swapchain_created(width: u32, height: u32, format: DXGI_FORMAT, buffers: u32) {
    if let Some(rt) = streamline::runtime() {
        mfg::on_swapchain_created(rt, width, height, format, buffers);
    }
}

pub fn on_before_resize() {
    if let Some(rt) = streamline::runtime() {
        mfg::on_before_resize(rt);
    }
}

pub fn on_render_submit_start() {
    if let Some(rt) = streamline::runtime() {
        reflex::on_render_submit_start(rt);
    }
}

pub fn on_render_submit_end() {
    if let Some(rt) = streamline::runtime() {
        reflex::on_render_submit_end(rt);
    }
}

/// Runs right before `IDXGISwapChain::Present`: tags DLSS-G frame snapshots (or
/// auto-pauses on menu/loading frames) and emits `PCLMarker::ePresentStart`.
pub fn before_present(width: u32, height: u32, format: DXGI_FORMAT) {
    let Some(rt) = streamline::runtime() else { return };
    let Some(token) = rt.ensure_frame_token() else { return };
    mfg::on_before_present(rt, token, width, height, format);
    reflex::on_present_start(rt, token);
}

/// Runs right after `IDXGISwapChain::Present`: emits `PCLMarker::ePresentEnd`,
/// checks `DLSSGState`, and opens the next frame token with `slReflexSleep`.
pub fn after_present(present_ok: bool) {
    let Some(rt) = streamline::runtime() else { return };
    let token = {
        let tracker = rt.frames.lock().unwrap_or_else(|e| e.into_inner());
        tracker.token_ptr()
    };
    if let Some(tok) = token {
        reflex::on_present_end(rt, tok);
    }
    mfg::on_after_present(rt, present_ok);
    rt.advance_frame_token();
}

/// Evaluates DLSS 4.5 SR and records DLSS-G snapshots during `ffxDispatch`.
/// Returns `true` if DLSS 4.5 SR ran on the GPU.
pub(crate) fn dispatch_sr_and_capture(
    list: &HostList,
    params: &sr::UpscaleFrameParams<'_>,
    hudless: Option<(&ID3D12Resource, D3D12_RESOURCE_STATES)>,
) -> bool {
    let Some(rt) = streamline::runtime() else { return false };
    let Some((token, _consts)) = sr::evaluate(rt, &list.list, params) else {
        return false;
    };
    mfg::record_dispatch_snapshots(
        rt,
        list,
        token,
        params.depth,
        params.mvec,
        hudless,
        params.render_size,
        params.upscale_size,
        params.create_flags,
    );
    true
}

/// Captures `sl::Constants` and DLSS-G snapshots when AMD FFX performed the SR
/// upscale (`VICTORY_DLSS_SR=0` + `VICTORY_DLSS_FG != off`).
pub(crate) fn capture_fg_after_external_sr(
    list: &HostList,
    params: &sr::UpscaleFrameParams<'_>,
    hudless: Option<(&ID3D12Resource, D3D12_RESOURCE_STATES)>,
) {
    let Some(rt) = streamline::runtime() else { return };
    let Some(fns) = rt.feature_fns() else { return };
    if !fns.dlssg_supported {
        return;
    }
    let Some(token) = rt.ensure_frame_token() else { return };
    let cfg = streamline::config();
    let consts = sr::build_common_constants(
        params.jitter,
        params.motion_vector_scale,
        params.render_size,
        params.camera,
        params.create_flags,
        params.reset,
        cfg.jitter_sign,
        cfg.mv_sign,
    );
    rt.set_constants_once(token, &consts);
    mfg::record_dispatch_snapshots(
        rt,
        list,
        token,
        params.depth,
        params.mvec,
        hudless,
        params.render_size,
        params.upscale_size,
        params.create_flags,
    );
}

pub fn free_sr_resources() {
    if let Some(rt) = streamline::runtime() {
        sr::free_resources(rt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_streamline_dir_resolves_none_and_hooks_are_safe_noops() {
        assert!(streamline::resolve_dll_dir(std::path::Path::new(r"Z:\nonexistent_streamline_dir")).is_none());
        // With Streamline uninitialized in unit-test environment, all hooks are safe no-ops.
        on_before_resize();
        on_render_submit_start();
        on_render_submit_end();
        before_present(2560, 1440, windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM);
        after_present(true);
        free_sr_resources();
    }
}

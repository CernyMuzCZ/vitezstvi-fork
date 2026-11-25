//! Dynamic loader and lifecycle manager for NVIDIA Streamline (`sl.interposer.dll`).
//!
//! Streamline is loaded strictly via `LoadLibraryW` + `GetProcAddress` from
//! `VICTORY_STREAMLINE_DIR` (zero link-time `.lib` dependency), so WARP, CI,
//! non-NVIDIA GPUs, and launches without Streamline DLLs behave unchanged.

use std::ffi::{CStr, CString, OsStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL;
use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{Interface, PCSTR, PCWSTR};

use super::ffi::*;
use super::mfg::FgModeConfig;
use super::reflex::ReflexModeConfig;
use super::sr::{DlssModeConfig, DlssPresetConfig};

/// Single primary viewport handle (Streamline DLSS-G docs specify viewport 0).
pub const MAIN_VIEWPORT: ViewportHandle = ViewportHandle::new(0);

/// Default RFC-4122 project GUID for `NVSDK_NGX_D3D12_Init_with_ProjectID`
/// (production `sl.common.dll` disables NGX if only `kTemporaryAppId` is set
/// without a project ID + engine version).
const DEFAULT_PROJECT_ID: &str = "a0f57b54-1daf-4934-90ae-c4035c19df04";
const DEFAULT_ENGINE_VERSION: &str = "1.0.0";

/// Parsed environment configuration for all NVIDIA Streamline features.
#[derive(Clone, Debug)]
pub struct StreamlineConfig {
    pub dir: Option<PathBuf>,
    pub sr_requested: bool,
    pub dlss_mode: DlssModeConfig,
    pub dlss_preset: DlssPresetConfig,
    pub dlss_sharpness: f32,
    pub jitter_sign: f32,
    pub mv_sign: f32,
    pub fg_mode: FgModeConfig,
    pub fg_target_fps: f32,
    pub fg_hudless: HudlessMode,
    pub reflex_mode: ReflexModeConfig,
    pub reflex_frame_limit_us: u32,
    pub log_level: u32,
    pub allow_ota: bool,
    pub app_id_override: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudlessMode {
    Auto,
    On,
    Off,
}

impl HudlessMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("1" | "on" | "true" | "yes") => Self::On,
            Some("0" | "off" | "false" | "no") => Self::Off,
            _ => Self::Auto,
        }
    }
}

impl StreamlineConfig {
    pub fn from_env() -> Self {
        let dir = std::env::var_os("VICTORY_STREAMLINE_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let sr_requested = parse_bool_env("VICTORY_DLSS_SR");
        let dlss_mode = DlssModeConfig::parse(std::env::var("VICTORY_DLSS_MODE").ok().as_deref());
        let dlss_preset =
            DlssPresetConfig::parse(std::env::var("VICTORY_DLSS_PRESET").ok().as_deref());
        let dlss_sharpness = std::env::var("VICTORY_DLSS_SHARPNESS")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let jitter_sign = parse_sign_env("VICTORY_DLSS_JITTER_SIGN");
        let mv_sign = parse_sign_env("VICTORY_DLSS_MV_SIGN");
        let fg_mode = FgModeConfig::parse(std::env::var("VICTORY_DLSS_FG").ok().as_deref());
        let fg_target_fps = std::env::var("VICTORY_DLSS_FG_TARGET_FPS")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(0.0);
        let fg_hudless = HudlessMode::parse(std::env::var("VICTORY_DLSS_FG_HUDLESS").ok().as_deref());
        let reflex_raw = std::env::var("VICTORY_REFLEX").ok();
        let reflex_mode = ReflexModeConfig::parse(reflex_raw.as_deref(), fg_mode.is_enabled());
        let reflex_frame_limit_us = std::env::var("VICTORY_REFLEX_FPS_LIMIT")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|&fps| fps > 0)
            .map_or(0, |fps| 1_000_000 / fps);
        let log_level = match std::env::var("VICTORY_STREAMLINE_LOG")
            .ok()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("verbose" | "2") => LOG_LEVEL_VERBOSE,
            Some("off" | "0") => LOG_LEVEL_OFF,
            _ => LOG_LEVEL_DEFAULT,
        };
        let allow_ota = parse_bool_env("VICTORY_STREAMLINE_OTA");
        let app_id_override = std::env::var("VICTORY_STREAMLINE_APP_ID")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|&id| id != 0);

        Self {
            dir,
            sr_requested,
            dlss_mode,
            dlss_preset,
            dlss_sharpness,
            jitter_sign,
            mv_sign,
            fg_mode,
            fg_target_fps,
            fg_hudless,
            reflex_mode,
            reflex_frame_limit_us,
            log_level,
            allow_ota,
            app_id_override,
        }
    }

    /// True if any Streamline feature (DLSS SR, DLSS-G FG, or Reflex) was requested
    /// and a Streamline DLL directory was provided.
    pub fn wants_streamline(&self) -> bool {
        self.dir.is_some()
            && (self.sr_requested
                || self.fg_mode.is_enabled()
                || self.reflex_mode != ReflexModeConfig::Off)
    }
}

fn parse_bool_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
}

fn parse_sign_env(name: &str) -> f32 {
    match std::env::var(name).ok().as_deref().map(str::trim) {
        Some("-1" | "-1.0" | "neg" | "invert") => -1.0,
        _ => 1.0,
    }
}

pub fn config() -> &'static StreamlineConfig {
    static CONFIG: OnceLock<StreamlineConfig> = OnceLock::new();
    CONFIG.get_or_init(StreamlineConfig::from_env)
}

/// Resolves the directory containing `sl.interposer.dll` given a base path.
pub fn resolve_dll_dir(base: &Path) -> Option<PathBuf> {
    let candidates = [
        base.to_path_buf(),
        base.join("bin").join("x64"),
        base.join("sdk").join("bin").join("x64"),
    ];
    candidates
        .into_iter()
        .find(|dir| dir.join("sl.interposer.dll").is_file())
}

fn encode_wide_nul(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

type Farproc = unsafe extern "system" fn() -> isize;

/// Core API symbols loaded dynamically from `sl.interposer.dll`.
pub struct StreamlineApi {
    _module: HMODULE,
    pub dll_dir: PathBuf,
    pub sl_init: FnSlInit,
    pub sl_shutdown: FnSlShutdown,
    pub sl_is_feature_supported: FnSlIsFeatureSupported,
    pub sl_is_feature_loaded: FnSlIsFeatureLoaded,
    pub sl_set_feature_loaded: FnSlSetFeatureLoaded,
    pub sl_evaluate_feature: FnSlEvaluateFeature,
    pub sl_free_resources: FnSlFreeResources,
    pub sl_set_tag_for_frame: FnSlSetTagForFrame,
    pub sl_set_constants: FnSlSetConstants,
    pub sl_get_feature_requirements: FnSlGetFeatureRequirements,
    pub sl_get_feature_version: FnSlGetFeatureVersion,
    pub sl_get_feature_function: FnSlGetFeatureFunction,
    pub sl_get_new_frame_token: FnSlGetNewFrameToken,
    pub sl_set_d3d_device: FnSlSetD3DDevice,
    pub sl_get_native_interface: FnSlGetNativeInterface,
    pub d3d12_create_device: FnD3D12CreateDevice,
    pub create_dxgi_factory2: FnCreateDxgiFactory2,
}

// SAFETY: `HMODULE` and exported C function pointers are process-global and immutable.
unsafe impl Send for StreamlineApi {}
// SAFETY: as above.
unsafe impl Sync for StreamlineApi {}

/// Resolved per-feature plugin function tables (loaded after `slSetD3DDevice`).
#[derive(Default, Clone, Copy)]
pub struct FeatureFns {
    pub dlss_supported: bool,
    pub dlss_set_options: Option<FnSlDlssSetOptions>,
    pub dlss_get_optimal_settings: Option<FnSlDlssGetOptimalSettings>,
    pub dlss_get_state: Option<FnSlDlssGetState>,

    pub dlssg_supported: bool,
    pub dlssg_set_options: Option<FnSlDlssgSetOptions>,
    pub dlssg_get_state: Option<FnSlDlssgGetState>,

    pub reflex_supported: bool,
    pub reflex_set_options: Option<FnSlReflexSetOptions>,
    pub reflex_sleep: Option<FnSlReflexSleep>,
    pub reflex_get_state: Option<FnSlReflexGetState>,

    pub pcl_supported: bool,
    pub pcl_set_marker: Option<FnSlPclSetMarker>,
    pub pcl_set_options: Option<FnSlPclSetOptions>,
}

// SAFETY: plain immutable function pointers and booleans.
unsafe impl Send for FeatureFns {}
// SAFETY: as above.
unsafe impl Sync for FeatureFns {}

/// Tracks the current Streamline frame token and per-frame single-shot guards.
#[derive(Default)]
pub struct FrameTracker {
    pub current_token: Option<usize>,
    pub constants_set_token: Option<usize>,
    pub sim_started_token: Option<usize>,
    pub sim_ended_token: Option<usize>,
    pub render_submit_started_token: Option<usize>,
    pub render_submit_ended_token: Option<usize>,
    pub frame_seq: u64,
}

impl FrameTracker {
    pub fn token_ptr(&self) -> Option<*const FrameToken> {
        self.current_token.map(|addr| addr as *const FrameToken)
    }
}

/// Process-wide initialized Streamline runtime.
pub struct StreamlineRuntime {
    pub api: StreamlineApi,
    pub fns: OnceLock<FeatureFns>,
    pub frames: Mutex<FrameTracker>,
}

impl StreamlineRuntime {
    pub fn feature_fns(&self) -> Option<&FeatureFns> {
        self.fns.get()
    }

    /// Returns the active frame token, allocating one (and running Reflex sleep
    /// + `SimulationStart`) if no token is open yet.
    pub fn ensure_frame_token(&self) -> Option<*const FrameToken> {
        let mut tracker = self.frames.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ptr) = tracker.token_ptr() {
            return Some(ptr);
        }
        let mut raw: *mut FrameToken = std::ptr::null_mut();
        // SAFETY: `raw` is a valid out pointer for `slGetNewFrameToken`.
        let r = unsafe { (self.api.sl_get_new_frame_token)(&mut raw, std::ptr::null()) };
        if !r.is_ok() || raw.is_null() {
            tracing::warn!(result = r.name(), code = r.0, "dlss: slGetNewFrameToken failed");
            return None;
        }
        let addr = raw as usize;
        tracker.current_token = Some(addr);
        tracker.frame_seq = tracker.frame_seq.wrapping_add(1);
        let token = raw as *const FrameToken;
        if let Some(fns) = self.feature_fns() {
            super::reflex::on_frame_start(fns, token, &mut tracker);
        }
        Some(token)
    }

    /// Closes the presented frame token and immediately opens the next frame
    /// token with `slReflexSleep` + `PCLMarker::SimulationStart`.
    pub fn advance_frame_token(&self) {
        let mut tracker = self.frames.lock().unwrap_or_else(|e| e.into_inner());
        tracker.current_token = None;
        tracker.constants_set_token = None;
        tracker.sim_started_token = None;
        tracker.sim_ended_token = None;
        tracker.render_submit_started_token = None;
        tracker.render_submit_ended_token = None;

        let mut raw: *mut FrameToken = std::ptr::null_mut();
        // SAFETY: `raw` is a valid out pointer for `slGetNewFrameToken`.
        let r = unsafe { (self.api.sl_get_new_frame_token)(&mut raw, std::ptr::null()) };
        if !r.is_ok() || raw.is_null() {
            return;
        }
        tracker.current_token = Some(raw as usize);
        tracker.frame_seq = tracker.frame_seq.wrapping_add(1);
        let token = raw as *const FrameToken;
        if let Some(fns) = self.feature_fns() {
            super::reflex::on_frame_start(fns, token, &mut tracker);
        }
    }

    /// Calls `slSetConstants` at most once per active `FrameToken` to avoid
    /// `eErrorDuplicatedConstants`.
    pub fn set_constants_once(&self, token: *const FrameToken, consts: &Constants) -> bool {
        let addr = token as usize;
        {
            let tracker = self.frames.lock().unwrap_or_else(|e| e.into_inner());
            if tracker.constants_set_token == Some(addr) {
                return true;
            }
        }
        // SAFETY: `consts`, `token`, and `MAIN_VIEWPORT` live for the call.
        let r = unsafe { (self.api.sl_set_constants)(consts, token, &MAIN_VIEWPORT) };
        if r.is_ok() || r.0 == SlResult::ErrorDuplicatedConstants as i32 {
            let mut tracker = self.frames.lock().unwrap_or_else(|e| e.into_inner());
            tracker.constants_set_token = Some(addr);
            true
        } else {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                tracing::warn!(result = r.name(), code = r.0, "dlss: slSetConstants failed");
            });
            false
        }
    }
}

static RUNTIME: OnceLock<Option<StreamlineRuntime>> = OnceLock::new();

/// Returns the already-initialized Streamline runtime, if active.
pub fn runtime() -> Option<&'static StreamlineRuntime> {
    RUNTIME.get().and_then(Option::as_ref)
}

/// Initializes Streamline (`slInit`) before `D3D12CreateDevice` / `CreateDXGIFactory2`.
/// Safe to call repeatedly; returns `None` when Streamline is disabled or failed to load.
pub fn init_before_device() -> Option<&'static StreamlineRuntime> {
    RUNTIME
        .get_or_init(|| {
            let cfg = config();
            if !cfg.wants_streamline() {
                return None;
            }
            let base_dir = cfg.dir.as_deref()?;
            let Some(dll_dir) = resolve_dll_dir(base_dir) else {
                tracing::warn!(
                    ?base_dir,
                    "dlss: sl.interposer.dll not found under VICTORY_STREAMLINE_DIR; falling back to native D3D12"
                );
                return None;
            };
            let api = load_interposer(&dll_dir)?;
            if !call_sl_init(&api, cfg) {
                return None;
            }
            Some(StreamlineRuntime {
                api,
                fns: OnceLock::new(),
                frames: Mutex::new(FrameTracker::default()),
            })
        })
        .as_ref()
}

unsafe extern "C" fn sl_log_callback(log_type: u32, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    // SAFETY: Streamline passes a NUL-terminated UTF-8/ASCII log line.
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    match log_type {
        LOG_TYPE_ERROR => tracing::error!(target: "victory_gpu::dlss::sl", "{trimmed}"),
        LOG_TYPE_WARN => tracing::warn!(target: "victory_gpu::dlss::sl", "{trimmed}"),
        _ => tracing::info!(target: "victory_gpu::dlss::sl", "{trimmed}"),
    }
}

fn load_interposer(dll_dir: &Path) -> Option<StreamlineApi> {
    let interposer_path = dll_dir.join("sl.interposer.dll");
    let wide = encode_wide_nul(interposer_path.as_os_str());
    // SAFETY: nul-terminated absolute DLL path.
    let module = match unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) } {
        Ok(m) => m,
        Err(error) => {
            tracing::error!(?interposer_path, %error, "dlss: failed to load sl.interposer.dll");
            return None;
        }
    };

    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            // SAFETY: loaded module handle and nul-terminated export name.
            let Some(proc) = (unsafe { GetProcAddress(module, PCSTR(concat!($name, "\0").as_ptr())) }) else {
                tracing::error!(symbol = $name, "dlss: missing export in sl.interposer.dll");
                return None;
            };
            // SAFETY: export signature verified against Streamline SDK v2.14.16 headers.
            unsafe { std::mem::transmute::<Farproc, $ty>(proc) }
        }};
    }

    Some(StreamlineApi {
        _module: module,
        dll_dir: dll_dir.to_path_buf(),
        sl_init: sym!("slInit", FnSlInit),
        sl_shutdown: sym!("slShutdown", FnSlShutdown),
        sl_is_feature_supported: sym!("slIsFeatureSupported", FnSlIsFeatureSupported),
        sl_is_feature_loaded: sym!("slIsFeatureLoaded", FnSlIsFeatureLoaded),
        sl_set_feature_loaded: sym!("slSetFeatureLoaded", FnSlSetFeatureLoaded),
        sl_evaluate_feature: sym!("slEvaluateFeature", FnSlEvaluateFeature),
        sl_free_resources: sym!("slFreeResources", FnSlFreeResources),
        sl_set_tag_for_frame: sym!("slSetTagForFrame", FnSlSetTagForFrame),
        sl_set_constants: sym!("slSetConstants", FnSlSetConstants),
        sl_get_feature_requirements: sym!("slGetFeatureRequirements", FnSlGetFeatureRequirements),
        sl_get_feature_version: sym!("slGetFeatureVersion", FnSlGetFeatureVersion),
        sl_get_feature_function: sym!("slGetFeatureFunction", FnSlGetFeatureFunction),
        sl_get_new_frame_token: sym!("slGetNewFrameToken", FnSlGetNewFrameToken),
        sl_set_d3d_device: sym!("slSetD3DDevice", FnSlSetD3DDevice),
        sl_get_native_interface: sym!("slGetNativeInterface", FnSlGetNativeInterface),
        d3d12_create_device: sym!("D3D12CreateDevice", FnD3D12CreateDevice),
        create_dxgi_factory2: sym!("CreateDXGIFactory2", FnCreateDxgiFactory2),
    })
}

fn call_sl_init(api: &StreamlineApi, cfg: &StreamlineConfig) -> bool {
    let mut features = vec![FEATURE_PCL, FEATURE_REFLEX];
    if cfg.sr_requested {
        features.push(FEATURE_DLSS);
    }
    if cfg.fg_mode.is_enabled() {
        features.push(FEATURE_DLSS_G);
    }

    let plugin_dir_wide = encode_wide_nul(api.dll_dir.as_os_str());
    let plugin_dirs = [plugin_dir_wide.as_ptr()];

    let engine_ver = CString::new(DEFAULT_ENGINE_VERSION).unwrap();
    let project_id = CString::new(DEFAULT_PROJECT_ID).unwrap();

    let mut flags = PREF_DISABLE_CL_STATE_TRACKING
        | PREF_DISABLE_DEBUG_TEXT
        | PREF_USE_DXGI_FACTORY_PROXY
        | PREF_USE_FRAME_BASED_RESOURCE_TAGGING;
    if cfg.allow_ota {
        flags |= PREF_ALLOW_OTA | PREF_LOAD_DOWNLOADED_PLUGINS;
    }

    let mut prefs = Preferences {
        show_console: false,
        log_level: cfg.log_level,
        paths_to_plugins: plugin_dirs.as_ptr(),
        num_paths_to_plugins: plugin_dirs.len() as u32,
        path_to_logs_and_data: std::ptr::null(),
        log_message_callback: Some(sl_log_callback),
        flags,
        features_to_load: features.as_ptr(),
        num_features_to_load: features.len() as u32,
        render_api: RENDER_API_D3D12,
        ..Preferences::default()
    };

    if let Some(app_id) = cfg.app_id_override {
        prefs.application_id = app_id;
    } else {
        prefs.engine = ENGINE_CUSTOM;
        prefs.engine_version = engine_ver.as_ptr();
        prefs.project_id = project_id.as_ptr();
    }

    // SAFETY: all pointed-to arrays and strings live through `slInit` (Streamline copies them).
    let res = unsafe { (api.sl_init)(&prefs, SDK_VERSION) };
    if !res.is_ok() {
        tracing::error!(
            result = res.name(),
            code = res.0,
            dir = ?api.dll_dir,
            "dlss: slInit failed; falling back to native D3D12"
        );
        return false;
    }

    tracing::info!(
        sdk = format_args!("{SL_VERSION_MAJOR}.{SL_VERSION_MINOR}.{SL_VERSION_PATCH}"),
        dir = ?api.dll_dir,
        sr = cfg.sr_requested,
        fg = ?cfg.fg_mode,
        reflex = ?cfg.reflex_mode,
        "dlss: NVIDIA Streamline initialized"
    );
    true
}

/// Creates the host `ID3D12Device` via Streamline's interposer when enabled,
/// binds it with `slSetD3DDevice`, and resolves feature plugin tables.
pub fn create_d3d12_device(
    level: D3D_FEATURE_LEVEL,
    out: &mut Option<ID3D12Device>,
) -> windows::core::Result<()> {
    let Some(rt) = init_before_device() else {
        // SAFETY: standard D3D12 device creation when Streamline is inactive.
        return unsafe { D3D12CreateDevice(None, level, out) };
    };

    let mut raw_device: *mut c_void = std::ptr::null_mut();
    // SAFETY: calls Streamline's exported `D3D12CreateDevice` on the default adapter.
    let hr = unsafe {
        (rt.api.d3d12_create_device)(
            std::ptr::null_mut(),
            level.0,
            &ID3D12Device::IID,
            &mut raw_device,
        )
    };
    if hr.is_err() || raw_device.is_null() {
        tracing::warn!(
            hresult = %format_args!("{:#010x}", hr.0 as u32),
            "dlss: interposed D3D12CreateDevice failed; falling back to native D3D12CreateDevice"
        );
        // SAFETY: fallback to native D3D12CreateDevice.
        return unsafe { D3D12CreateDevice(None, level, out) };
    }

    // SAFETY: `raw_device` is a valid newly-created COM `ID3D12Device` (owned ref).
    let device = unsafe { ID3D12Device::from_raw(raw_device) };
    // SAFETY: informs Streamline which D3D12 device to initialize plugins on.
    let set_res = unsafe { (rt.api.sl_set_d3d_device)(device.as_raw()) };
    if !set_res.is_ok() {
        tracing::error!(
            result = set_res.name(),
            code = set_res.0,
            "dlss: slSetD3DDevice failed"
        );
    } else {
        let fns = resolve_feature_functions(rt, &device);
        let _ = rt.fns.set(fns);
        super::reflex::initialize_after_device(rt);
        super::mfg::initialize_after_device(rt);
    }

    *out = Some(device);
    Ok(())
}

/// Creates a DXGI factory via Streamline's interposer when Streamline is active
/// (enabling `eUseDXGIFactoryProxy` so `CreateSwapChainForHwnd` returns the
/// DLSS-G proxy swapchain).
pub fn create_dxgi_factory2<T: Interface>(
    flags: DXGI_CREATE_FACTORY_FLAGS,
) -> windows::core::Result<T> {
    let Some(rt) = runtime() else {
        // SAFETY: standard DXGI factory creation when Streamline is inactive.
        return unsafe { CreateDXGIFactory2(flags) };
    };
    let mut raw: *mut c_void = std::ptr::null_mut();
    // SAFETY: calls Streamline's interposed `CreateDXGIFactory2` for `T::IID`.
    let hr = unsafe { (rt.api.create_dxgi_factory2)(flags.0, &T::IID, &mut raw) };
    if hr.is_ok() && !raw.is_null() {
        // SAFETY: `raw` is a newly created COM interface of type `T`.
        return Ok(unsafe { T::from_raw(raw) });
    }
    tracing::warn!(
        hresult = %format_args!("{:#010x}", hr.0 as u32),
        "dlss: interposed CreateDXGIFactory2 failed; falling back to native CreateDXGIFactory2"
    );
    // SAFETY: native DXGI fallback.
    unsafe { CreateDXGIFactory2(flags) }
}

/// Unwraps a Streamline `ID3D12Device` proxy to its underlying native
/// `ID3D12Device*` (non-owning) for 3rd-party SDKs like AMD FidelityFX.
pub fn native_d3d12_device(device: &ID3D12Device) -> *mut c_void {
    let raw = device.as_raw();
    let Some(rt) = runtime() else {
        return raw;
    };
    let mut base: *mut c_void = std::ptr::null_mut();
    // SAFETY: `raw` is a live `ID3D12Device` (possibly an SL proxy); `base` is
    // our out pointer and Streamline returns a non-owning pointer to `m_base`.
    let r = unsafe { (rt.api.sl_get_native_interface)(raw, &mut base) };
    if r.is_ok() && !base.is_null() {
        base
    } else {
        raw
    }
}

fn query_feature_fn<T>(api: &StreamlineApi, feature: Feature, name: &'static [u8]) -> Option<T> {
    let mut raw: *mut c_void = std::ptr::null_mut();
    // SAFETY: `name` is NUL-terminated; `raw` is our out pointer.
    let r = unsafe { (api.sl_get_feature_function)(feature, name.as_ptr().cast(), &mut raw) };
    if !r.is_ok() || raw.is_null() {
        return None;
    }
    // SAFETY: caller supplies the exact function-pointer type matching `name`.
    Some(unsafe { std::mem::transmute_copy(&raw) })
}

fn check_support_and_log(
    api: &StreamlineApi,
    feature: Feature,
    label: &str,
    adapter: &AdapterInfo,
) -> bool {
    // SAFETY: `adapter` lives for the call.
    let support = unsafe { (api.sl_is_feature_supported)(feature, adapter) };
    let mut loaded = false;
    // SAFETY: `loaded` lives for the call.
    let loaded_res = unsafe { (api.sl_is_feature_loaded)(feature, &mut loaded) };
    let mut version = FeatureVersion::default();
    // SAFETY: `version` lives for the call.
    let ver_ok = unsafe { (api.sl_get_feature_version)(feature, &mut version) }.is_ok();
    let ok = support.is_ok() && loaded_res.is_ok() && loaded;
    if ver_ok {
        tracing::info!(
            feature = label,
            supported = support.name(),
            loaded,
            sl_ver = %format_args!("{}.{}.{}", version.version_sl.major, version.version_sl.minor, version.version_sl.build),
            ngx_ver = %format_args!("{}.{}.{}", version.version_ngx.major, version.version_ngx.minor, version.version_ngx.build),
            "dlss: feature status"
        );
    } else {
        tracing::info!(
            feature = label,
            supported = support.name(),
            loaded,
            "dlss: feature status"
        );
    }
    ok
}

fn resolve_feature_functions(rt: &StreamlineRuntime, device: &ID3D12Device) -> FeatureFns {
    let cfg = config();
    // SAFETY: plain D3D12 adapter LUID query.
    let mut luid = unsafe { device.GetAdapterLuid() };
    let adapter = AdapterInfo {
        device_luid: (&raw mut luid).cast::<u8>(),
        device_luid_size_in_bytes: std::mem::size_of_val(&luid) as u32,
        ..AdapterInfo::default()
    };

    let mut fns = FeatureFns {
        pcl_supported: check_support_and_log(&rt.api, FEATURE_PCL, "PCL", &adapter),
        reflex_supported: check_support_and_log(&rt.api, FEATURE_REFLEX, "Reflex", &adapter),
        dlss_supported: cfg.sr_requested
            && check_support_and_log(&rt.api, FEATURE_DLSS, "DLSS-SR", &adapter),
        dlssg_supported: cfg.fg_mode.is_enabled()
            && check_support_and_log(&rt.api, FEATURE_DLSS_G, "DLSS-G", &adapter),
        ..FeatureFns::default()
    };

    if fns.pcl_supported {
        fns.pcl_set_marker = query_feature_fn(&rt.api, FEATURE_PCL, b"slPCLSetMarker\0");
        fns.pcl_set_options = query_feature_fn(&rt.api, FEATURE_PCL, b"slPCLSetOptions\0");
    }
    if fns.reflex_supported {
        fns.reflex_set_options = query_feature_fn(&rt.api, FEATURE_REFLEX, b"slReflexSetOptions\0");
        fns.reflex_sleep = query_feature_fn(&rt.api, FEATURE_REFLEX, b"slReflexSleep\0");
        fns.reflex_get_state = query_feature_fn(&rt.api, FEATURE_REFLEX, b"slReflexGetState\0");
    }
    if fns.dlss_supported {
        fns.dlss_set_options = query_feature_fn(&rt.api, FEATURE_DLSS, b"slDLSSSetOptions\0");
        fns.dlss_get_optimal_settings =
            query_feature_fn(&rt.api, FEATURE_DLSS, b"slDLSSGetOptimalSettings\0");
        fns.dlss_get_state = query_feature_fn(&rt.api, FEATURE_DLSS, b"slDLSSGetState\0");
        if fns.dlss_set_options.is_none() {
            tracing::warn!("dlss: slDLSSSetOptions missing; disabling DLSS-SR");
            fns.dlss_supported = false;
        }
    }
    if fns.dlssg_supported {
        fns.dlssg_set_options = query_feature_fn(&rt.api, FEATURE_DLSS_G, b"slDLSSGSetOptions\0");
        fns.dlssg_get_state = query_feature_fn(&rt.api, FEATURE_DLSS_G, b"slDLSSGGetState\0");
        if fns.dlssg_set_options.is_none() || fns.dlssg_get_state.is_none() {
            tracing::warn!("dlss: DLSS-G functions missing; disabling DLSS-G");
            fns.dlssg_supported = false;
        }
    }
    fns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_version_and_dll_dir_resolution_work_for_flat_and_nested_layouts() {
        assert_eq!(SDK_VERSION, 0x0002_000e_0010_fedc);
        assert_eq!(RawResult::OK.name(), "eOk");
        assert_eq!(RawResult(SlResult::ErrorOSDisabledHWS as i32).name(), "eErrorOSDisabledHWS");

        let root = tempfile::tempdir().unwrap();
        assert!(resolve_dll_dir(root.path()).is_none());

        // Nested sdk/bin/x64 layout
        let nested = root.path().join("sdk").join("bin").join("x64");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("sl.interposer.dll"), b"").unwrap();
        assert_eq!(resolve_dll_dir(root.path()), Some(nested.clone()));

        // Direct flat layout takes priority when present
        std::fs::write(root.path().join("sl.interposer.dll"), b"").unwrap();
        assert_eq!(resolve_dll_dir(root.path()), Some(root.path().to_path_buf()));
    }

    #[test]
    fn hudless_mode_parser_handles_auto_on_off() {
        assert_eq!(HudlessMode::parse(None), HudlessMode::Auto);
        assert_eq!(HudlessMode::parse(Some("auto")), HudlessMode::Auto);
        assert_eq!(HudlessMode::parse(Some("on")), HudlessMode::On);
        assert_eq!(HudlessMode::parse(Some("1")), HudlessMode::On);
        assert_eq!(HudlessMode::parse(Some("off")), HudlessMode::Off);
        assert_eq!(HudlessMode::parse(Some("0")), HudlessMode::Off);
    }
}

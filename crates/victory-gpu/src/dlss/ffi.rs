//! Raw FFI bindings for NVIDIA Streamline SDK v2.14.16 (`sl.interposer.dll`).
//!
//! Every `#[repr(C)]` struct layout, GUID, version constant, and enum value is
//! matched byte-for-byte against the Streamline SDK v2.14.16 headers
//! (`sl_version.h`, `sl_struct.h`, `sl_result.h`, `sl_core_types.h`,
//! `sl_consts.h`, `sl_core_api.h`, `sl_dlss.h`, `sl_dlss_g.h`, `sl_reflex.h`,
//! `sl_pcl.h`) and validated against the `offsetof` static assertions in
//! `source/core/sl.api/sl.cpp`.
//!
//! FFI enum fields use raw integer types (`u32`, `i32`, `i16`, `i8`) and
//! `RawResult(i32)` rather than Rust `#[repr(...)] enum` discriminants so
//! future Streamline OTA updates with additional enum values cannot cause
//! invalid-discriminant undefined behavior across the C ABI boundary.

#![allow(dead_code)]

use std::ffi::{c_char, c_void};

use windows::core::{GUID, HRESULT};

pub const SL_VERSION_MAJOR: u64 = 2;
pub const SL_VERSION_MINOR: u64 = 14;
pub const SL_VERSION_PATCH: u64 = 16;
pub const SDK_VERSION_MAGIC: u64 = 0xfedc;
pub const SDK_VERSION: u64 = (SL_VERSION_MAJOR << 48)
    | (SL_VERSION_MINOR << 32)
    | (SL_VERSION_PATCH << 16)
    | SDK_VERSION_MAGIC;

pub const K_STRUCT_VERSION_1: usize = 1;
pub const K_STRUCT_VERSION_2: usize = 2;
pub const K_STRUCT_VERSION_3: usize = 3;
pub const K_STRUCT_VERSION_4: usize = 4;
pub const K_STRUCT_VERSION_5: usize = 5;

pub const INVALID_FLOAT: f32 = f32::MAX;
pub const INVALID_UINT: u32 = u32::MAX;

/// `sl::Result` discriminants (`sl_result.h`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlResult {
    Ok = 0,
    ErrorIO = 1,
    ErrorDriverOutOfDate = 2,
    ErrorOSOutOfDate = 3,
    ErrorOSDisabledHWS = 4,
    ErrorDeviceNotCreated = 5,
    ErrorNoSupportedAdapterFound = 6,
    ErrorAdapterNotSupported = 7,
    ErrorNoPlugins = 8,
    ErrorVulkanAPI = 9,
    ErrorDXGIAPI = 10,
    ErrorD3DAPI = 11,
    ErrorNRDAPI = 12,
    ErrorNVAPI = 13,
    ErrorReflexAPI = 14,
    ErrorNGXFailed = 15,
    ErrorJSONParsing = 16,
    ErrorMissingProxy = 17,
    ErrorMissingResourceState = 18,
    ErrorInvalidIntegration = 19,
    ErrorMissingInputParameter = 20,
    ErrorNotInitialized = 21,
    ErrorComputeFailed = 22,
    ErrorInitNotCalled = 23,
    ErrorExceptionHandler = 24,
    ErrorInvalidParameter = 25,
    ErrorMissingConstants = 26,
    ErrorDuplicatedConstants = 27,
    ErrorMissingOrInvalidAPI = 28,
    ErrorCommonConstantsMissing = 29,
    ErrorUnsupportedInterface = 30,
    ErrorFeatureMissing = 31,
    ErrorFeatureNotSupported = 32,
    ErrorFeatureMissingHooks = 33,
    ErrorFeatureFailedToLoad = 34,
    ErrorFeatureWrongPriority = 35,
    ErrorFeatureMissingDependency = 36,
    ErrorFeatureManagerInvalidState = 37,
    ErrorInvalidState = 38,
    WarnOutOfVRAM = 39,
}

/// ABI-safe wrapper around C `sl::Result` (`i32`).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawResult(pub i32);

impl RawResult {
    pub const OK: Self = Self(0);

    pub fn is_ok(self) -> bool {
        self.0 == SlResult::Ok as i32
    }

    pub fn name(self) -> &'static str {
        match self.0 {
            0 => "eOk",
            1 => "eErrorIO",
            2 => "eErrorDriverOutOfDate",
            3 => "eErrorOSOutOfDate",
            4 => "eErrorOSDisabledHWS",
            5 => "eErrorDeviceNotCreated",
            6 => "eErrorNoSupportedAdapterFound",
            7 => "eErrorAdapterNotSupported",
            8 => "eErrorNoPlugins",
            9 => "eErrorVulkanAPI",
            10 => "eErrorDXGIAPI",
            11 => "eErrorD3DAPI",
            12 => "eErrorNRDAPI",
            13 => "eErrorNVAPI",
            14 => "eErrorReflexAPI",
            15 => "eErrorNGXFailed",
            16 => "eErrorJSONParsing",
            17 => "eErrorMissingProxy",
            18 => "eErrorMissingResourceState",
            19 => "eErrorInvalidIntegration",
            20 => "eErrorMissingInputParameter",
            21 => "eErrorNotInitialized",
            22 => "eErrorComputeFailed",
            23 => "eErrorInitNotCalled",
            24 => "eErrorExceptionHandler",
            25 => "eErrorInvalidParameter",
            26 => "eErrorMissingConstants",
            27 => "eErrorDuplicatedConstants",
            28 => "eErrorMissingOrInvalidAPI",
            29 => "eErrorCommonConstantsMissing",
            30 => "eErrorUnsupportedInterface",
            31 => "eErrorFeatureMissing",
            32 => "eErrorFeatureNotSupported",
            33 => "eErrorFeatureMissingHooks",
            34 => "eErrorFeatureFailedToLoad",
            35 => "eErrorFeatureWrongPriority",
            36 => "eErrorFeatureMissingDependency",
            37 => "eErrorFeatureManagerInvalidState",
            38 => "eErrorInvalidState",
            39 => "eWarnOutOfVRAM",
            _ => "eUnknown",
        }
    }
}

/// `sl::Feature` (`sl_core_types.h`).
pub type Feature = u32;
pub const FEATURE_DLSS: Feature = 0;
pub const FEATURE_NIS: Feature = 2;
pub const FEATURE_REFLEX: Feature = 3;
pub const FEATURE_PCL: Feature = 4;
pub const FEATURE_DLSS_G: Feature = 1000;
pub const FEATURE_DLSS_RR: Feature = 1001;
pub const FEATURE_COMMON: Feature = u32::MAX;

/// `sl::BufferType` (`sl_core_types.h`).
pub type BufferType = u32;
pub const BUFFER_TYPE_DEPTH: BufferType = 0;
pub const BUFFER_TYPE_MOTION_VECTORS: BufferType = 1;
pub const BUFFER_TYPE_HUD_LESS_COLOR: BufferType = 2;
pub const BUFFER_TYPE_SCALING_INPUT_COLOR: BufferType = 3;
pub const BUFFER_TYPE_SCALING_OUTPUT_COLOR: BufferType = 4;
pub const BUFFER_TYPE_EXPOSURE: BufferType = 13;
pub const BUFFER_TYPE_UI_COLOR_AND_ALPHA: BufferType = 23;
pub const BUFFER_TYPE_TRANSPARENCY_HINT: BufferType = 27;
pub const BUFFER_TYPE_BIAS_CURRENT_COLOR_HINT: BufferType = 29;
pub const BUFFER_TYPE_REACTIVE_MASK_HINT: BufferType = 36;
pub const BUFFER_TYPE_TRANSPARENCY_AND_COMPOSITION_MASK_HINT: BufferType = 37;
pub const BUFFER_TYPE_BACKBUFFER: BufferType = 53;

/// `sl::Boolean` (`enum Boolean : char`, `sl_consts.h`).
pub type SlBoolean = i8;
pub const SL_FALSE: SlBoolean = 0;
pub const SL_TRUE: SlBoolean = 1;
pub const SL_INVALID: SlBoolean = 2;

pub const fn sl_bool(v: bool) -> SlBoolean {
    if v { SL_TRUE } else { SL_FALSE }
}

/// `sl::LogLevel` (`sl_core_types.h`).
pub const LOG_LEVEL_OFF: u32 = 0;
pub const LOG_LEVEL_DEFAULT: u32 = 1;
pub const LOG_LEVEL_VERBOSE: u32 = 2;

/// `sl::LogType` (`sl_core_types.h`).
pub const LOG_TYPE_INFO: u32 = 0;
pub const LOG_TYPE_WARN: u32 = 1;
pub const LOG_TYPE_ERROR: u32 = 2;

/// `sl::ResourceType` (`enum class ResourceType : char`, `sl_core_types.h`).
pub const RESOURCE_TYPE_TEX2D: i8 = 0;
pub const RESOURCE_TYPE_BUFFER: i8 = 1;
pub const RESOURCE_TYPE_COMMAND_QUEUE: i8 = 2;
pub const RESOURCE_TYPE_COMMAND_BUFFER: i8 = 3;
pub const RESOURCE_TYPE_COMMAND_POOL: i8 = 4;
pub const RESOURCE_TYPE_FENCE: i8 = 5;
pub const RESOURCE_TYPE_SWAPCHAIN: i8 = 6;
pub const RESOURCE_TYPE_HOST_FENCE: i8 = 7;
pub const RESOURCE_TYPE_UNKNOWN: i8 = 8;

/// `sl::ResourceLifecycle` (`sl_core_types.h`).
pub const LIFECYCLE_ONLY_VALID_NOW: i32 = 0;
pub const LIFECYCLE_VALID_UNTIL_PRESENT: i32 = 1;
pub const LIFECYCLE_VALID_UNTIL_EVALUATE: i32 = 2;

/// `sl::EngineType` (`sl_appidentity.h`).
pub const ENGINE_CUSTOM: u32 = 0;
pub const ENGINE_UNREAL: u32 = 1;
pub const ENGINE_UNITY: u32 = 2;

/// `sl::RenderAPI` (`sl_device_wrappers.h`).
pub const RENDER_API_D3D11: u32 = 0;
pub const RENDER_API_D3D12: u32 = 1;
pub const RENDER_API_VULKAN: u32 = 2;

/// `sl::PreferenceFlags` bitfield (`sl_core_types.h`).
pub const PREF_DISABLE_CL_STATE_TRACKING: u64 = 1 << 0;
pub const PREF_DISABLE_DEBUG_TEXT: u64 = 1 << 1;
pub const PREF_USE_MANUAL_HOOKING: u64 = 1 << 2;
pub const PREF_ALLOW_OTA: u64 = 1 << 3;
pub const PREF_BYPASS_OS_VERSION_CHECK: u64 = 1 << 4;
pub const PREF_USE_DXGI_FACTORY_PROXY: u64 = 1 << 5;
pub const PREF_LOAD_DOWNLOADED_PLUGINS: u64 = 1 << 6;
pub const PREF_USE_FRAME_BASED_RESOURCE_TAGGING: u64 = 1 << 7;

/// `sl::StructType` (`sl_struct.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructType {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl StructType {
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }
}

/// `sl::BaseStructure` (`sl_struct.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BaseStructure {
    pub next: *mut BaseStructure,
    pub struct_type: StructType,
    pub struct_version: usize,
}

impl BaseStructure {
    pub const fn new(struct_type: StructType, struct_version: usize) -> Self {
        Self {
            next: std::ptr::null_mut(),
            struct_type,
            struct_version,
        }
    }
}

/// `sl::Version` (`sl_version.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

/// `sl::float2` (`sl_consts.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float2 {
    pub x: f32,
    pub y: f32,
}

impl Float2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// `sl::float3` (`sl_consts.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Float3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// `sl::float4` (`sl_consts.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Float4 {
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

/// Row-major 4x4 matrix (`sl::float4x4`, `sl_consts.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float4x4 {
    pub row: [Float4; 4],
}

impl Float4x4 {
    pub const IDENTITY: Self = Self {
        row: [
            Float4::new(1.0, 0.0, 0.0, 0.0),
            Float4::new(0.0, 1.0, 0.0, 0.0),
            Float4::new(0.0, 0.0, 1.0, 0.0),
            Float4::new(0.0, 0.0, 0.0, 1.0),
        ],
    };

    pub const INVALID: Self = Self {
        row: [Float4::new(INVALID_FLOAT, INVALID_FLOAT, INVALID_FLOAT, INVALID_FLOAT); 4],
    };
}

/// `sl::Extent` (`sl_consts.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

/// `sl::APIError` (`sl_core_types.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ApiError {
    pub code: i32,
}

pub type LogMessageCallback = unsafe extern "C" fn(kind: u32, msg: *const c_char);
pub type OnApiErrorCallback = unsafe extern "C" fn(last_error: *const ApiError);

/// Opaque handle returned by `slGetNewFrameToken` (C++ class with virtual method).
#[repr(C)]
pub struct FrameToken {
    _opaque: [u8; 0],
}

/// `sl::Preferences` (`sl_core_types.h`).
pub const GUID_PREFERENCES: StructType = StructType::new(
    0x1ca1_0965,
    0xbf8e,
    0x432b,
    [0x8d, 0xa1, 0x67, 0x16, 0xd8, 0x79, 0xfb, 0x14],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Preferences {
    pub base: BaseStructure,
    pub show_console: bool,
    pub log_level: u32,
    pub paths_to_plugins: *const *const u16,
    pub num_paths_to_plugins: u32,
    pub path_to_logs_and_data: *const u16,
    pub allocate_callback: *const c_void,
    pub release_callback: *const c_void,
    pub log_message_callback: Option<LogMessageCallback>,
    pub flags: u64,
    pub features_to_load: *const Feature,
    pub num_features_to_load: u32,
    pub application_id: u32,
    pub engine: u32,
    pub engine_version: *const c_char,
    pub project_id: *const c_char,
    pub render_api: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_PREFERENCES, K_STRUCT_VERSION_1),
            show_console: false,
            log_level: LOG_LEVEL_DEFAULT,
            paths_to_plugins: std::ptr::null(),
            num_paths_to_plugins: 0,
            path_to_logs_and_data: std::ptr::null(),
            allocate_callback: std::ptr::null(),
            release_callback: std::ptr::null(),
            log_message_callback: None,
            flags: PREF_DISABLE_CL_STATE_TRACKING
                | PREF_USE_DXGI_FACTORY_PROXY
                | PREF_USE_FRAME_BASED_RESOURCE_TAGGING,
            features_to_load: std::ptr::null(),
            num_features_to_load: 0,
            application_id: 0,
            engine: ENGINE_CUSTOM,
            engine_version: std::ptr::null(),
            project_id: std::ptr::null(),
            render_api: RENDER_API_D3D12,
        }
    }
}

/// `sl::Resource` (`sl_core_types.h`).
pub const GUID_RESOURCE: StructType = StructType::new(
    0x3a9d_70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Resource {
    pub base: BaseStructure,
    pub kind: i8,
    pub native: *mut c_void,
    pub memory: *mut c_void,
    pub view: *mut c_void,
    pub state: u32,
    pub width: u32,
    pub height: u32,
    pub native_format: u32,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub gpu_virtual_address: u64,
    pub flags: u32,
    pub usage: u32,
    pub internal_flags: u16,
    pub reserved: u16,
}

impl Resource {
    pub fn tex2d(native: *mut c_void, state: u32) -> Self {
        Self {
            base: BaseStructure::new(GUID_RESOURCE, K_STRUCT_VERSION_1),
            kind: RESOURCE_TYPE_TEX2D,
            native,
            memory: std::ptr::null_mut(),
            view: std::ptr::null_mut(),
            state,
            width: 0,
            height: 0,
            native_format: 0,
            mip_levels: 0,
            array_layers: 0,
            gpu_virtual_address: 0,
            flags: 0,
            usage: 0,
            internal_flags: 0,
            reserved: 0,
        }
    }
}

/// `sl::ResourceTag` (`sl_core_types.h`).
pub const GUID_RESOURCE_TAG: StructType = StructType::new(
    0x4c6a_5aad,
    0xb445,
    0x496c,
    [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceTag {
    pub base: BaseStructure,
    pub resource: *mut Resource,
    pub kind: BufferType,
    pub lifecycle: i32,
    pub extent: Extent,
}

impl ResourceTag {
    pub fn new(
        resource: *mut Resource,
        kind: BufferType,
        lifecycle: i32,
        extent: Extent,
    ) -> Self {
        Self {
            base: BaseStructure::new(GUID_RESOURCE_TAG, K_STRUCT_VERSION_1),
            resource,
            kind,
            lifecycle,
            extent,
        }
    }
}

/// `sl::ViewportHandle` (`sl_core_types.h`).
pub const GUID_VIEWPORT_HANDLE: StructType = StructType::new(
    0x171b_6435,
    0x9b3c,
    0x4fc8,
    [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ViewportHandle {
    pub base: BaseStructure,
    pub value: u32,
}

impl ViewportHandle {
    pub const fn new(value: u32) -> Self {
        Self {
            base: BaseStructure::new(GUID_VIEWPORT_HANDLE, K_STRUCT_VERSION_1),
            value,
        }
    }
}

/// `sl::AdapterInfo` (`sl_core_types.h`).
pub const GUID_ADAPTER_INFO: StructType = StructType::new(
    0x0677_315f,
    0xa746,
    0x4492,
    [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AdapterInfo {
    pub base: BaseStructure,
    pub device_luid: *mut u8,
    pub device_luid_size_in_bytes: u32,
    pub vk_physical_device: *mut c_void,
}

impl Default for AdapterInfo {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_ADAPTER_INFO, K_STRUCT_VERSION_1),
            device_luid: std::ptr::null_mut(),
            device_luid_size_in_bytes: 0,
            vk_physical_device: std::ptr::null_mut(),
        }
    }
}

/// `sl::FeatureVersion` (`sl_core_types.h`).
pub const GUID_FEATURE_VERSION: StructType = StructType::new(
    0x6d5b_51f0,
    0x076b,
    0x486d,
    [0x99, 0x95, 0x5a, 0x56, 0x10, 0x43, 0xf5, 0xc1],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FeatureVersion {
    pub base: BaseStructure,
    pub version_sl: Version,
    pub version_ngx: Version,
}

impl Default for FeatureVersion {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_FEATURE_VERSION, K_STRUCT_VERSION_1),
            version_sl: Version::default(),
            version_ngx: Version::default(),
        }
    }
}

/// `sl::FeatureRequirements` (`sl_core_types.h`, `kStructVersion2`).
pub const GUID_FEATURE_REQUIREMENTS: StructType = StructType::new(
    0x6671_4097,
    0xac6d,
    0x4bc6,
    [0x89, 0x15, 0x1e, 0x0f, 0x55, 0xa6, 0xb6, 0x1f],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FeatureRequirements {
    pub base: BaseStructure,
    pub flags: u32,
    pub max_num_cpu_threads: u32,
    pub max_num_viewports: u32,
    pub num_required_tags: u32,
    pub required_tags: *const BufferType,
    pub os_version_detected: Version,
    pub os_version_required: Version,
    pub driver_version_detected: Version,
    pub driver_version_required: Version,
    pub vk_num_compute_queues_required: u32,
    pub vk_num_graphics_queues_required: u32,
    pub vk_num_device_extensions: u32,
    pub vk_device_extensions: *const *const c_char,
    pub vk_num_instance_extensions: u32,
    pub vk_instance_extensions: *const *const c_char,
    pub vk_num_features_12: u32,
    pub vk_features_12: *const *const c_char,
    pub vk_num_features_13: u32,
    pub vk_features_13: *const *const c_char,
    pub vk_num_optical_flow_queues_required: u32,
}

impl Default for FeatureRequirements {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_FEATURE_REQUIREMENTS, K_STRUCT_VERSION_2),
            flags: 0,
            max_num_cpu_threads: 0,
            max_num_viewports: 0,
            num_required_tags: 0,
            required_tags: std::ptr::null(),
            os_version_detected: Version::default(),
            os_version_required: Version::default(),
            driver_version_detected: Version::default(),
            driver_version_required: Version::default(),
            vk_num_compute_queues_required: 0,
            vk_num_graphics_queues_required: 0,
            vk_num_device_extensions: 0,
            vk_device_extensions: std::ptr::null(),
            vk_num_instance_extensions: 0,
            vk_instance_extensions: std::ptr::null(),
            vk_num_features_12: 0,
            vk_features_12: std::ptr::null(),
            vk_num_features_13: 0,
            vk_features_13: std::ptr::null(),
            vk_num_optical_flow_queues_required: 0,
        }
    }
}

/// `sl::Constants` (`sl_consts.h`, `kStructVersion2`).
pub const GUID_CONSTANTS: StructType = StructType::new(
    0xdcd3_5ad7,
    0x4e4a,
    0x4bad,
    [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Constants {
    pub base: BaseStructure,
    pub camera_view_to_clip: Float4x4,
    pub clip_to_camera_view: Float4x4,
    pub clip_to_lens_clip: Float4x4,
    pub clip_to_prev_clip: Float4x4,
    pub prev_clip_to_clip: Float4x4,
    pub jitter_offset: Float2,
    pub mvec_scale: Float2,
    pub camera_pinhole_offset: Float2,
    pub camera_pos: Float3,
    pub camera_up: Float3,
    pub camera_right: Float3,
    pub camera_fwd: Float3,
    pub camera_near: f32,
    pub camera_far: f32,
    pub camera_fov: f32,
    pub camera_aspect_ratio: f32,
    pub motion_vectors_invalid_value: f32,
    pub depth_inverted: SlBoolean,
    pub camera_motion_included: SlBoolean,
    pub motion_vectors_3d: SlBoolean,
    pub reset: SlBoolean,
    pub orthographic_projection: SlBoolean,
    pub motion_vectors_dilated: SlBoolean,
    pub motion_vectors_jittered: SlBoolean,
    pub min_relative_linear_depth_object_separation: f32,
}

impl Default for Constants {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_CONSTANTS, K_STRUCT_VERSION_2),
            camera_view_to_clip: Float4x4::IDENTITY,
            clip_to_camera_view: Float4x4::IDENTITY,
            clip_to_lens_clip: Float4x4::INVALID,
            clip_to_prev_clip: Float4x4::IDENTITY,
            prev_clip_to_clip: Float4x4::IDENTITY,
            jitter_offset: Float2::new(0.0, 0.0),
            mvec_scale: Float2::new(1.0, 1.0),
            camera_pinhole_offset: Float2::new(0.0, 0.0),
            camera_pos: Float3::new(0.0, 0.0, 0.0),
            camera_up: Float3::new(0.0, 1.0, 0.0),
            camera_right: Float3::new(1.0, 0.0, 0.0),
            camera_fwd: Float3::new(0.0, 0.0, 1.0),
            camera_near: 0.1,
            camera_far: 10_000.0,
            camera_fov: std::f32::consts::FRAC_PI_3,
            camera_aspect_ratio: 16.0 / 9.0,
            motion_vectors_invalid_value: INVALID_FLOAT,
            depth_inverted: SL_TRUE,
            camera_motion_included: SL_TRUE,
            motion_vectors_3d: SL_FALSE,
            reset: SL_FALSE,
            orthographic_projection: SL_FALSE,
            motion_vectors_dilated: SL_FALSE,
            motion_vectors_jittered: SL_FALSE,
            min_relative_linear_depth_object_separation: 40.0,
        }
    }
}

/// `sl::DLSSMode` (`sl_dlss.h`).
pub const DLSS_MODE_OFF: u32 = 0;
pub const DLSS_MODE_MAX_PERFORMANCE: u32 = 1;
pub const DLSS_MODE_BALANCED: u32 = 2;
pub const DLSS_MODE_MAX_QUALITY: u32 = 3;
pub const DLSS_MODE_ULTRA_PERFORMANCE: u32 = 4;
pub const DLSS_MODE_ULTRA_QUALITY: u32 = 5;
pub const DLSS_MODE_DLAA: u32 = 6;

/// `sl::DLSSPreset` (`sl_dlss.h`).
pub const DLSS_PRESET_DEFAULT: u32 = 0;
pub const DLSS_PRESET_J: u32 = 10;
pub const DLSS_PRESET_K: u32 = 11;
pub const DLSS_PRESET_L: u32 = 12;
pub const DLSS_PRESET_M: u32 = 13;

/// `sl::DLSSOptions` (`sl_dlss.h`, `kStructVersion3`).
pub const GUID_DLSS_OPTIONS: StructType = StructType::new(
    0x6ac8_26e4,
    0x4c61,
    0x4101,
    [0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssOptions {
    pub base: BaseStructure,
    pub mode: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub sharpness: f32,
    pub pre_exposure: f32,
    pub exposure_scale: f32,
    pub color_buffers_hdr: SlBoolean,
    pub indicator_invert_axis_x: SlBoolean,
    pub indicator_invert_axis_y: SlBoolean,
    pub dlaa_preset: u32,
    pub quality_preset: u32,
    pub balanced_preset: u32,
    pub performance_preset: u32,
    pub ultra_performance_preset: u32,
    pub ultra_quality_preset: u32,
    pub use_auto_exposure: SlBoolean,
    pub alpha_upscaling_enabled: SlBoolean,
}

impl Default for DlssOptions {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_DLSS_OPTIONS, K_STRUCT_VERSION_3),
            mode: DLSS_MODE_OFF,
            output_width: INVALID_UINT,
            output_height: INVALID_UINT,
            sharpness: 0.0,
            pre_exposure: 1.0,
            exposure_scale: 1.0,
            color_buffers_hdr: SL_TRUE,
            indicator_invert_axis_x: SL_FALSE,
            indicator_invert_axis_y: SL_FALSE,
            dlaa_preset: DLSS_PRESET_DEFAULT,
            quality_preset: DLSS_PRESET_DEFAULT,
            balanced_preset: DLSS_PRESET_DEFAULT,
            performance_preset: DLSS_PRESET_DEFAULT,
            ultra_performance_preset: DLSS_PRESET_DEFAULT,
            ultra_quality_preset: DLSS_PRESET_DEFAULT,
            use_auto_exposure: SL_FALSE,
            alpha_upscaling_enabled: SL_FALSE,
        }
    }
}

/// `sl::DLSSOptimalSettings` (`sl_dlss.h`).
pub const GUID_DLSS_OPTIMAL_SETTINGS: StructType = StructType::new(
    0xef1d_0957,
    0xfd58,
    0x4df7,
    [0xb5, 0x04, 0x8b, 0x69, 0xd8, 0xaa, 0x6b, 0x76],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssOptimalSettings {
    pub base: BaseStructure,
    pub optimal_render_width: u32,
    pub optimal_render_height: u32,
    pub optimal_sharpness: f32,
    pub render_width_min: u32,
    pub render_height_min: u32,
    pub render_width_max: u32,
    pub render_height_max: u32,
}

impl Default for DlssOptimalSettings {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_DLSS_OPTIMAL_SETTINGS, K_STRUCT_VERSION_1),
            optimal_render_width: 0,
            optimal_render_height: 0,
            optimal_sharpness: 0.0,
            render_width_min: 0,
            render_height_min: 0,
            render_width_max: 0,
            render_height_max: 0,
        }
    }
}

/// `sl::DLSSState` (`sl_dlss.h`).
pub const GUID_DLSS_STATE: StructType = StructType::new(
    0x9366_b056,
    0x8c01,
    0x463c,
    [0xbb, 0x91, 0xe6, 0x87, 0x82, 0x63, 0x6c, 0xe9],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssState {
    pub base: BaseStructure,
    pub estimated_vram_usage_in_bytes: u64,
}

impl Default for DlssState {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_DLSS_STATE, K_STRUCT_VERSION_1),
            estimated_vram_usage_in_bytes: 0,
        }
    }
}

/// `sl::DLSSGMode` (`sl_dlss_g.h`).
pub const DLSSG_MODE_OFF: u32 = 0;
pub const DLSSG_MODE_ON: u32 = 1;
pub const DLSSG_MODE_AUTO: u32 = 2;
pub const DLSSG_MODE_DYNAMIC: u32 = 3;

/// `sl::DLSSGFlags` bitfield (`sl_dlss_g.h`).
pub const DLSSG_FLAG_SHOW_ONLY_INTERPOLATED_FRAME: u32 = 1 << 0;
pub const DLSSG_FLAG_DYNAMIC_RESOLUTION_ENABLED: u32 = 1 << 1;
pub const DLSSG_FLAG_REQUEST_VRAM_ESTIMATE: u32 = 1 << 2;
pub const DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF: u32 = 1 << 3;
pub const DLSSG_FLAG_ENABLE_FULLSCREEN_MENU_DETECTION: u32 = 1 << 4;

/// `sl::DLSSGStatus` bitfield (`sl_dlss_g.h`).
pub const DLSSG_STATUS_OK: u32 = 0;
pub const DLSSG_STATUS_FAIL_RESOLUTION_TOO_LOW: u32 = 1 << 0;
pub const DLSSG_STATUS_FAIL_REFLEX_NOT_DETECTED_AT_RUNTIME: u32 = 1 << 1;
pub const DLSSG_STATUS_FAIL_HDR_FORMAT_NOT_SUPPORTED: u32 = 1 << 2;
pub const DLSSG_STATUS_FAIL_COMMON_CONSTANTS_INVALID: u32 = 1 << 3;
pub const DLSSG_STATUS_FAIL_GET_CURRENT_BACK_BUFFER_INDEX_NOT_CALLED: u32 = 1 << 4;

/// `sl::DLSSGQueueParallelismMode` (`sl_dlss_g.h`).
pub const DLSSG_QUEUE_BLOCK_PRESENTING_CLIENT_QUEUE: u32 = 0;
pub const DLSSG_QUEUE_BLOCK_NO_CLIENT_QUEUES: u32 = 1;

/// `sl::DLSSGOptions` (`sl_dlss_g.h`, `kStructVersion5`).
pub const GUID_DLSSG_OPTIONS: StructType = StructType::new(
    0xfac5_f1cb,
    0x2dfd,
    0x4f36,
    [0xa1, 0xe6, 0x3a, 0x9e, 0x86, 0x52, 0x56, 0xc5],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DlssgOptions {
    pub base: BaseStructure,
    pub mode: u32,
    pub num_frames_to_generate: u32,
    pub flags: u32,
    pub dynamic_res_width: u32,
    pub dynamic_res_height: u32,
    pub num_back_buffers: u32,
    pub mvec_depth_width: u32,
    pub mvec_depth_height: u32,
    pub color_width: u32,
    pub color_height: u32,
    pub color_buffer_format: u32,
    pub mvec_buffer_format: u32,
    pub depth_buffer_format: u32,
    pub hud_less_buffer_format: u32,
    pub ui_buffer_format: u32,
    pub on_error_callback: Option<OnApiErrorCallback>,
    pub b_reserved15: SlBoolean,
    pub queue_parallelism_mode: u32,
    pub enable_user_interface_recomposition: SlBoolean,
    pub dynamic_target_frame_rate: f32,
}

impl Default for DlssgOptions {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_DLSSG_OPTIONS, K_STRUCT_VERSION_5),
            mode: DLSSG_MODE_OFF,
            num_frames_to_generate: 1,
            flags: DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF,
            dynamic_res_width: 0,
            dynamic_res_height: 0,
            num_back_buffers: 0,
            mvec_depth_width: 0,
            mvec_depth_height: 0,
            color_width: 0,
            color_height: 0,
            color_buffer_format: 0,
            mvec_buffer_format: 0,
            depth_buffer_format: 0,
            hud_less_buffer_format: 0,
            ui_buffer_format: 0,
            on_error_callback: None,
            b_reserved15: SL_INVALID,
            queue_parallelism_mode: DLSSG_QUEUE_BLOCK_PRESENTING_CLIENT_QUEUE,
            enable_user_interface_recomposition: SL_FALSE,
            dynamic_target_frame_rate: 0.0,
        }
    }
}

/// `sl::DLSSGState` (`sl_dlss_g.h`, `kStructVersion4`).
pub const GUID_DLSSG_STATE: StructType = StructType::new(
    0xcc8a_c8e1,
    0xa179,
    0x44f5,
    [0x97, 0xfa, 0xe7, 0x41, 0x12, 0xf9, 0xbc, 0x61],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssgState {
    pub base: BaseStructure,
    pub estimated_vram_usage_in_bytes: u64,
    pub status: u32,
    pub min_width_or_height: u32,
    pub num_frames_actually_presented: u32,
    pub num_frames_to_generate_max: u32,
    pub b_reserved4: SlBoolean,
    pub b_is_vsync_support_available: SlBoolean,
    pub inputs_processing_completion_fence: *mut c_void,
    pub last_present_inputs_processing_completion_fence_value: u64,
    pub b_is_dynamic_mfg_supported: SlBoolean,
}

impl Default for DlssgState {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_DLSSG_STATE, K_STRUCT_VERSION_4),
            estimated_vram_usage_in_bytes: 0,
            status: 0,
            min_width_or_height: 0,
            num_frames_actually_presented: 0,
            num_frames_to_generate_max: 0,
            b_reserved4: SL_FALSE,
            b_is_vsync_support_available: SL_FALSE,
            inputs_processing_completion_fence: std::ptr::null_mut(),
            last_present_inputs_processing_completion_fence_value: 0,
            b_is_dynamic_mfg_supported: SL_FALSE,
        }
    }
}

/// `sl::ReflexMode` (`sl_reflex.h`).
pub const REFLEX_MODE_OFF: i32 = 0;
pub const REFLEX_MODE_LOW_LATENCY: i32 = 1;
pub const REFLEX_MODE_LOW_LATENCY_WITH_BOOST: i32 = 2;

/// `sl::ReflexOptions` (`sl_reflex.h`, `kStructVersion1`).
pub const GUID_REFLEX_OPTIONS: StructType = StructType::new(
    0xf03a_f81a,
    0x6d0b,
    0x4902,
    [0xa6, 0x51, 0xc4, 0x96, 0x5e, 0x21, 0x54, 0x34],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ReflexOptions {
    pub base: BaseStructure,
    pub mode: i32,
    pub frame_limit_us: u32,
    pub use_markers_to_optimize: bool,
    pub virtual_key: u16,
    pub id_thread: u32,
}

impl Default for ReflexOptions {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_REFLEX_OPTIONS, K_STRUCT_VERSION_1),
            mode: REFLEX_MODE_OFF,
            frame_limit_us: 0,
            use_markers_to_optimize: false,
            virtual_key: 0,
            id_thread: 0,
        }
    }
}

/// `sl::ReflexReport` (`sl_reflex.h`, `kStructVersion1`).
pub const GUID_REFLEX_REPORT: StructType = StructType::new(
    0x0d56_9b37,
    0xa1c8,
    0x4453,
    [0xbe, 0x4d, 0x40, 0xf4, 0xde, 0x57, 0x95, 0x2b],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ReflexReport {
    pub base: BaseStructure,
    pub frame_id: u64,
    pub input_sample_time: u64,
    pub sim_start_time: u64,
    pub sim_end_time: u64,
    pub render_submit_start_time: u64,
    pub render_submit_end_time: u64,
    pub present_start_time: u64,
    pub present_end_time: u64,
    pub driver_start_time: u64,
    pub driver_end_time: u64,
    pub os_render_queue_start_time: u64,
    pub os_render_queue_end_time: u64,
    pub gpu_render_start_time: u64,
    pub gpu_render_end_time: u64,
    pub gpu_active_render_time_us: u32,
    pub gpu_frame_time_us: u32,
}

impl Default for ReflexReport {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_REFLEX_REPORT, K_STRUCT_VERSION_1),
            frame_id: 0,
            input_sample_time: 0,
            sim_start_time: 0,
            sim_end_time: 0,
            render_submit_start_time: 0,
            render_submit_end_time: 0,
            present_start_time: 0,
            present_end_time: 0,
            driver_start_time: 0,
            driver_end_time: 0,
            os_render_queue_start_time: 0,
            os_render_queue_end_time: 0,
            gpu_render_start_time: 0,
            gpu_render_end_time: 0,
            gpu_active_render_time_us: 0,
            gpu_frame_time_us: 0,
        }
    }
}

/// `sl::ReflexReport2` (`sl_reflex.h`, `kStructVersion1`).
pub const GUID_REFLEX_REPORT2: StructType = StructType::new(
    0x68bb_0632,
    0x5e1c,
    0x402b,
    [0x89, 0x9d, 0xb4, 0x9f, 0x63, 0x3c, 0x56, 0xc2],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ReflexReport2 {
    pub base: BaseStructure,
    pub camera_constructed_time: u64,
    pub cross_adapter_copy_time_us: u32,
}

impl Default for ReflexReport2 {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_REFLEX_REPORT2, K_STRUCT_VERSION_1),
            camera_constructed_time: 0,
            cross_adapter_copy_time_us: 0,
        }
    }
}

pub const REFLEX_FRAME_REPORT_COUNT: usize = 64;

/// `sl::ReflexState` (`sl_reflex.h`, `kStructVersion2`).
pub const GUID_REFLEX_STATE: StructType = StructType::new(
    0xf0bb_5985,
    0xdaf9,
    0x4728,
    [0xb2, 0xfd, 0xae, 0x80, 0xa2, 0xbd, 0x79, 0x89],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ReflexState {
    pub base: BaseStructure,
    pub low_latency_available: bool,
    pub latency_report_available: bool,
    pub stats_window_message: u32,
    pub frame_report: [ReflexReport; REFLEX_FRAME_REPORT_COUNT],
    pub flash_indicator_driver_controlled: bool,
    pub frame_report2: [ReflexReport2; REFLEX_FRAME_REPORT_COUNT],
}

impl Default for ReflexState {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_REFLEX_STATE, K_STRUCT_VERSION_2),
            low_latency_available: false,
            latency_report_available: false,
            stats_window_message: 0,
            frame_report: [ReflexReport::default(); REFLEX_FRAME_REPORT_COUNT],
            flash_indicator_driver_controlled: false,
            frame_report2: [ReflexReport2::default(); REFLEX_FRAME_REPORT_COUNT],
        }
    }
}

/// `sl::PCLHotKey` (`sl_pcl.h`).
pub const PCL_HOTKEY_USE_PING_MESSAGE: i16 = 0;

/// `sl::PCLOptions` (`sl_pcl.h`, `kStructVersion1`).
pub const GUID_PCL_OPTIONS: StructType = StructType::new(
    0xcfa3_2f9b,
    0x023c,
    0x420e,
    [0x90, 0x56, 0x68, 0x32, 0xb7, 0x4f, 0x89, 0xb4],
);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PclOptions {
    pub base: BaseStructure,
    pub virtual_key: i16,
    pub id_thread: u32,
}

impl Default for PclOptions {
    fn default() -> Self {
        Self {
            base: BaseStructure::new(GUID_PCL_OPTIONS, K_STRUCT_VERSION_1),
            virtual_key: PCL_HOTKEY_USE_PING_MESSAGE,
            id_thread: 0,
        }
    }
}

/// `sl::PCLMarker` (`sl_pcl.h`).
pub const PCL_MARKER_SIMULATION_START: u32 = 0;
pub const PCL_MARKER_SIMULATION_END: u32 = 1;
pub const PCL_MARKER_RENDER_SUBMIT_START: u32 = 2;
pub const PCL_MARKER_RENDER_SUBMIT_END: u32 = 3;
pub const PCL_MARKER_PRESENT_START: u32 = 4;
pub const PCL_MARKER_PRESENT_END: u32 = 5;
pub const PCL_MARKER_TRIGGER_FLASH: u32 = 7;
pub const PCL_MARKER_PC_LATENCY_PING: u32 = 8;

// Core `sl.interposer.dll` function signatures.
pub type FnSlInit = unsafe extern "C" fn(*const Preferences, u64) -> RawResult;
pub type FnSlShutdown = unsafe extern "C" fn() -> RawResult;
pub type FnSlIsFeatureSupported = unsafe extern "C" fn(Feature, *const AdapterInfo) -> RawResult;
pub type FnSlIsFeatureLoaded = unsafe extern "C" fn(Feature, *mut bool) -> RawResult;
pub type FnSlSetFeatureLoaded = unsafe extern "C" fn(Feature, bool) -> RawResult;
pub type FnSlEvaluateFeature = unsafe extern "C" fn(
    Feature,
    *const FrameToken,
    *const *const BaseStructure,
    u32,
    *mut c_void,
) -> RawResult;
pub type FnSlAllocateResources =
    unsafe extern "C" fn(*mut c_void, Feature, *const ViewportHandle) -> RawResult;
pub type FnSlFreeResources = unsafe extern "C" fn(Feature, *const ViewportHandle) -> RawResult;
pub type FnSlSetTagForFrame = unsafe extern "C" fn(
    *const FrameToken,
    *const ViewportHandle,
    *const ResourceTag,
    u32,
    *mut c_void,
) -> RawResult;
pub type FnSlGetFeatureRequirements =
    unsafe extern "C" fn(Feature, *mut FeatureRequirements) -> RawResult;
pub type FnSlGetFeatureVersion = unsafe extern "C" fn(Feature, *mut FeatureVersion) -> RawResult;
pub type FnSlSetConstants =
    unsafe extern "C" fn(*const Constants, *const FrameToken, *const ViewportHandle) -> RawResult;
pub type FnSlGetFeatureFunction =
    unsafe extern "C" fn(Feature, *const c_char, *mut *mut c_void) -> RawResult;
pub type FnSlGetNewFrameToken = unsafe extern "C" fn(*mut *mut FrameToken, *const u32) -> RawResult;
pub type FnSlSetD3DDevice = unsafe extern "C" fn(*mut c_void) -> RawResult;
pub type FnSlGetNativeInterface = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> RawResult;

// Interposed D3D12 / DXGI entrypoints exported by `sl.interposer.dll`.
pub type FnD3D12CreateDevice = unsafe extern "system" fn(
    p_adapter: *mut c_void,
    min_feature_level: i32,
    riid: *const GUID,
    pp_device: *mut *mut c_void,
) -> HRESULT;
pub type FnCreateDxgiFactory2 = unsafe extern "system" fn(
    flags: u32,
    riid: *const GUID,
    pp_factory: *mut *mut c_void,
) -> HRESULT;

// Feature plugin functions obtained via `slGetFeatureFunction`.
pub type FnSlDlssGetOptimalSettings =
    unsafe extern "C" fn(*const DlssOptions, *mut DlssOptimalSettings) -> RawResult;
pub type FnSlDlssGetState = unsafe extern "C" fn(*const ViewportHandle, *mut DlssState) -> RawResult;
pub type FnSlDlssSetOptions =
    unsafe extern "C" fn(*const ViewportHandle, *const DlssOptions) -> RawResult;
pub type FnSlDlssgGetState = unsafe extern "C" fn(
    *const ViewportHandle,
    *mut DlssgState,
    *const DlssgOptions,
) -> RawResult;
pub type FnSlDlssgSetOptions =
    unsafe extern "C" fn(*const ViewportHandle, *const DlssgOptions) -> RawResult;
pub type FnSlReflexSleep = unsafe extern "C" fn(*const FrameToken) -> RawResult;
pub type FnSlReflexSetOptions = unsafe extern "C" fn(*const ReflexOptions) -> RawResult;
pub type FnSlReflexGetState = unsafe extern "C" fn(*mut ReflexState) -> RawResult;
pub type FnSlPclSetMarker = unsafe extern "C" fn(u32, *const FrameToken) -> RawResult;
pub type FnSlPclSetOptions = unsafe extern "C" fn(*const PclOptions) -> RawResult;

// Compile-time ABI assertions matching `source/core/sl.api/sl.cpp` on 64-bit Windows.
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<StructType>() == 16);
    assert!(size_of::<BaseStructure>() == 32);
    assert!(size_of::<Extent>() == 16);
    assert!(offset_of!(Preferences, render_api) == 136);
    assert!(size_of::<Preferences>() == 144);
    assert!(offset_of!(Resource, internal_flags) == 104);
    assert!(offset_of!(Resource, reserved) == 106);
    assert!(size_of::<Resource>() == 112);
    assert!(offset_of!(ResourceTag, extent) == 48);
    assert!(size_of::<ResourceTag>() == 64);
    assert!(size_of::<ViewportHandle>() == 40);
    assert!(offset_of!(AdapterInfo, vk_physical_device) == 48);
    assert!(size_of::<AdapterInfo>() == 56);
    assert!(offset_of!(FeatureVersion, version_ngx) == 44);
    assert!(size_of::<FeatureVersion>() == 56);
    assert!(offset_of!(FeatureRequirements, vk_num_optical_flow_queues_required) == 176);
    assert!(size_of::<FeatureRequirements>() == 184);
    assert!(offset_of!(Constants, motion_vectors_jittered) == 450);
    assert!(offset_of!(Constants, min_relative_linear_depth_object_separation) == 452);
    assert!(size_of::<Constants>() == 456);
    assert!(size_of::<DlssOptions>() == 88);
    assert!(size_of::<DlssOptimalSettings>() == 64);
    assert!(size_of::<DlssState>() == 40);
    assert!(size_of::<DlssgOptions>() == 120);
    assert!(size_of::<DlssgState>() == 88);
    assert!(size_of::<ReflexOptions>() == 48);
    assert!(size_of::<ReflexReport>() == 152);
    assert!(size_of::<ReflexReport2>() == 48);
    assert!(offset_of!(ReflexState, stats_window_message) == 36);
    assert!(offset_of!(ReflexState, frame_report) == 40);
    assert!(offset_of!(ReflexState, flash_indicator_driver_controlled) == 9768);
    assert!(offset_of!(ReflexState, frame_report2) == 9776);
    assert!(size_of::<ReflexState>() == 12_848);
    assert!(size_of::<PclOptions>() == 40);
};

#[cfg(test)]
mod abi_tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn streamline_v2_14_16_struct_layouts_match_sdk_headers() {
        assert_eq!(SDK_VERSION, 0x0002_000e_0010_fedc);
        assert_eq!(size_of::<BaseStructure>(), 32);
        assert_eq!(offset_of!(BaseStructure, struct_type), 8);
        assert_eq!(offset_of!(BaseStructure, struct_version), 24);

        assert_eq!(size_of::<Preferences>(), 144);
        assert_eq!(offset_of!(Preferences, flags), 88);
        assert_eq!(offset_of!(Preferences, render_api), 136);

        assert_eq!(size_of::<Resource>(), 112);
        assert_eq!(offset_of!(Resource, native), 40);
        assert_eq!(offset_of!(Resource, state), 64);
        assert_eq!(offset_of!(Resource, internal_flags), 104);
        assert_eq!(offset_of!(Resource, reserved), 106);

        assert_eq!(size_of::<ResourceTag>(), 64);
        assert_eq!(offset_of!(ResourceTag, resource), 32);
        assert_eq!(offset_of!(ResourceTag, kind), 40);
        assert_eq!(offset_of!(ResourceTag, lifecycle), 44);
        assert_eq!(offset_of!(ResourceTag, extent), 48);

        assert_eq!(size_of::<Constants>(), 456);
        assert_eq!(offset_of!(Constants, jitter_offset), 352);
        assert_eq!(offset_of!(Constants, mvec_scale), 360);
        assert_eq!(offset_of!(Constants, depth_inverted), 444);
        assert_eq!(offset_of!(Constants, motion_vectors_jittered), 450);
        assert_eq!(offset_of!(Constants, min_relative_linear_depth_object_separation), 452);

        assert_eq!(size_of::<DlssOptions>(), 88);
        assert_eq!(offset_of!(DlssOptions, mode), 32);
        assert_eq!(offset_of!(DlssOptions, color_buffers_hdr), 56);
        assert_eq!(offset_of!(DlssOptions, dlaa_preset), 60);
        assert_eq!(offset_of!(DlssOptions, use_auto_exposure), 84);
        assert_eq!(DlssOptions::default().base.struct_version, K_STRUCT_VERSION_3);

        assert_eq!(size_of::<DlssgOptions>(), 120);
        assert_eq!(offset_of!(DlssgOptions, on_error_callback), 96);
        assert_eq!(offset_of!(DlssgOptions, queue_parallelism_mode), 108);
        assert_eq!(offset_of!(DlssgOptions, dynamic_target_frame_rate), 116);
        assert_eq!(DlssgOptions::default().base.struct_version, K_STRUCT_VERSION_5);

        assert_eq!(size_of::<DlssgState>(), 88);
        assert_eq!(offset_of!(DlssgState, num_frames_to_generate_max), 52);
        assert_eq!(offset_of!(DlssgState, b_is_dynamic_mfg_supported), 80);
        assert_eq!(DlssgState::default().base.struct_version, K_STRUCT_VERSION_4);

        assert_eq!(size_of::<ReflexOptions>(), 48);
        assert_eq!(size_of::<ReflexState>(), 12_848);
        assert_eq!(size_of::<PclOptions>(), 40);
    }
}

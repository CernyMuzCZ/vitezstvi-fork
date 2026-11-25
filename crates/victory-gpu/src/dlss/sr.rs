//! NVIDIA Streamline DLSS 4.5 Super Resolution (Transformer) backend and
//! camera/projection math for `ffx.rs::ffxDispatch`.

use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
    ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::core::Interface;

use super::ffi::*;
use super::streamline::{MAIN_VIEWPORT, StreamlineRuntime, config};

// AMD FidelityFX `FfxApiCreateContextUpscaleFlags` bits (`ffx_upscale.h`).
pub const FFX_FLAG_HDR: u32 = 1 << 0;
pub const FFX_FLAG_DISPLAY_RES_MV: u32 = 1 << 1;
pub const FFX_FLAG_MV_JITTER_CANCELLATION: u32 = 1 << 2;
pub const FFX_FLAG_DEPTH_INVERTED: u32 = 1 << 3;
pub const FFX_FLAG_DEPTH_INFINITE: u32 = 1 << 4;
pub const FFX_FLAG_AUTO_EXPOSURE: u32 = 1 << 5;
pub const FFX_FLAG_DYNAMIC_RESOLUTION: u32 = 1 << 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlssModeConfig {
    Auto,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
    UltraQuality,
    Dlaa,
}

impl DlssModeConfig {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("quality" | "max_quality" | "q") => Self::Quality,
            Some("balanced" | "b") => Self::Balanced,
            Some("performance" | "perf" | "max_performance" | "p") => Self::Performance,
            Some("ultra_performance" | "ultraperf" | "up") => Self::UltraPerformance,
            Some("ultra_quality" | "uq") => Self::UltraQuality,
            Some("dlaa" | "native") => Self::Dlaa,
            _ => Self::Auto,
        }
    }

    pub fn resolve(self, render_size: [u32; 2], upscale_size: [u32; 2]) -> u32 {
        match self {
            Self::Quality => DLSS_MODE_MAX_QUALITY,
            Self::Balanced => DLSS_MODE_BALANCED,
            Self::Performance => DLSS_MODE_MAX_PERFORMANCE,
            Self::UltraPerformance => DLSS_MODE_ULTRA_PERFORMANCE,
            Self::UltraQuality => DLSS_MODE_ULTRA_QUALITY,
            Self::Dlaa => DLSS_MODE_DLAA,
            Self::Auto => infer_dlss_mode(render_size, upscale_size),
        }
    }
}

/// Maps `render_size / upscale_size` to the closest standard `sl::DLSSMode`
/// (`0.6667` -> `eMaxQuality`, `0.58` -> `eBalanced`, `0.50` -> `eMaxPerformance`,
/// `0.33` -> `eUltraPerformance`, `1.0` -> `eDLAA`).
pub fn infer_dlss_mode(render_size: [u32; 2], upscale_size: [u32; 2]) -> u32 {
    if upscale_size[0] == 0 || upscale_size[1] == 0 {
        return DLSS_MODE_MAX_QUALITY;
    }
    let ratio = render_size[0] as f32 / upscale_size[0] as f32;
    if ratio >= 0.99 {
        DLSS_MODE_DLAA
    } else if ratio >= 0.62 {
        DLSS_MODE_MAX_QUALITY
    } else if ratio >= 0.54 {
        DLSS_MODE_BALANCED
    } else if ratio >= 0.42 {
        DLSS_MODE_MAX_PERFORMANCE
    } else {
        DLSS_MODE_ULTRA_PERFORMANCE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlssPresetConfig {
    /// Official DLSS 4.5 Transformer defaults (`sl_dlss.h` / `ProgrammingGuideDLSS.md`):
    /// Preset K for DLAA/Quality/Balanced, Preset M for Performance, Preset L for UltraPerformance.
    Auto,
    J,
    K,
    L,
    M,
}

impl DlssPresetConfig {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_uppercase()).as_deref() {
            Some("J") => Self::J,
            Some("K") => Self::K,
            Some("L") => Self::L,
            Some("M") => Self::M,
            _ => Self::Auto,
        }
    }

    pub fn apply(self, opts: &mut DlssOptions) {
        match self {
            Self::Auto => {
                opts.dlaa_preset = DLSS_PRESET_K;
                opts.quality_preset = DLSS_PRESET_K;
                opts.balanced_preset = DLSS_PRESET_K;
                opts.performance_preset = DLSS_PRESET_M;
                opts.ultra_performance_preset = DLSS_PRESET_L;
                opts.ultra_quality_preset = DLSS_PRESET_K;
            }
            Self::J | Self::K | Self::L | Self::M => {
                let p = match self {
                    Self::J => DLSS_PRESET_J,
                    Self::K => DLSS_PRESET_K,
                    Self::L => DLSS_PRESET_L,
                    Self::M => DLSS_PRESET_M,
                    Self::Auto => unreachable!(),
                };
                opts.dlaa_preset = p;
                opts.quality_preset = p;
                opts.balanced_preset = p;
                opts.performance_preset = p;
                opts.ultra_performance_preset = p;
                opts.ultra_quality_preset = p;
            }
        }
    }
}

/// Parameters forwarded from `ffx.rs::ffxDispatch` for both DLSS-SR and DLSS-G.
pub struct UpscaleFrameParams<'a> {
    pub color_in: &'a ID3D12Resource,
    pub color_out: &'a ID3D12Resource,
    pub depth: &'a ID3D12Resource,
    pub mvec: &'a ID3D12Resource,
    pub exposure: Option<&'a ID3D12Resource>,
    pub reactive: Option<&'a ID3D12Resource>,
    pub transparency: Option<&'a ID3D12Resource>,
    pub jitter: [f32; 2],
    pub motion_vector_scale: [f32; 2],
    pub render_size: [u32; 2],
    pub upscale_size: [u32; 2],
    pub pre_exposure: f32,
    pub sharpness: f32,
    pub reset: bool,
    /// FFX `cameraNear`, `cameraFar`, `cameraFovAngleVertical`, `viewSpaceToMetersFactor`.
    pub camera: [f32; 4],
    pub create_flags: u32,
}

/// Builds a row-major D3D12 left-handed perspective projection matrix and its
/// exact inverse, supporting both standard Z `[0..1]` and reversed Z `[1..0]`
/// (finite and infinite far plane).
pub fn build_projection_and_inverse(
    near: f32,
    far: f32,
    fov_y_rad: f32,
    aspect: f32,
    depth_inverted: bool,
    depth_infinite: bool,
) -> (Float4x4, Float4x4) {
    let near = if near.is_finite() && near > 1e-4 { near } else { 0.1 };
    let far = if far.is_finite() && far > near + 1e-3 { far } else { 10_000.0 };
    let fov = if fov_y_rad.is_finite() && fov_y_rad > 0.05 && fov_y_rad < 3.05 {
        fov_y_rad
    } else {
        std::f32::consts::FRAC_PI_3
    };
    let aspect = if aspect.is_finite() && aspect > 0.1 { aspect } else { 16.0 / 9.0 };

    let y_scale = 1.0 / (0.5 * fov).tan();
    let x_scale = y_scale / aspect;

    let (m22, m32) = if depth_inverted {
        if depth_infinite {
            (0.0, near)
        } else {
            (-near / (far - near), (near * far) / (far - near))
        }
    } else if depth_infinite {
        (1.0, -near)
    } else {
        (far / (far - near), -(near * far) / (far - near))
    };

    let proj = Float4x4 {
        row: [
            Float4 { x: x_scale, y: 0.0, z: 0.0, w: 0.0 },
            Float4 { x: 0.0, y: y_scale, z: 0.0, w: 0.0 },
            Float4 { x: 0.0, y: 0.0, z: m22, w: 1.0 },
            Float4 { x: 0.0, y: 0.0, z: m32, w: 0.0 },
        ],
    };

    // Analytic inverse of the perspective matrix above (avoids fp32 cofactor drift).
    let inv_m32 = if m32.abs() > 1e-12 { 1.0 / m32 } else { 0.0 };
    let inv = Float4x4 {
        row: [
            Float4 { x: 1.0 / x_scale, y: 0.0, z: 0.0, w: 0.0 },
            Float4 { x: 0.0, y: 1.0 / y_scale, z: 0.0, w: 0.0 },
            Float4 { x: 0.0, y: 0.0, z: 0.0, w: inv_m32 },
            Float4 { x: 0.0, y: 0.0, z: 1.0, w: -m22 * inv_m32 },
        ],
    };

    (proj, inv)
}

/// Row-major 4x4 matrix multiplication (`sl_matrix_helpers.h::matrixMul`).
pub fn matrix_mul(a: &Float4x4, b: &Float4x4) -> Float4x4 {
    let row = |r: Float4| Float4 {
        x: r.x * b.row[0].x + r.y * b.row[1].x + r.z * b.row[2].x + r.w * b.row[3].x,
        y: r.x * b.row[0].y + r.y * b.row[1].y + r.z * b.row[2].y + r.w * b.row[3].y,
        z: r.x * b.row[0].z + r.y * b.row[1].z + r.z * b.row[2].z + r.w * b.row[3].z,
        w: r.x * b.row[0].w + r.y * b.row[1].w + r.z * b.row[2].w + r.w * b.row[3].w,
    };
    Float4x4 {
        row: [row(a.row[0]), row(a.row[1]), row(a.row[2]), row(a.row[3])],
    }
}

/// Converts AMD FFX `ffxDispatchDescUpscale` camera/jitter/MV parameters into
/// Streamline `sl::Constants`.
///
/// Key scaling identity verified in Streamline SDK `dlssEntry.cpp:684-685`:
/// - FFX `motionVectorScale` converts MV texture values to render-pixel offsets.
/// - Streamline `sl.dlss` multiplies `consts.mvecScale * renderSize` to feed NGX.
/// - Therefore `consts.mvecScale = (ffx_mv_scale.x / render_w, ffx_mv_scale.y / render_h)`.
pub fn build_common_constants(
    jitter: [f32; 2],
    ffx_mv_scale: [f32; 2],
    render_size: [u32; 2],
    camera: [f32; 4],
    create_flags: u32,
    reset: bool,
    jitter_sign: f32,
    mv_sign: f32,
) -> Constants {
    let depth_inverted = create_flags & FFX_FLAG_DEPTH_INVERTED != 0;
    let depth_infinite = create_flags & FFX_FLAG_DEPTH_INFINITE != 0;
    let mv_jittered = create_flags & FFX_FLAG_MV_JITTER_CANCELLATION != 0;
    let mv_dilated = create_flags & FFX_FLAG_DISPLAY_RES_MV != 0;

    let rw = render_size[0].max(1) as f32;
    let rh = render_size[1].max(1) as f32;
    let aspect = rw / rh;

    // FFX swaps cameraNear/cameraFar on some inverted-depth titles; normalize to
    // finite positive near < far for Streamline's camera fields and projection.
    let c0 = camera[0];
    let c1 = camera[1];
    let (near, far) = if c0.is_finite() && c1.is_finite() && c0 > 0.0 && c1 > 0.0 {
        (c0.min(c1), c0.max(c1))
    } else if c0.is_finite() && c0 > 0.0 {
        (c0, 10_000.0)
    } else if c1.is_finite() && c1 > 0.0 {
        (c1, 10_000.0)
    } else {
        (0.1, 10_000.0)
    };
    let fov = if camera[2].is_finite() && camera[2] > 0.05 && camera[2] < 3.05 {
        camera[2]
    } else {
        std::f32::consts::FRAC_PI_3
    };

    let (proj, inv_proj) =
        build_projection_and_inverse(near, far, fov, aspect, depth_inverted, depth_infinite);

    // If the title passed (0, 0) for MV scale, fall back to pixel-space scaling.
    let mv_sx = if ffx_mv_scale[0] == 0.0 { 1.0 } else { ffx_mv_scale[0] };
    let mv_sy = if ffx_mv_scale[1] == 0.0 { 1.0 } else { ffx_mv_scale[1] };

    Constants {
        camera_view_to_clip: proj,
        clip_to_camera_view: inv_proj,
        clip_to_lens_clip: Float4x4::INVALID,
        clip_to_prev_clip: Float4x4::IDENTITY,
        prev_clip_to_clip: Float4x4::IDENTITY,
        jitter_offset: Float2 {
            x: jitter[0] * jitter_sign,
            y: jitter[1] * jitter_sign,
        },
        mvec_scale: Float2 {
            x: (mv_sx / rw) * mv_sign,
            y: (mv_sy / rh) * mv_sign,
        },
        camera_pinhole_offset: Float2 { x: 0.0, y: 0.0 },
        camera_pos: Float3 { x: 0.0, y: 0.0, z: 0.0 },
        // Orthonormal basis required by DLSS-G Auto Scene Change Detection.
        camera_up: Float3 { x: 0.0, y: 1.0, z: 0.0 },
        camera_right: Float3 { x: 1.0, y: 0.0, z: 0.0 },
        camera_fwd: Float3 { x: 0.0, y: 0.0, z: 1.0 },
        camera_near: near,
        camera_far: far,
        camera_fov: fov,
        camera_aspect_ratio: aspect,
        motion_vectors_invalid_value: INVALID_FLOAT,
        depth_inverted: sl_bool(depth_inverted),
        camera_motion_included: SL_TRUE,
        motion_vectors_3d: SL_FALSE,
        reset: sl_bool(reset),
        orthographic_projection: SL_FALSE,
        motion_vectors_dilated: sl_bool(mv_dilated),
        motion_vectors_jittered: sl_bool(mv_jittered),
        min_relative_linear_depth_object_separation: 40.0,
        ..Constants::default()
    }
}

#[derive(Clone, Copy, PartialEq)]
struct AppliedDlssKey {
    mode: u32,
    output_width: u32,
    output_height: u32,
    sharpness_bits: u32,
    pre_exposure_bits: u32,
    hdr: SlBoolean,
    auto_exposure: SlBoolean,
    preset: DlssPresetConfig,
}

static LAST_DLSS_KEY: Mutex<Option<AppliedDlssKey>> = Mutex::new(None);

/// Executes NVIDIA DLSS 4.5 Super Resolution on `cmd_list` using Streamline
/// local tags so resource states match `ffxDispatch` cleanly without colliding
/// with DLSS-G global `eValidUntilPresent` tags.
pub fn evaluate(
    rt: &StreamlineRuntime,
    cmd_list: &ID3D12GraphicsCommandList,
    params: &UpscaleFrameParams<'_>,
) -> Option<(*const FrameToken, Constants)> {
    let fns = rt.feature_fns()?;
    if !fns.dlss_supported {
        return None;
    }
    let set_options = fns.dlss_set_options?;
    let token = rt.ensure_frame_token()?;
    let cfg = config();

    let consts = build_common_constants(
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

    let mode = cfg.dlss_mode.resolve(params.render_size, params.upscale_size);
    let hdr = sl_bool(params.create_flags & FFX_FLAG_HDR != 0);
    let auto_exposure = sl_bool(
        params.exposure.is_none() || (params.create_flags & FFX_FLAG_AUTO_EXPOSURE != 0),
    );
    let pre_exposure = if params.pre_exposure.is_finite() && params.pre_exposure > 0.0 {
        params.pre_exposure
    } else {
        1.0
    };
    let sharpness = if cfg.dlss_sharpness > 0.0 {
        cfg.dlss_sharpness
    } else {
        params.sharpness.clamp(0.0, 1.0)
    };

    let key = AppliedDlssKey {
        mode,
        output_width: params.upscale_size[0],
        output_height: params.upscale_size[1],
        sharpness_bits: sharpness.to_bits(),
        pre_exposure_bits: pre_exposure.to_bits(),
        hdr,
        auto_exposure,
        preset: cfg.dlss_preset,
    };

    let need_set = {
        let last = LAST_DLSS_KEY.lock().unwrap_or_else(|e| e.into_inner());
        *last != Some(key)
    };
    if need_set {
        let mut opts = DlssOptions {
            mode,
            output_width: params.upscale_size[0],
            output_height: params.upscale_size[1],
            sharpness,
            pre_exposure,
            exposure_scale: 1.0,
            color_buffers_hdr: hdr,
            use_auto_exposure: auto_exposure,
            ..DlssOptions::default()
        };
        cfg.dlss_preset.apply(&mut opts);
        // SAFETY: `MAIN_VIEWPORT` and `opts` live for the call.
        let r = unsafe { set_options(&MAIN_VIEWPORT, &opts) };
        if !r.is_ok() {
            tracing::warn!(result = r.name(), code = r.0, "dlss: slDLSSSetOptions failed");
            return None;
        }
        *LAST_DLSS_KEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
        tracing::info!(
            render = ?params.render_size,
            upscale = ?params.upscale_size,
            mode,
            preset = ?cfg.dlss_preset,
            hdr = hdr == SL_TRUE,
            auto_exposure = auto_exposure == SL_TRUE,
            depth_inverted = consts.depth_inverted == SL_TRUE,
            mv_scale = ?[consts.mvec_scale.x, consts.mvec_scale.y],
            "dlss: DLSS 4.5 SR options configured"
        );
    }

    let read_state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32;
    let uav_state = D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32;

    let render_ext = Extent {
        top: 0,
        left: 0,
        width: params.render_size[0],
        height: params.render_size[1],
    };
    let output_ext = Extent {
        top: 0,
        left: 0,
        width: params.upscale_size[0],
        height: params.upscale_size[1],
    };
    let mv_ext = if params.create_flags & FFX_FLAG_DISPLAY_RES_MV != 0 {
        output_ext
    } else {
        render_ext
    };

    let mut color_in_res = Resource::tex2d(params.color_in.as_raw(), read_state);
    let mut color_out_res = Resource::tex2d(params.color_out.as_raw(), uav_state);
    let mut depth_res = Resource::tex2d(params.depth.as_raw(), read_state);
    let mut mvec_res = Resource::tex2d(params.mvec.as_raw(), read_state);
    let mut exposure_res = params
        .exposure
        .map(|r| Resource::tex2d(r.as_raw(), read_state));
    let mut reactive_res = params
        .reactive
        .map(|r| Resource::tex2d(r.as_raw(), read_state));
    let mut transparency_res = params
        .transparency
        .map(|r| Resource::tex2d(r.as_raw(), read_state));

    let tag_color_in = ResourceTag::new(
        &raw mut color_in_res,
        BUFFER_TYPE_SCALING_INPUT_COLOR,
        LIFECYCLE_VALID_UNTIL_EVALUATE,
        render_ext,
    );
    let tag_color_out = ResourceTag::new(
        &raw mut color_out_res,
        BUFFER_TYPE_SCALING_OUTPUT_COLOR,
        LIFECYCLE_VALID_UNTIL_EVALUATE,
        output_ext,
    );
    let tag_depth = ResourceTag::new(
        &raw mut depth_res,
        BUFFER_TYPE_DEPTH,
        LIFECYCLE_VALID_UNTIL_EVALUATE,
        render_ext,
    );
    let tag_mvec = ResourceTag::new(
        &raw mut mvec_res,
        BUFFER_TYPE_MOTION_VECTORS,
        LIFECYCLE_VALID_UNTIL_EVALUATE,
        mv_ext,
    );
    let tag_exposure = exposure_res.as_mut().map(|r| {
        ResourceTag::new(
            r,
            BUFFER_TYPE_EXPOSURE,
            LIFECYCLE_VALID_UNTIL_EVALUATE,
            Extent { top: 0, left: 0, width: 1, height: 1 },
        )
    });
    let tag_reactive = reactive_res.as_mut().map(|r| {
        ResourceTag::new(
            r,
            BUFFER_TYPE_REACTIVE_MASK_HINT,
            LIFECYCLE_VALID_UNTIL_EVALUATE,
            render_ext,
        )
    });
    let tag_transparency = transparency_res.as_mut().map(|r| {
        ResourceTag::new(
            r,
            BUFFER_TYPE_TRANSPARENCY_AND_COMPOSITION_MASK_HINT,
            LIFECYCLE_VALID_UNTIL_EVALUATE,
            render_ext,
        )
    });

    let viewport = MAIN_VIEWPORT;
    let mut inputs: Vec<*const BaseStructure> = Vec::with_capacity(8);
    inputs.push((&raw const viewport).cast());
    inputs.push((&raw const tag_color_in).cast());
    inputs.push((&raw const tag_color_out).cast());
    inputs.push((&raw const tag_depth).cast());
    inputs.push((&raw const tag_mvec).cast());
    if let Some(t) = &tag_exposure {
        inputs.push((t as *const ResourceTag).cast());
    }
    if let Some(t) = &tag_reactive {
        inputs.push((t as *const ResourceTag).cast());
    }
    if let Some(t) = &tag_transparency {
        inputs.push((t as *const ResourceTag).cast());
    }

    // SAFETY: `inputs` and every pointed-to `ResourceTag` / `Resource` live through
    // `slEvaluateFeature`; `cmd_list` is an open host D3D12 graphics command list.
    let eval_res = unsafe {
        (rt.api.sl_evaluate_feature)(
            FEATURE_DLSS,
            token,
            inputs.as_ptr(),
            inputs.len() as u32,
            cmd_list.as_raw(),
        )
    };
    if !eval_res.is_ok() {
        static EVAL_ERR: std::sync::Once = std::sync::Once::new();
        EVAL_ERR.call_once(|| {
            tracing::error!(
                result = eval_res.name(),
                code = eval_res.0,
                "dlss: slEvaluateFeature(kFeatureDLSS) failed"
            );
        });
        return None;
    }

    static FIRST_EVAL: std::sync::Once = std::sync::Once::new();
    FIRST_EVAL.call_once(|| {
        tracing::info!(
            render = ?params.render_size,
            upscale = ?params.upscale_size,
            "dlss: first DLSS 4.5 SR upscale recorded on the host GPU"
        );
    });

    Some((token, consts))
}

/// Frees DLSS-SR resources for the main viewport on context destruction.
pub fn free_resources(rt: &StreamlineRuntime) {
    *LAST_DLSS_KEY.lock().unwrap_or_else(|e| e.into_inner()) = None;
    // SAFETY: releases viewport resources cleanly.
    let _ = unsafe { (rt.api.sl_free_resources)(FEATURE_DLSS, &MAIN_VIEWPORT) };
}

/// Recommended Halton jitter phase count for both FSR and NVIDIA DLSS
/// (`ceil(8 * (display_width / render_width)^2)`).
pub fn jitter_phase_count(render_width: u32, display_width: u32) -> i32 {
    if render_width == 0 || display_width == 0 {
        return 8;
    }
    let ratio = display_width as f32 / render_width as f32;
    (8.0 * ratio * ratio).ceil().clamp(8.0, 64.0) as i32
}

/// Halton sequence value in `[0, 1)` for 1-based index and prime base.
fn halton(mut index: i32, base: i32) -> f32 {
    let mut f = 1.0_f32;
    let mut r = 0.0_f32;
    let b = base as f32;
    while index > 0 {
        f /= b;
        r += f * (index % base) as f32;
        index /= base;
    }
    r
}

/// Subpixel Halton(2, 3) jitter offset centered in `[-0.5, 0.5]`.
pub fn halton_jitter_offset(index: i32, phase_count: i32) -> (f32, f32) {
    let phase = phase_count.max(1);
    let slot = index.rem_euclid(phase) + 1;
    (halton(slot, 2) - 0.5, halton(slot, 3) - 0.5)
}

/// Upscale ratio matching `FfxApiUpscaleQualityMode` (`0..=4`) for standalone
/// `ffxQuery` responses when `VICTORY_FFX_DIR` is unset.
pub fn upscale_ratio_for_quality_mode(quality_mode: u32) -> f32 {
    match quality_mode {
        0 => 1.0, // NativeAA / DLAA
        1 => 1.5, // Quality (0.6667)
        2 => 1.7, // Balanced (0.588)
        3 => 2.0, // Performance (0.50)
        4 => 3.0, // UltraPerformance (0.333)
        _ => 1.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn projection_matrix_and_inverse_multiply_to_identity() {
        for (inverted, infinite) in [(false, false), (true, false), (true, true), (false, true)] {
            let (proj, inv) =
                build_projection_and_inverse(0.15, 5000.0, 1.0472, 16.0 / 9.0, inverted, infinite);
            let prod = matrix_mul(&proj, &inv);
            for r in 0..4 {
                let row = [prod.row[r].x, prod.row[r].y, prod.row[r].z, prod.row[r].w];
                for c in 0..4 {
                    let want = if r == c { 1.0 } else { 0.0 };
                    assert!(
                        approx_eq(row[c], want, 1e-4),
                        "inv={inverted} inf={infinite} [{r},{c}] got={} want={want}",
                        row[c]
                    );
                }
            }
        }
    }

    #[test]
    fn common_constants_scale_motion_vectors_and_jitter_correctly() {
        // Pixel-space MVs (FFX scale = 1.0) -> Streamline mvecScale = 1 / render_size.
        let c = build_common_constants(
            [0.25, -0.375],
            [1.0, -1.0],
            [2560, 1440],
            [0.15, 5000.0, 1.0, 1.0],
            FFX_FLAG_HDR | FFX_FLAG_DEPTH_INVERTED | FFX_FLAG_MV_JITTER_CANCELLATION,
            true,
            -1.0,
            1.0,
        );
        assert!(approx_eq(c.mvec_scale.x, 1.0 / 2560.0, 1e-7));
        assert!(approx_eq(c.mvec_scale.y, -1.0 / 1440.0, 1e-7));
        assert!(approx_eq(c.jitter_offset.x, -0.25, 1e-7));
        assert!(approx_eq(c.jitter_offset.y, 0.375, 1e-7));
        assert_eq!(c.depth_inverted, SL_TRUE);
        assert_eq!(c.motion_vectors_jittered, SL_TRUE);
        assert_eq!(c.camera_motion_included, SL_TRUE);
        assert_eq!(c.reset, SL_TRUE);
        assert!(approx_eq(c.camera_aspect_ratio, 2560.0 / 1440.0, 1e-5));

        // UV-space MVs (FFX scale = (2560, 1440)) -> Streamline mvecScale = (1, 1).
        let c2 = build_common_constants(
            [0.1, 0.2],
            [2560.0, 1440.0],
            [2560, 1440],
            [5000.0, 0.15, 1.0, 1.0],
            FFX_FLAG_DEPTH_INVERTED,
            false,
            1.0,
            -1.0,
        );
        assert!(approx_eq(c2.mvec_scale.x, -1.0, 1e-6));
        assert!(approx_eq(c2.mvec_scale.y, -1.0, 1e-6));
        assert!(approx_eq(c2.camera_near, 0.15, 1e-6));
        assert!(approx_eq(c2.camera_far, 5000.0, 1e-4));
    }

    #[test]
    fn dlss_mode_inference_and_preset_defaults() {
        assert_eq!(infer_dlss_mode([2560, 1440], [3840, 2160]), DLSS_MODE_MAX_QUALITY);
        assert_eq!(infer_dlss_mode([2227, 1253], [3840, 2160]), DLSS_MODE_BALANCED);
        assert_eq!(infer_dlss_mode([1920, 1080], [3840, 2160]), DLSS_MODE_MAX_PERFORMANCE);
        assert_eq!(infer_dlss_mode([1280, 720], [3840, 2160]), DLSS_MODE_ULTRA_PERFORMANCE);
        assert_eq!(infer_dlss_mode([3840, 2160], [3840, 2160]), DLSS_MODE_DLAA);

        assert_eq!(DlssModeConfig::parse(Some("quality")), DlssModeConfig::Quality);
        assert_eq!(DlssModeConfig::parse(Some("dlaa")), DlssModeConfig::Dlaa);
        assert_eq!(DlssModeConfig::parse(None), DlssModeConfig::Auto);

        let mut opts = DlssOptions::default();
        DlssPresetConfig::Auto.apply(&mut opts);
        assert_eq!(opts.quality_preset, DLSS_PRESET_K);
        assert_eq!(opts.performance_preset, DLSS_PRESET_M);
        assert_eq!(opts.ultra_performance_preset, DLSS_PRESET_L);

        DlssPresetConfig::parse(Some("m")).apply(&mut opts);
        assert_eq!(opts.quality_preset, DLSS_PRESET_M);
        assert_eq!(opts.dlaa_preset, DLSS_PRESET_M);
    }

    #[test]
    fn jitter_phase_count_and_halton_offsets_match_spec() {
        assert_eq!(jitter_phase_count(2560, 3840), 18);
        assert_eq!(jitter_phase_count(1920, 3840), 32);
        assert_eq!(jitter_phase_count(3840, 3840), 8);
        let (jx, jy) = halton_jitter_offset(0, 18);
        assert!(approx_eq(jx, 0.0, 1e-6));
        assert!(approx_eq(jy, (1.0 / 3.0) - 0.5, 1e-6));
    }
}

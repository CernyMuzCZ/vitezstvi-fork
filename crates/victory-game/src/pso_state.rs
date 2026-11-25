//! The two state blocks of a precompiled graphics pipeline
//! ([`crate::pso_cache`]), decoded into what a D3D12 graphics pipeline
//! description holds.
//!
//! - Block 0 (380 to 520 bytes) is the console driver's register image of
//!   the pipeline: GFX10 context registers in an order of the driver's own,
//!   the same for every record from the dword `0x060201bc` on (dword 29 of
//!   a vertex-shader pipeline, dword 50 of a mesh one; dword 1 is 4 or 6).
//!   Blend, depth/stencil and rasteriser state are read from it.
//! - Block 1 (80 to 340 bytes) is the driver's copy of the description's
//!   other fields: render target and depth formats, their count, the
//!   topology type, sample mask and count, the stream output (always
//!   empty) and the input layout with its semantic names.

/// One render target's blend state, as `D3D12_RENDER_TARGET_BLEND_DESC`
/// holds it (D3D12 enumeration values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetBlend {
    pub blend: bool,
    pub logic_op_enable: bool,
    pub src: u32,
    pub dst: u32,
    pub op: u32,
    pub src_alpha: u32,
    pub dst_alpha: u32,
    pub op_alpha: u32,
    pub logic_op: u32,
    pub write_mask: u8,
}

/// `D3D12_DEPTH_STENCILOP_DESC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StencilFace {
    pub fail: u32,
    pub depth_fail: u32,
    pub pass: u32,
    pub func: u32,
}

/// `D3D12_INPUT_ELEMENT_DESC` (format: the console's DXGI number).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputElement {
    pub name: String,
    pub index: u32,
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub class: u32,
    pub step_rate: u32,
}

/// A graphics pipeline description without its shaders and root
/// signature. Enumerations are D3D12's values; formats the console's
/// DXGI numbers (as the title passes them in a stream).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphicsDesc {
    /// The record's shader stages feed a mesh shader (no input layout).
    pub mesh: bool,
    pub alpha_to_coverage: bool,
    pub independent_blend: bool,
    pub targets_blend: [TargetBlend; 8],
    pub sample_mask: u32,
    pub fill_mode: u32,
    pub cull_mode: u32,
    pub front_ccw: bool,
    pub depth_bias: i32,
    pub depth_bias_clamp: f32,
    pub slope_scaled_depth_bias: f32,
    pub depth_clip: bool,
    pub depth_enable: bool,
    /// `D3D12_DEPTH_WRITE_MASK`: 0 or 1.
    pub depth_write: u32,
    pub depth_func: u32,
    pub stencil_enable: bool,
    pub stencil_read_mask: u8,
    pub stencil_write_mask: u8,
    pub front: StencilFace,
    pub back: StencilFace,
    pub depth_bounds: bool,
    pub inputs: Vec<InputElement>,
    pub strip_cut: u32,
    pub topology: u32,
    pub num_targets: u32,
    pub target_formats: [u32; 8],
    pub depth_format: u32,
    pub sample_count: u32,
    pub sample_quality: u32,
}

/// Why a pair of state blocks does not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The register block is too short or has no anchor where expected.
    Registers,
    /// A register field holds a value without a D3D12 equivalent.
    Value(&'static str, u32),
    /// The description block does not hold what its sizes say.
    Desc(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Registers => f.write_str("register block of unknown layout"),
            Error::Value(what, v) => write!(f, "{what} {v:#x} has no D3D12 value"),
            Error::Desc(what) => write!(f, "description block: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// The dword every register block has where its fixed part starts.
pub const ANCHOR: u32 = 0x0602_01bc;

/// Register block dwords, from [`ANCHOR`].
mod at {
    pub const CB_COLOR_CONTROL: usize = 2;
    pub const PA_CL_CLIP_CNTL: usize = 10;
    pub const PA_SU_SC_MODE_CNTL: usize = 11;
    pub const POLY_OFFSET_CLAMP: usize = 14;
    pub const POLY_OFFSET_SCALE: usize = 15;
    pub const POLY_OFFSET_OFFSET: usize = 16;
    pub const DB_DEPTH_CONTROL: usize = 17;
    pub const DB_STENCIL_CONTROL: usize = 18;
    pub const DB_STENCILREFMASK: usize = 19;
    /// `CB_TARGET_MASK` as the description gives it: 4 bits per target,
    /// all eight.
    pub const CB_TARGET_MASK: usize = 22;
    pub const CB_BLEND0_CONTROL: usize = 23;
    pub const SX_PS_DOWNCONVERT: usize = 32;
    /// Dwords from the anchor on that a block has at least.
    pub const END: usize = 46;
}

/// Where the anchor is: dword 29 of a vertex-shader pipeline's block
/// (dword 1 = 4), 50 of a mesh one's (dword 1 = 6).
fn anchor(w: &[u32]) -> Option<(usize, bool)> {
    let (a, mesh) = match w.get(1)? {
        4 => (29, false),
        6 => (50, true),
        _ => return None,
    };
    (w.get(a) == Some(&ANCHOR) && w.len() >= a + at::END).then_some((a, mesh))
}

fn dwords(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes")))
        .collect()
}

fn bits(v: u32, lo: u32, n: u32) -> u32 {
    v >> lo & ((1 << n) - 1)
}

/// `V_028780_BLEND_*` to `D3D12_BLEND` (for the alpha factors, the colour
/// ones read as their alpha ones, which is what they are there).
fn blend_factor(hw: u32, alpha: bool) -> Result<u32, Error> {
    Ok(match (hw, alpha) {
        (0..=10, false) => hw + 1,
        (2, true) => 5,
        (3, true) => 6,
        (8, true) => 7,
        (9, true) => 8,
        (0 | 1 | 4..=7 | 10, true) => hw + 1,
        (13, _) => 14,
        (14, _) => 15,
        (15, false) => 16,
        (16, false) => 17,
        (15, true) | (17, _) => 18,
        (16, true) | (18, _) => 19,
        (19, _) => 20,
        (20, _) => 21,
        _ => return Err(Error::Value("blend factor", hw)),
    })
}

/// `COMB_FCN` to `D3D12_BLEND_OP`.
fn blend_op(hw: u32) -> Result<u32, Error> {
    Ok(match hw {
        0 => 1, // add
        1 => 2, // subtract
        4 => 3, // reverse subtract
        2 => 4, // min
        3 => 5, // max
        _ => return Err(Error::Value("blend function", hw)),
    })
}

/// `CB_COLOR_CONTROL.ROP3` to `D3D12_LOGIC_OP` (`None`: copy, no logic op).
fn logic_op(rop3: u32) -> Result<Option<u32>, Error> {
    Ok(Some(match rop3 {
        0xcc => return Ok(None),
        0x00 => 0,  // clear
        0xff => 1,  // set
        0x33 => 3,  // copy inverted
        0xaa => 4,  // noop
        0x55 => 5,  // invert
        0x88 => 6,  // and
        0x77 => 7,  // nand
        0xee => 8,  // or
        0x11 => 9,  // nor
        0x66 => 10, // xor
        0x99 => 11, // equiv
        0x44 => 12, // and reverse
        0x22 => 13, // and inverted
        0xdd => 14, // or reverse
        0xbb => 15, // or inverted
        _ => return Err(Error::Value("ROP3", rop3)),
    }))
}

/// `STENCIL_*` to `D3D12_STENCIL_OP` (both replace forms are replace; the
/// clamped and wrapped add and subtract of the operation value 1 are
/// D3D12's increments and decrements).
fn stencil_op(hw: u32) -> Result<u32, Error> {
    Ok(match hw {
        0 => 1,
        1 => 2,
        3 | 4 => 3,
        5 => 4,
        6 => 5,
        7 => 6,
        8 => 7,
        9 => 8,
        _ => return Err(Error::Value("stencil operation", hw)),
    })
}

fn f32_of(v: u32) -> f32 {
    f32::from_bits(v)
}

/// The register image's part of the description.
fn registers(block: &[u8], d: &mut GraphicsDesc) -> Result<(), Error> {
    let w = dwords(block);
    let (a, mesh) = anchor(&w).ok_or(Error::Registers)?;
    let r = |i: usize| w[a + i];
    d.mesh = mesh;

    // Blend.
    let rop = logic_op(bits(r(at::CB_COLOR_CONTROL), 16, 8))?;
    let masks = r(at::CB_TARGET_MASK);
    for (t, out) in d.targets_blend.iter_mut().enumerate() {
        let b = r(at::CB_BLEND0_CONTROL + t);
        let write_mask = bits(masks, 4 * t as u32, 4) as u8;
        *out = if b >> 30 & 1 == 1 {
            TargetBlend {
                blend: true,
                src: blend_factor(bits(b, 0, 5), false)?,
                op: blend_op(bits(b, 5, 3))?,
                dst: blend_factor(bits(b, 8, 5), false)?,
                src_alpha: blend_factor(bits(b, 16, 5), true)?,
                op_alpha: blend_op(bits(b, 21, 3))?,
                dst_alpha: blend_factor(bits(b, 24, 5), true)?,
                write_mask,
                ..DEFAULT_BLEND
            }
        } else {
            TargetBlend {
                write_mask,
                ..DEFAULT_BLEND
            }
        };
        if let Some(op) = rop {
            out.logic_op_enable = true;
            out.logic_op = op;
        }
    }
    d.independent_blend = d.targets_blend.iter().any(|t| *t != d.targets_blend[0]);

    // Rasteriser.
    let mode = r(at::PA_SU_SC_MODE_CNTL);
    d.cull_mode = match bits(mode, 0, 2) {
        0 => 1,     // none
        1 => 2,     // front
        2 | 3 => 3, // back (both culled: nothing drawn either way)
        _ => unreachable!(),
    };
    d.front_ccw = bits(mode, 2, 1) == 0;
    d.fill_mode = match (bits(mode, 3, 2), bits(mode, 5, 3)) {
        (0, _) | (_, 2) => 3, // solid
        (_, 1) => 2,          // wireframe
        (m, p) => return Err(Error::Value("polygon mode", m << 3 | p)),
    };
    d.depth_bias_clamp = f32_of(r(at::POLY_OFFSET_CLAMP));
    // The hardware's slope factor is in 1/16 pixel units.
    d.slope_scaled_depth_bias = f32_of(r(at::POLY_OFFSET_SCALE)) / 16.0;
    d.depth_bias = f32_of(r(at::POLY_OFFSET_OFFSET)).round() as i32;
    let clip = r(at::PA_CL_CLIP_CNTL);
    d.depth_clip = bits(clip, 26, 2) == 0;

    // Depth and stencil.
    let dc = r(at::DB_DEPTH_CONTROL);
    let sc = r(at::DB_STENCIL_CONTROL);
    let refmask = r(at::DB_STENCILREFMASK);
    d.stencil_enable = bits(dc, 0, 1) == 1;
    d.depth_enable = bits(dc, 1, 1) == 1;
    d.depth_write = bits(dc, 2, 1);
    d.depth_bounds = bits(dc, 3, 1) == 1;
    d.depth_func = bits(dc, 4, 3) + 1;
    let back_func = if bits(dc, 7, 1) == 1 {
        bits(dc, 20, 3)
    } else {
        bits(dc, 8, 3)
    };
    let face = |at: u32, func: u32| -> Result<StencilFace, Error> {
        Ok(StencilFace {
            fail: stencil_op(bits(sc, at, 4))?,
            pass: stencil_op(bits(sc, at + 4, 4))?,
            depth_fail: stencil_op(bits(sc, at + 8, 4))?,
            func: func + 1,
        })
    };
    d.front = face(0, bits(dc, 8, 3))?;
    d.back = face(12, back_func)?;
    d.stencil_read_mask = bits(refmask, 8, 8) as u8;
    d.stencil_write_mask = bits(refmask, 16, 8) as u8;
    Ok(())
}

const DEFAULT_BLEND: TargetBlend = TargetBlend {
    blend: false,
    logic_op_enable: false,
    src: 2,
    dst: 1,
    op: 1,
    src_alpha: 2,
    dst_alpha: 1,
    op_alpha: 1,
    logic_op: 4,
    write_mask: 0xf,
};

/// The console's rectangle-list topology type, as the title's pipeline
/// streams name it.
pub const RECTANGLES: u32 = 19;

/// The topology byte at +0x26 as (topology type, strip cut value). 0x13
/// is the console's rectangle list, not a triangle type with a strip cut:
/// taken apart as such, its pipelines lost the geometry stage that
/// completes each rectangle and drew half of each one.
fn topology(byte: u32) -> (u32, u32) {
    if byte == RECTANGLES {
        (RECTANGLES, 0)
    } else {
        (bits(byte, 0, 4), bits(byte, 4, 2))
    }
}

/// The description block's part.
///
/// ```text
/// +0x00 u32 hash, u32 0x1000, u32 0, u32 has input layout
/// +0x10 u8[8] render target formats
/// +0x18 u32 0, u32 sample mask, u32 0
/// +0x24 u8 depth format, u8 render targets, u8 topology type (low 4 bits)
///       and strip cut value (bits 4-5), u8 0
/// +0x28 u32 sample count (low byte), u32 x3
/// +0x38 u32 input layout bytes (from +0x4c), u32 stream output bytes (12)
/// +0x40 stream output: u32 entries, strides, rasterized stream (all 0)
/// +0x4c u32 input elements, then each: u32 name bytes (with the NUL),
///       the name, u32 index, format, slot, offset, class, step rate
/// ```
fn description(block: &[u8], d: &mut GraphicsDesc) -> Result<(), Error> {
    let u = |at: usize| -> Result<u32, Error> {
        block
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
            .ok_or(Error::Desc("too short"))
    };
    if block.len() < 0x50 {
        return Err(Error::Desc("too short"));
    }
    for (t, f) in d.target_formats.iter_mut().enumerate() {
        *f = u32::from(block[0x10 + t]);
    }
    d.sample_mask = u(0x1c)?;
    let w = u(0x24)?;
    d.depth_format = bits(w, 0, 8);
    d.num_targets = bits(w, 8, 8);
    if d.num_targets > 8 {
        return Err(Error::Desc("more than eight render targets"));
    }
    (d.topology, d.strip_cut) = topology(bits(w, 16, 8));
    d.sample_count = bits(u(0x28)?, 0, 8).max(1);
    d.sample_quality = 0;
    if u(0x3c)? != 12 || u(0x40)? != 0 || u(0x44)? != 0 {
        return Err(Error::Desc("a stream output"));
    }
    let layout = u(0x38)? as usize;
    let n = u(0x4c)? as usize;
    let mut at = 0x50;
    for _ in 0..n {
        let len = u(at)? as usize;
        let name = block
            .get(at + 4..at + 4 + len)
            .ok_or(Error::Desc("input element name"))?;
        let name = name
            .strip_suffix(&[0])
            .ok_or(Error::Desc("input element name without its end"))?;
        let name = std::str::from_utf8(name).map_err(|_| Error::Desc("input element name"))?;
        at += 4 + len;
        d.inputs.push(InputElement {
            name: name.to_owned(),
            index: u(at)?,
            format: u(at + 4)?,
            slot: u(at + 8)?,
            offset: u(at + 12)?,
            class: u(at + 16)?,
            step_rate: u(at + 20)?,
        });
        at += 24;
    }
    if at - 0x4c != layout {
        return Err(Error::Desc("input layout size"));
    }
    Ok(())
}

/// A record's two state blocks, decoded.
pub fn decode(registers_block: &[u8], description_block: &[u8]) -> Result<GraphicsDesc, Error> {
    let mut d = GraphicsDesc {
        mesh: false,
        alpha_to_coverage: false,
        independent_blend: false,
        targets_blend: [DEFAULT_BLEND; 8],
        sample_mask: u32::MAX,
        fill_mode: 3,
        cull_mode: 3,
        front_ccw: false,
        depth_bias: 0,
        depth_bias_clamp: 0.0,
        slope_scaled_depth_bias: 0.0,
        depth_clip: true,
        depth_enable: false,
        depth_write: 0,
        depth_func: 8,
        stencil_enable: false,
        stencil_read_mask: 0xff,
        stencil_write_mask: 0xff,
        front: StencilFace {
            fail: 1,
            depth_fail: 1,
            pass: 1,
            func: 8,
        },
        back: StencilFace {
            fail: 1,
            depth_fail: 1,
            pass: 1,
            func: 8,
        },
        depth_bounds: false,
        inputs: Vec::new(),
        strip_cut: 0,
        topology: 3,
        num_targets: 0,
        target_formats: [0; 8],
        depth_format: 0,
        sample_count: 1,
        sample_quality: 0,
    };
    registers(registers_block, &mut d)?;
    description(description_block, &mut d)?;
    if d.mesh && !d.inputs.is_empty() {
        return Err(Error::Desc("an input layout for a mesh pipeline"));
    }
    Ok(d)
}

/// `SX_PS_DOWNCONVERT`'s format for a render target format of the console
/// (`None`: not one seen in the title's records). For [`consistent`].
fn downconvert(format: u32) -> Option<&'static [u32]> {
    Some(match format {
        28 | 29 | 49 | 61 | 62 | 87 | 51 => &[0, 5],
        24 => &[4],
        26 => &[0, 3],
        34 | 35 => &[0, 9],
        41 | 42 => &[1],
        54 | 56 | 57 => &[0, 10],
        2 | 3 | 10 | 17 | 65 | 86 | 115 => &[0, 5],
        67 => &[11],
        _ => return None,
    })
}

/// Whether the register image agrees with the description block on the
/// render target formats (`SX_PS_DOWNCONVERT`, derived by the driver from
/// them). A check of the decoding, not needed to use it.
pub fn consistent(registers_block: &[u8], d: &GraphicsDesc) -> bool {
    let w = dwords(registers_block);
    let Some((a, _)) = anchor(&w) else {
        return false;
    };
    let down = w[a + at::SX_PS_DOWNCONVERT];
    (0..d.num_targets as usize).all(|t| {
        downconvert(d.target_formats[t]).is_none_or(|ok| ok.contains(&bits(down, 4 * t as u32, 4)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes a synthetic GFX10 register block (vertex or mesh pipeline).
    struct RegBuilder {
        anchor: usize,
        words: Vec<u32>,
    }

    impl RegBuilder {
        fn new(mesh: bool) -> Self {
            let anchor = if mesh { 50 } else { 29 };
            let mut words = vec![0u32; anchor + at::END + 4];
            words[1] = if mesh { 6 } else { 4 };
            words[anchor] = ANCHOR;
            // Default ROP3 = 0xcc (copy, no logic op), all targets write mask 0xf.
            words[anchor + at::CB_COLOR_CONTROL] = 0xcc << 16;
            words[anchor + at::CB_TARGET_MASK] = 0xffff_ffff;
            Self { anchor, words }
        }

        fn set(&mut self, reg: usize, value: u32) -> &mut Self {
            self.words[self.anchor + reg] = value;
            self
        }

        fn blend_word(
            src: u32,
            op: u32,
            dst: u32,
            src_a: u32,
            op_a: u32,
            dst_a: u32,
        ) -> u32 {
            (1 << 30)
                | (src & 0x1f)
                | ((op & 7) << 5)
                | ((dst & 0x1f) << 8)
                | ((src_a & 0x1f) << 16)
                | ((op_a & 7) << 21)
                | ((dst_a & 0x1f) << 24)
        }

        fn build(&self) -> Vec<u8> {
            self.words.iter().flat_map(|w| w.to_le_bytes()).collect()
        }
    }

    /// Encodes a synthetic description block matching the layout documented in `description`.
    fn desc_block(
        rt_formats: &[u8],
        depth_format: u8,
        topology_byte: u8,
        sample_mask: u32,
        sample_count: u8,
        inputs: &[(&str, u32, u32, u32, u32, u32, u32)],
    ) -> Vec<u8> {
        let mut out = vec![0u8; 0x50];
        let put = |b: &mut [u8], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut out, 0x04, 0x1000);
        put(&mut out, 0x0c, u32::from(!inputs.is_empty()));
        for (i, &f) in rt_formats.iter().take(8).enumerate() {
            out[0x10 + i] = f;
        }
        put(&mut out, 0x1c, sample_mask);
        let packed = u32::from(depth_format)
            | (u32::from(rt_formats.len() as u8) << 8)
            | (u32::from(topology_byte) << 16);
        put(&mut out, 0x24, packed);
        put(&mut out, 0x28, u32::from(sample_count));
        put(&mut out, 0x3c, 12); // empty stream output (12 bytes of zeros at 0x40..0x4c)
        put(&mut out, 0x4c, inputs.len() as u32);
        for &(name, index, format, slot, offset, class, step_rate) in inputs {
            let name_len = (name.len() + 1) as u32;
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            for v in [index, format, slot, offset, class, step_rate] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        let layout_bytes = (out.len() - 0x4c) as u32;
        put(&mut out, 0x38, layout_bytes);
        out
    }

    #[test]
    fn the_rectangle_list_byte_is_a_topology_not_a_strip_cut() {
        assert_eq!(topology(0x13), (RECTANGLES, 0));
        assert_eq!(topology(3), (3, 0));
        assert_eq!(topology(0x23), (3, 2));
    }

    #[test]
    fn a_ui_pipeline_blends_with_source_alpha() {
        let mut regs = RegBuilder::new(false);
        // hw blend 4 = SRC_ALPHA (D3D12 5), hw 5 = INV_SRC_ALPHA (D3D12 6), op 0 = ADD (1).
        let blend = RegBuilder::blend_word(4, 0, 5, 4, 0, 5);
        for t in 0..8 {
            regs.set(at::CB_BLEND0_CONTROL + t, blend);
        }
        // Cull none (0), front CCW (bit 2 = 0), fill solid.
        regs.set(at::PA_SU_SC_MODE_CNTL, 0);
        // Downconvert 0 (8-bit unorm) for target 0 (format 87).
        regs.set(at::SX_PS_DOWNCONVERT, 0);
        let a = regs.build();
        let b = desc_block(
            &[87],
            0,
            3,
            u32::MAX,
            1,
            &[
                ("POSITION", 0, 6, 0, 0, 0, 0),
                ("TEXCOORD", 0, 16, 0, 12, 0, 0),
            ],
        );
        let d = decode(&a, &b).unwrap();
        assert!(consistent(&a, &d));
        assert!(!d.mesh);
        assert!(!d.depth_enable && !d.stencil_enable);
        assert_eq!(
            (d.cull_mode, d.front_ccw, d.fill_mode, d.depth_clip),
            (1, true, 3, true)
        );
        let t = d.targets_blend[0];
        assert!(t.blend && !t.logic_op_enable);
        assert_eq!((t.src, t.dst, t.op, t.write_mask), (5, 6, 1, 0xf));
        assert!(!d.independent_blend);
        assert_eq!((d.num_targets, d.target_formats[0], d.depth_format), (1, 87, 0));
        assert_eq!(
            (d.topology, d.strip_cut, d.sample_count, d.sample_mask),
            (3, 0, 1, u32::MAX)
        );
        assert_eq!(d.inputs.len(), 2);
        assert_eq!(d.inputs[0].name, "POSITION");
        assert_eq!(d.inputs[1].name, "TEXCOORD");
    }

    #[test]
    fn a_stencil_tested_additive_pipeline() {
        let mut regs = RegBuilder::new(false);
        // hw blend 1 = ONE (D3D12 2), op 0 = ADD (1).
        let blend = RegBuilder::blend_word(1, 0, 1, 1, 0, 1);
        for t in 0..8 {
            regs.set(at::CB_BLEND0_CONTROL + t, blend);
        }
        // Cull back (2), front CCW (bit 2 = 0).
        regs.set(at::PA_SU_SC_MODE_CNTL, 2);
        // Stencil enable (bit 0), depth enable (bit 1), depth write 0 (bit 2 = 0),
        // depth bounds (bit 3 = 1), depth func 6+1=7 (bits 4..7), front func 2+1=3 (bits 8..11).
        regs.set(at::DB_DEPTH_CONTROL, 1 | (1 << 1) | (1 << 3) | (6 << 4) | (2 << 8));
        // Stencil ops: all 0 -> D3D12_STENCIL_OP_KEEP (1).
        regs.set(at::DB_STENCIL_CONTROL, 0);
        regs.set(at::DB_STENCILREFMASK, 0x18 << 8);
        let a = regs.build();
        let b = desc_block(&[10], 20, 3, u32::MAX, 1, &[("POSITION", 0, 6, 0, 0, 0, 0)]);
        let d = decode(&a, &b).unwrap();
        assert!(consistent(&a, &d));
        assert!(d.depth_enable && d.stencil_enable && d.depth_bounds);
        assert_eq!((d.depth_write, d.depth_func), (0, 7));
        assert_eq!((d.stencil_read_mask, d.stencil_write_mask), (0x18, 0));
        let face = StencilFace {
            fail: 1,
            depth_fail: 1,
            pass: 1,
            func: 3,
        };
        assert_eq!((d.front, d.back), (face, face));
        assert_eq!((d.cull_mode, d.front_ccw), (3, true));
        let t = d.targets_blend[0];
        assert_eq!((t.blend, t.src, t.dst, t.op, t.write_mask), (true, 2, 2, 1, 0xf));
        assert_eq!((d.num_targets, d.target_formats[0], d.depth_format), (1, 10, 20));
    }

    #[test]
    fn a_three_target_pass_writing_stencil_and_independent_masks() {
        let mut regs = RegBuilder::new(false);
        regs.set(at::CB_TARGET_MASK, 0x0000_03ff); // masks: 0xf, 0xf, 0x3, 0...
        // Depth enable + write + func 7; stencil enable + separate back func (bit 7 = 1, back func 4+1=5).
        regs.set(
            at::DB_DEPTH_CONTROL,
            1 | (1 << 1) | (1 << 2) | (6 << 4) | (1 << 7) | (7 << 8) | (4 << 20),
        );
        // Front pass = hw 3 (REPLACE -> 3); back fail = hw 1 (ZERO -> 2), pass = hw 5 (INCR_SAT -> 4).
        regs.set(at::DB_STENCIL_CONTROL, (3 << 4) | (1 << 12) | (5 << 16));
        regs.set(at::DB_STENCILREFMASK, (0xff << 8) | (0xff << 16));
        regs.set(at::SX_PS_DOWNCONVERT, (5 << 4) | (9 << 8));
        let a = regs.build();
        let b = desc_block(&[29, 28, 34], 20, 3, u32::MAX, 1, &[]);
        let d = decode(&a, &b).unwrap();
        assert!(consistent(&a, &d));
        assert_eq!((d.depth_write, d.depth_func), (1, 7));
        assert_eq!(
            d.front,
            StencilFace {
                fail: 1,
                depth_fail: 1,
                pass: 3,
                func: 8
            }
        );
        assert_eq!(
            d.back,
            StencilFace {
                fail: 2,
                depth_fail: 1,
                pass: 4,
                func: 5
            }
        );
        assert_eq!((d.num_targets, &d.target_formats[..3], d.depth_format), (3, &[29, 28, 34][..], 20));
        assert!(d.targets_blend.iter().all(|t| !t.blend));
        let masks: Vec<u8> = d.targets_blend.iter().map(|t| t.write_mask).collect();
        assert_eq!(&masks[..4], &[0xf, 0xf, 0x3, 0x0]);
        assert!(d.independent_blend);
    }

    #[test]
    fn a_mesh_pipeline_with_dual_source_blending_and_depth_bias() {
        let mut regs = RegBuilder::new(true);
        // src=ONE (1->2), dst=INV_SRC1_COLOR (16->17), src_a=ONE (1->2), dst_a=INV_SRC1_ALPHA (18->19).
        let blend = RegBuilder::blend_word(1, 0, 16, 1, 0, 18);
        for t in 0..8 {
            regs.set(at::CB_BLEND0_CONTROL + t, blend);
        }
        regs.set(at::CB_TARGET_MASK, 0x0000_000f);
        regs.set(at::POLY_OFFSET_CLAMP, 0.0f32.to_bits());
        regs.set(at::POLY_OFFSET_SCALE, 40.0f32.to_bits());
        regs.set(at::POLY_OFFSET_OFFSET, 5.0f32.to_bits());
        let a = regs.build();
        let b = desc_block(&[49], 0, 3, u32::MAX, 1, &[]);
        let d = decode(&a, &b).unwrap();
        assert!(consistent(&a, &d));
        assert!(d.mesh && d.inputs.is_empty());
        let t = d.targets_blend[0];
        assert_eq!((t.src, t.dst, t.op, t.src_alpha, t.dst_alpha), (2, 17, 1, 2, 19));
        assert_eq!(
            (d.depth_bias, d.slope_scaled_depth_bias, d.depth_bias_clamp),
            (5, 2.5, 0.0)
        );
        assert_eq!((d.num_targets, d.target_formats[0]), (1, 49));
    }

    #[test]
    fn values_without_a_d3d12_meaning_are_refused() {
        let mut regs = RegBuilder::new(false);
        // Blend factor 11 (BOTH_SRC_ALPHA) has no D3D12 equivalent.
        regs.set(at::CB_BLEND0_CONTROL, RegBuilder::blend_word(11, 0, 5, 4, 0, 5));
        let bad_blend = regs.build();
        let good_desc = desc_block(&[87], 0, 3, u32::MAX, 1, &[("POSITION", 0, 6, 0, 0, 0, 0)]);
        assert_eq!(
            decode(&bad_blend, &good_desc),
            Err(Error::Value("blend factor", 11))
        );
        // No anchor / too short.
        assert_eq!(decode(&bad_blend[..0x60], &good_desc), Err(Error::Registers));
        // A description whose layout size disagrees.
        let good_regs = RegBuilder::new(false).build();
        let mut bad_desc = good_desc.clone();
        bad_desc[0x38] ^= 1;
        assert_eq!(
            decode(&good_regs, &bad_desc),
            Err(Error::Desc("input layout size"))
        );
        // Mesh pipeline with an input layout is refused.
        let mesh_regs = RegBuilder::new(true).build();
        assert_eq!(
            decode(&mesh_regs, &good_desc),
            Err(Error::Desc("an input layout for a mesh pipeline"))
        );
    }

    #[test]
    fn enumerations() {
        assert_eq!(blend_factor(15, false), Ok(16));
        assert_eq!(blend_factor(17, true), Ok(18));
        assert_eq!(blend_factor(2, true), Ok(5));
        assert_eq!(blend_op(4), Ok(3));
        assert_eq!(stencil_op(3), Ok(3));
        assert_eq!(logic_op(0xcc), Ok(None));
        assert_eq!(logic_op(0x66), Ok(Some(10)));
    }

    #[test]
    fn a_logic_op_pipeline_decodes_rop3_on_targets() {
        let mut regs = RegBuilder::new(false);
        // ROP3 = 0x66 (XOR -> D3D12_LOGIC_OP_XOR = 10), write mask only on target 0.
        regs.set(at::CB_COLOR_CONTROL, 0x66 << 16);
        regs.set(at::CB_TARGET_MASK, 0x0000_000f);
        let a = regs.build();
        let b = desc_block(&[28], 0, 3, u32::MAX, 1, &[]);
        let d = decode(&a, &b).unwrap();
        assert!(d.targets_blend[0].logic_op_enable);
        assert_eq!(d.targets_blend[0].logic_op, 10);
        assert!(!d.targets_blend[0].blend);
    }

    /// Every record of the title's file (path in `VICTORY_PSO_CACHE` for an
    /// unpacked `v2.d3d12PipelineCache`, or `VICTORY_SHADERS_RPF` for the
    /// shader archive): all decode and agree with themselves.
    /// `cargo test -p victory-game pso_state -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn every_record_of_the_title_s_file() {
        let file = if let Some(path) = std::env::var_os("VICTORY_PSO_CACHE") {
            std::fs::read(path).unwrap()
        } else if let Some(path) = std::env::var_os("VICTORY_SHADERS_RPF") {
            let archive = std::fs::read(path).unwrap();
            crate::pso_cache::find_in_archive(&archive)
                .expect("pipeline cache inside shader archive")
        } else {
            return;
        };
        let cache = crate::pso_cache::Cache::parse(&file).unwrap();
        let (mut ok, mut agree, mut mesh, mut rects) = (0, 0, 0, 0);
        for i in 0..cache.graphics_count() {
            let g = cache.graphics(i).unwrap();
            let (a, b) = (
                cache.block(g.state[0]).unwrap(),
                cache.block(g.state[1]).unwrap(),
            );
            let d = decode(a, b).unwrap_or_else(|e| panic!("record {i}: {e}"));
            ok += 1;
            agree += usize::from(consistent(a, &d));
            mesh += usize::from(d.mesh);
            rects += usize::from(d.topology == RECTANGLES);
        }
        eprintln!(
            "pso_cache build={} driver={} graphics={} (mesh={}, rects={}) compute={} decoded={} consistent={}",
            cache.build,
            cache.driver,
            cache.graphics_count(),
            mesh,
            rects,
            cache.compute_count(),
            ok,
            agree
        );
        assert_eq!(ok, cache.graphics_count());
        assert_eq!(agree, ok);
    }
}

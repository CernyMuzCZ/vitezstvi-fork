//! The title's precompiled pipelines: `platform:/shaders/v2.d3d12PipelineCache`
//! (the title appends the extension at title+0x554cc50 and loads the file
//! at title+0x248e070). It is stored in the shader archive as a chain of zlib
//! streams of 64 KiB each. Its records are what the title hands the console
//! device's `DeserializeGraphicsPipelineStateX` and
//! `DeserializeComputePipelineStateX` (device slots 0x1c8 and 0x218):
//!
//! ```text
//! +0x00 u32 0x09005350, char[32] build, char[32] driver,
//!       u64, then u32 counts: graphics records (0x70 B), compute records
//!       (0x18 B), blocks, codes, block bytes, code bytes; 0x80 bytes in all
//! graphics u64 key, u32 x2 state blocks, u64 x5 hashes, 7 stage slots of
//!          (u32 code, u32 metadata block), 0xffffffff where unused
//! compute  u64 key, u32 x2 state blocks, u32 code, u32 metadata block
//! blocks   (u32 offset, u32 size) into the block bytes; codes likewise
//!          into the code bytes (0x100-aligned; the title copies them into
//!          GPU memory)
//! ```
//!
//! Stage slots, by the programs they hold: 0 pixel, 1 vertex, 2 mesh; slot 6
//! seen rarely, unknown. The code is native GFX10 compiled for the pipeline
//! (not the archive's programs), but it names its source: the first six
//! dwords are two VOP3 instructions whose operands spell the container's
//! `HASH` digest ([`code_shader_hash`]); the metadata block ends with the
//! same digest (entry 0x43, [`meta_shader_hash`]).

/// The file's first word.
pub const MAGIC: u32 = 0x0900_5350;
const HEADER: usize = 0x80;
const GRAPHICS_RECORD: usize = 0x70;
const COMPUTE_RECORD: usize = 0x18;
const NONE: u32 = u32::MAX;
/// Stage slots of a graphics record.
pub const GRAPHICS_SLOTS: usize = 7;

/// The stage a graphics stage slot holds, as the pipeline code names
/// stages; `None` for slots not seen holding a known stage.
pub fn slot_stage(slot: usize) -> Option<&'static str> {
    match slot {
        0 => Some("pixel"),
        1 => Some("vertex"),
        2 => Some("mesh"),
        _ => None,
    }
}

/// A parsed cache file (borrowing its bytes).
#[derive(Debug, Clone, Copy)]
pub struct Cache<'a> {
    pub build: &'a str,
    pub driver: &'a str,
    graphics: &'a [u8],
    compute: &'a [u8],
    blocks: &'a [u8],
    codes: &'a [u8],
    block_bytes: &'a [u8],
    code_bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Graphics {
    pub key: u64,
    pub state: [u32; 2],
    pub hashes: [u64; 5],
    /// Per slot: (code, metadata block), indices into the tables.
    pub stages: [Option<(u32, u32)>; GRAPHICS_SLOTS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compute {
    pub key: u64,
    pub state: [u32; 2],
    pub code: u32,
    pub meta: u32,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8)
        .map(|b| u64::from_le_bytes(b.try_into().expect("8 bytes")))
}

fn text(b: &[u8]) -> &str {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    std::str::from_utf8(&b[..end]).unwrap_or("")
}

impl<'a> Cache<'a> {
    /// The file, if its header and tables fit it exactly.
    pub fn parse(file: &'a [u8]) -> Result<Cache<'a>, &'static str> {
        if u32_at(file, 0) != Some(MAGIC) || file.len() < HEADER {
            return Err("not a pipeline cache");
        }
        let count = |i: usize| u32_at(file, 0x50 + 4 * i).expect("in the header") as usize;
        let sizes = [
            count(0)
                .checked_mul(GRAPHICS_RECORD)
                .ok_or("pipeline cache table overflow")?,
            count(1)
                .checked_mul(COMPUTE_RECORD)
                .ok_or("pipeline cache table overflow")?,
            count(2)
                .checked_mul(8)
                .ok_or("pipeline cache table overflow")?,
            count(3)
                .checked_mul(8)
                .ok_or("pipeline cache table overflow")?,
            count(4),
            count(5),
        ];
        let total = sizes
            .iter()
            .try_fold(HEADER, |acc, &n| acc.checked_add(n))
            .ok_or("pipeline cache table overflow")?;
        if total != file.len() {
            return Err("pipeline cache tables do not add up to its size");
        }
        let mut at = HEADER;
        let mut take = |n: usize| {
            let part = &file[at..at + n];
            at += n;
            part
        };
        Ok(Cache {
            build: text(&file[4..0x24]),
            driver: text(&file[0x24..0x44]),
            graphics: take(sizes[0]),
            compute: take(sizes[1]),
            blocks: take(sizes[2]),
            codes: take(sizes[3]),
            block_bytes: take(sizes[4]),
            code_bytes: take(sizes[5]),
        })
    }

    /// The total size the header of a file says it has (to know when a
    /// chain of streams is complete).
    pub fn size_of(header: &[u8]) -> Option<usize> {
        if u32_at(header, 0)? != MAGIC {
            return None;
        }
        let count = |i: usize| u32_at(header, 0x50 + 4 * i).map(|c| c as usize);
        let mut total = HEADER;
        total = total.checked_add(count(0)?.checked_mul(GRAPHICS_RECORD)?)?;
        total = total.checked_add(count(1)?.checked_mul(COMPUTE_RECORD)?)?;
        total = total.checked_add(count(2)?.checked_mul(8)?)?;
        total = total.checked_add(count(3)?.checked_mul(8)?)?;
        total = total.checked_add(count(4)?)?;
        total = total.checked_add(count(5)?)?;
        Some(total)
    }

    pub fn graphics_count(&self) -> usize {
        self.graphics.len() / GRAPHICS_RECORD
    }

    pub fn compute_count(&self) -> usize {
        self.compute.len() / COMPUTE_RECORD
    }

    pub fn graphics(&self, i: usize) -> Option<Graphics> {
        let r = self
            .graphics
            .get(i * GRAPHICS_RECORD..(i + 1) * GRAPHICS_RECORD)?;
        let mut stages = [None; GRAPHICS_SLOTS];
        for (s, slot) in stages.iter_mut().enumerate() {
            let (code, meta) = (u32_at(r, 0x38 + 8 * s)?, u32_at(r, 0x3c + 8 * s)?);
            *slot = (code != NONE).then_some((code, meta));
        }
        Some(Graphics {
            key: u64_at(r, 0)?,
            state: [u32_at(r, 8)?, u32_at(r, 12)?],
            hashes: [0, 1, 2, 3, 4].map(|k| u64_at(r, 0x10 + 8 * k).expect("in the record")),
            stages,
        })
    }

    pub fn compute(&self, i: usize) -> Option<Compute> {
        let r = self
            .compute
            .get(i * COMPUTE_RECORD..(i + 1) * COMPUTE_RECORD)?;
        Some(Compute {
            key: u64_at(r, 0)?,
            state: [u32_at(r, 8)?, u32_at(r, 12)?],
            code: u32_at(r, 16)?,
            meta: u32_at(r, 20)?,
        })
    }

    /// A state or metadata block.
    pub fn block(&self, i: u32) -> Option<&'a [u8]> {
        slice(self.blocks, self.block_bytes, i)
    }

    /// A stage's native code.
    pub fn code(&self, i: u32) -> Option<&'a [u8]> {
        slice(self.codes, self.code_bytes, i)
    }
}

fn slice<'a>(table: &[u8], bytes: &'a [u8], i: u32) -> Option<&'a [u8]> {
    let at = 8usize.checked_mul(i as usize)?;
    let (offset, size) = (u32_at(table, at)? as usize, u32_at(table, at + 4)? as usize);
    bytes.get(offset..offset.checked_add(size)?)
}

/// The shader digest a native program's first two instructions spell
/// (`v_nop`-like VOP3 instructions with a literal: low half of the first
/// dword, then the low bytes of `src0` and `src1`, then the literal).
pub fn code_shader_hash(code: &[u8]) -> Option<[u8; 16]> {
    let w: Vec<u32> = (0..6).map(|i| u32_at(code, 4 * i)).collect::<Option<_>>()?;
    if w[0] >> 16 != 0xd580 || w[3] >> 16 != 0xd580 {
        return None;
    }
    let half = |a: u32, b: u32| (a & 0xffff) | (b & 0xff) << 16 | (b >> 9 & 0xff) << 24;
    let mut out = [0u8; 16];
    for (i, d) in [half(w[0], w[1]), w[2], half(w[3], w[4]), w[5]]
        .into_iter()
        .enumerate()
    {
        out[4 * i..4 * i + 4].copy_from_slice(&d.to_le_bytes());
    }
    Some(out)
}

/// The shader digest a metadata block ends with (entry 0x43). Graphics
/// blocks have a 16-byte head (u32 2, u32 size used, two words), compute
/// blocks a 24-byte head (u32 0x403, ...); entries 0x41 (descriptor ranges
/// read) and 0x42 (user-data layout) are 0x20 bytes, 0x43 is 0x14.
pub fn meta_shader_hash(meta: &[u8]) -> Option<[u8; 16]> {
    let mut at = match u32_at(meta, 0)? {
        2 => 16,
        0x403 => 24,
        _ => return None,
    };
    loop {
        match u32_at(meta, at)? {
            0x41 | 0x42 => at += 0x20,
            0x43 => return meta.get(at + 4..at + 20)?.try_into().ok(),
            _ => return None,
        }
    }
}

/// The digest a stage names: its metadata's, else its code's; `None` if
/// neither has one or they disagree.
pub fn stage_shader_hash(code: &[u8], meta: &[u8]) -> Option<[u8; 16]> {
    match (meta_shader_hash(meta), code_shader_hash(code)) {
        (Some(m), Some(c)) if m != c => None,
        (Some(h), _) | (None, Some(h)) => Some(h),
        (None, None) => None,
    }
}

/// The cache file inside the shader archive: the chain of zlib streams
/// whose first unpacks to a cache header, each next stream starting at most
/// 64 bytes after the end of the one before.
pub fn find_in_archive(archive: &[u8]) -> Option<Vec<u8>> {
    let mut from = 0;
    while let Some(at) = memchr::memmem::find(&archive[from..], &[0x78]).map(|p| from + p) {
        from = at + 1;
        let Some(&next_byte) = archive.get(at + 1) else {
            break;
        };
        if (u16::from(archive[at]) << 8 | u16::from(next_byte)) % 31 != 0 {
            continue;
        }
        let Some((first, _)) = inflate_one(&archive[at..], HEADER) else {
            continue;
        };
        if u32_at(&first, 0) != Some(MAGIC) {
            continue;
        }
        return chain(archive, at);
    }
    None
}

/// The streams from `at` on, until the header's size is reached.
fn chain(archive: &[u8], mut at: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut want = usize::MAX;
    while out.len() < want {
        let (part, used) = inflate_one(archive.get(at..)?, usize::MAX)?;
        out.extend_from_slice(&part);
        if want == usize::MAX && out.len() >= HEADER {
            want = Cache::size_of(&out)?;
        }
        if out.len() >= want {
            break;
        }
        let next = at + used;
        at = (next..(next + 64).min(archive.len().saturating_sub(1))).find(|&i| {
            archive[i] == 0x78
                && (u16::from(archive[i]) << 8 | u16::from(archive[i + 1])) % 31 == 0
        })?;
    }
    (out.len() == want).then_some(out)
}

/// One zlib stream at the start of `input`: its bytes (at most `limit`) and
/// how many input bytes it took (when unpacked whole).
fn inflate_one(input: &[u8], limit: usize) -> Option<(Vec<u8>, usize)> {
    let mut state = zlib_rs::Inflate::new(true, 15);
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut in_pos = 0usize;
    let mut total_out = 0usize;
    loop {
        let max_write = if limit == usize::MAX {
            buf.len()
        } else {
            limit.saturating_sub(out.len()).min(buf.len())
        };
        if max_write == 0 {
            return Some((out, in_pos));
        }
        let status = state
            .decompress(
                &input[in_pos..],
                &mut buf[..max_write],
                zlib_rs::InflateFlush::NoFlush,
            )
            .ok()?;
        let next_in = state.total_in() as usize;
        let next_out = state.total_out() as usize;
        let written = next_out.saturating_sub(total_out);
        out.extend_from_slice(&buf[..written]);
        in_pos = next_in;
        total_out = next_out;
        if out.len() >= limit {
            out.truncate(limit);
            return Some((out, in_pos));
        }
        match status {
            zlib_rs::Status::StreamEnd => return Some((out, in_pos)),
            zlib_rs::Status::Ok if written > 0 || next_in > 0 => {
                if written == 0 && in_pos >= input.len() {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compress_zlib(data: &[u8], level: i32) -> Vec<u8> {
        let mut buf = vec![0u8; zlib_rs::compress_bound(data.len()) + 16];
        let config = zlib_rs::DeflateConfig {
            window_bits: 15,
            ..zlib_rs::DeflateConfig::new(level)
        };
        let (packed, rc) = zlib_rs::compress_slice(&mut buf, data, config);
        assert_eq!(rc, zlib_rs::ReturnCode::Ok);
        packed.to_vec()
    }

    /// A code block whose prologue spells `hash`.
    pub(crate) fn code_with(hash: [u8; 16], body: &[u32]) -> Vec<u8> {
        let d = |i: usize| u32::from_le_bytes(hash[4 * i..4 * i + 4].try_into().unwrap());
        let ops =
            |x: u32| 0xff << 18 | (0x100 | (x >> 24 & 0xff)) << 9 | 0x100 | (x >> 16 & 0xff);
        let mut w = vec![
            0xd580_0000 | (d(0) & 0xffff),
            ops(d(0)),
            d(1),
            0xd580_0000 | (d(2) & 0xffff),
            ops(d(2)),
            d(3),
        ];
        w.extend_from_slice(body);
        w.extend([0xbf9f_0000; 4]);
        w.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn meta_with(head: &[u32], hash: [u8; 16]) -> Vec<u8> {
        let mut m: Vec<u8> = head.iter().flat_map(|x| x.to_le_bytes()).collect();
        for kind in [0x41u32, 0x42] {
            m.extend_from_slice(&kind.to_le_bytes());
            m.extend_from_slice(&[7; 0x1c]);
        }
        m.extend_from_slice(&0x43u32.to_le_bytes());
        m.extend_from_slice(&hash);
        m.extend_from_slice(&[0; 12]);
        m
    }

    /// A cache of one graphics pipeline (pixel + mesh) and one compute one.
    pub(crate) fn cache_bytes(hashes: [[u8; 16]; 3]) -> Vec<u8> {
        let blocks: Vec<Vec<u8>> = vec![
            vec![1; 0x184],
            vec![2; 0x50],
            meta_with(&[2, 0, 0x76, 0], hashes[0]),
            meta_with(&[2, 0, 0x76, 0], hashes[1]),
            vec![3; 0x1c],
            vec![4; 0x14],
            meta_with(&[0x403, 0, 0x60, 0, 0, 0], hashes[2]),
        ];
        let codes: Vec<Vec<u8>> = hashes
            .iter()
            .map(|h| code_with(*h, &[0xbfa0_0003, 0xbf81_0000]))
            .collect();
        let table = |parts: &[Vec<u8>], align: usize| {
            let (mut t, mut bytes) = (Vec::new(), Vec::new());
            for p in parts {
                bytes.resize(bytes.len().next_multiple_of(align), 0);
                t.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                t.extend_from_slice(&(p.len() as u32).to_le_bytes());
                bytes.extend_from_slice(p);
            }
            (t, bytes)
        };
        let (bt, bb) = table(&blocks, 4);
        let (ct, cb) = table(&codes, 0x100);
        let mut g = Vec::new();
        g.extend_from_slice(&0x1122u64.to_le_bytes());
        g.extend_from_slice(&0u32.to_le_bytes());
        g.extend_from_slice(&1u32.to_le_bytes());
        for k in 0..5u64 {
            g.extend_from_slice(&(0x100 + k).to_le_bytes());
        }
        for slot in 0..GRAPHICS_SLOTS {
            let (c, m) = match slot {
                0 => (0, 2),
                2 => (1, 3),
                _ => (NONE, NONE),
            };
            g.extend_from_slice(&c.to_le_bytes());
            g.extend_from_slice(&m.to_le_bytes());
        }
        let mut c = Vec::new();
        c.extend_from_slice(&0x3344u64.to_le_bytes());
        for v in [4u32, 5, 2, 6] {
            c.extend_from_slice(&v.to_le_bytes());
        }
        let mut f = vec![0u8; HEADER];
        f[..4].copy_from_slice(&MAGIC.to_le_bytes());
        f[4..14].copy_from_slice(b"0x12345678");
        f[0x24..0x2f].copy_from_slice(b"100.200.300");
        for (i, n) in [1, 1, blocks.len(), codes.len(), bb.len(), cb.len()]
            .into_iter()
            .enumerate()
        {
            f[0x50 + 4 * i..0x54 + 4 * i].copy_from_slice(&(n as u32).to_le_bytes());
        }
        for part in [g, c, bt, ct, bb, cb] {
            f.extend_from_slice(&part);
        }
        f
    }

    const H: [[u8; 16]; 3] = [[0xa1; 16], [0xb2; 16], [0xc3; 16]];

    #[test]
    fn parses_records_blocks_and_codes() {
        let file = cache_bytes(H);
        let cache = Cache::parse(&file).unwrap();
        assert_eq!((cache.build, cache.driver), ("0x12345678", "100.200.300"));
        assert_eq!((cache.graphics_count(), cache.compute_count()), (1, 1));
        assert_eq!(Cache::size_of(&file[..HEADER]), Some(file.len()));
        let g = cache.graphics(0).unwrap();
        assert_eq!(g.key, 0x1122);
        assert_eq!(g.hashes[4], 0x104);
        assert_eq!(cache.block(g.state[0]).unwrap().len(), 0x184);
        let used: Vec<usize> = (0..GRAPHICS_SLOTS)
            .filter(|&s| g.stages[s].is_some())
            .collect();
        assert_eq!(used, [0, 2]);
        assert_eq!(
            used.iter()
                .map(|&s| slot_stage(s).unwrap())
                .collect::<Vec<_>>(),
            ["pixel", "mesh"]
        );
        for (slot, want) in [(0, H[0]), (2, H[1])] {
            let (code, meta) = g.stages[slot].unwrap();
            let (code, meta) = (cache.code(code).unwrap(), cache.block(meta).unwrap());
            assert_eq!(stage_shader_hash(code, meta), Some(want));
        }
        let c = cache.compute(0).unwrap();
        assert_eq!((c.key, c.state), (0x3344, [4, 5]));
        assert_eq!(cache.block(c.state[0]).unwrap().len(), 0x1c);
        assert_eq!(
            stage_shader_hash(cache.code(c.code).unwrap(), cache.block(c.meta).unwrap()),
            Some(H[2])
        );
        assert!(cache.graphics(1).is_none() && cache.code(9).is_none());
        // A size that disagrees with the tables is refused.
        assert!(Cache::parse(&file[..file.len() - 1]).is_err());
    }

    #[test]
    fn the_code_prologue_spells_a_distinct_16_byte_digest() {
        let want: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        let code = code_with(want, &[0xd580_0000]);
        assert_eq!(code_shader_hash(&code), Some(want));
        assert_eq!(code_shader_hash(&code[4..]), None);
        // Metadata and code that disagree name nothing.
        let meta = meta_with(&[2, 0, 0x76, 0], [9; 16]);
        assert_eq!(meta_shader_hash(&meta), Some([9; 16]));
        assert_eq!(stage_shader_hash(&code, &meta), None);
        assert_eq!(stage_shader_hash(&code, &[]), Some(want));
        assert_eq!(meta_shader_hash(&[5, 0, 0, 0]), None);
        // Agreeing code and metadata succeed.
        let matching = meta_with(&[0x403, 0, 0x60, 0, 0, 0], want);
        assert_eq!(stage_shader_hash(&code, &matching), Some(want));
    }

    #[test]
    fn finds_the_chain_of_streams_in_an_archive() {
        let file = cache_bytes(H);
        let mut archive = vec![0x78u8, 0x9c, 0x00, 0x01, 0x55];
        archive.extend(compress_zlib(b"another file", 6));
        // The cache in 100-byte pieces, with a few bytes between streams.
        for (i, piece) in file.chunks(100).enumerate() {
            archive.extend(compress_zlib(piece, 9));
            archive.extend(std::iter::repeat_n(0u8, i % 13));
        }
        assert_eq!(find_in_archive(&archive).as_deref(), Some(&file[..]));
        // A chain that stops early is no file.
        let cut = archive.len() - 40;
        assert_eq!(find_in_archive(&archive[..cut]), None);
    }

    /// Read-only archive coverage check against `VICTORY_SHADERS_RPF`:
    /// parses `v2.d3d12PipelineCache`, indexes `SGD5`, and verifies stage
    /// digest match rate and graphics state decode coverage.
    /// `cargo test -p victory-game pso_cache::tests::archive_coverage -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn archive_coverage() {
        let Some(path) = std::env::var_os("VICTORY_SHADERS_RPF") else {
            return;
        };
        let mut f = std::fs::File::open(&path).unwrap();
        let entries = crate::sgd5::index(&mut f).unwrap();
        let known: std::collections::HashSet<[u8; 16]> =
            entries.iter().map(|(h, _)| *h).collect();
        let archive = std::fs::read(&path).unwrap();
        let file = find_in_archive(&archive).expect("pipeline cache inside archive");
        let cache = Cache::parse(&file).unwrap();

        let (mut g_total, mut g_decoded, mut g_consistent, mut g_all_mapped, mut g_mesh, mut g_rects) =
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        let (mut stage_total, mut stage_named, mut stage_mapped, mut unknown_slots) =
            (0usize, 0usize, 0usize, 0usize);
        for i in 0..cache.graphics_count() {
            g_total += 1;
            let g = cache.graphics(i).unwrap();
            let (a, b) = (
                cache.block(g.state[0]).unwrap(),
                cache.block(g.state[1]).unwrap(),
            );
            let d = crate::pso_state::decode(a, b).unwrap();
            g_decoded += 1;
            g_consistent += usize::from(crate::pso_state::consistent(a, &d));
            g_mesh += usize::from(d.mesh);
            g_rects += usize::from(d.topology == crate::pso_state::RECTANGLES);
            let mut all_ok = true;
            let mut has_stage = false;
            for (slot, st) in g.stages.iter().enumerate() {
                let Some((code_i, meta_i)) = *st else {
                    continue;
                };
                has_stage = true;
                stage_total += 1;
                if slot_stage(slot).is_none() {
                    unknown_slots += 1;
                    all_ok = false;
                    continue;
                }
                let code = cache.code(code_i).unwrap();
                let meta = cache.block(meta_i).unwrap_or_default();
                match stage_shader_hash(code, meta) {
                    Some(h) => {
                        stage_named += 1;
                        if known.contains(&h) {
                            stage_mapped += 1;
                        } else {
                            all_ok = false;
                        }
                    }
                    None => all_ok = false,
                }
            }
            if has_stage && all_ok {
                g_all_mapped += 1;
            }
        }
        let (mut c_total, mut c_mapped) = (0usize, 0usize);
        for i in 0..cache.compute_count() {
            c_total += 1;
            let c = cache.compute(i).unwrap();
            let code = cache.code(c.code).unwrap();
            let meta = cache.block(c.meta).unwrap_or_default();
            if let Some(h) = stage_shader_hash(code, meta)
                && known.contains(&h)
            {
                c_mapped += 1;
            }
        }
        eprintln!(
            "ARCHIVE_COVERAGE sgd5_unique={} cache_bytes={} build={} driver={} graphics={} (decoded={}, consistent={}, all_stages_mapped={}, mesh={}, rects={}) stages={} (named={}, mapped={}, unknown_slots={}) compute={} (mapped={})",
            known.len(),
            file.len(),
            cache.build,
            cache.driver,
            g_total,
            g_decoded,
            g_consistent,
            g_all_mapped,
            g_mesh,
            g_rects,
            stage_total,
            stage_named,
            stage_mapped,
            unknown_slots,
            c_total,
            c_mapped
        );
        assert_eq!(g_decoded, g_total);
        assert_eq!(g_consistent, g_total);
        assert!(g_all_mapped > 0);
        assert!(c_mapped > 0);
    }
}

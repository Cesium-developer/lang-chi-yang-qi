use std::env;
mod wdl_storage;

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU16, AtomicU8, AtomicU64, Ordering};
use std::time::Instant;
use wdl_storage::{load_wdl_layer, pack_or_prefix};

const N: i32 = 5;
const CELLS: usize = 25;
const FREE: usize = 22;
const BOARD_MASK: u32 = (1u32 << CELLS) - 1;
const WOLF: u8 = 0;
const SHEEP: u8 = 1;
const DRAW: u8 = 0;
const WOLF_WIN: u8 = 1;
const SHEEP_WIN: u8 = 2;
const INF: u16 = u16::MAX;
const MAX_DTM: usize = 2047;
const OR_GROUP: u64 = 256;
const MIN_FREE_RAM: u64 = 2 * 1024 * 1024 * 1024;

#[cfg(target_os = "windows")]
#[repr(C)]
struct MemoryStatusEx {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" { fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32; }

fn available_ram_bytes() -> Option<u64> {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut s = MemoryStatusEx {
            dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        if GlobalMemoryStatusEx(&mut s) != 0 { return Some(s.ull_avail_phys); }
    }
    None
}

fn binom_table(n: usize) -> Vec<Vec<u64>> {
    let mut c = vec![vec![0u64; n + 1]; n + 1];
    for i in 0..=n {
        c[i][0] = 1;
        for j in 1..=i {
            c[i][j] = c[i - 1][j - 1] + if j < i { c[i - 1][j] } else { 0 };
        }
    }
    c
}

fn unrank_comb(mut r: u64, k: usize, n: usize, b: &[Vec<u64>], out: &mut [usize]) {
    let mut c = n as i64 - 1;
    for i in (0..k).rev() {
        while c >= 0 && b[c as usize][i + 1] > r { c -= 1; }
        out[i] = c as usize;
        r -= b[c as usize][i + 1];
        c -= 1;
    }
}

#[inline]
fn pext_soft(x: u32, mut mask: u32) -> u32 {
    let mut out = 0;
    let mut dst = 1;
    while mask != 0 {
        let bit = mask & mask.wrapping_neg();
        if x & bit != 0 { out |= dst; }
        mask &= mask - 1;
        dst <<= 1;
    }
    out
}

#[inline]
fn pdep_soft(mut x: u32, mut mask: u32) -> u32 {
    let mut out = 0;
    while mask != 0 {
        let bit = mask & mask.wrapping_neg();
        if x & 1 != 0 { out |= bit; }
        x >>= 1;
        mask &= mask - 1;
    }
    out
}

#[inline]
#[target_feature(enable = "bmi2")]
unsafe fn pext_bmi2(x: u32, mask: u32) -> u32 {
    #[cfg(target_arch = "x86_64")]
    { std::arch::x86_64::_pext_u32(x, mask) }
    #[cfg(not(target_arch = "x86_64"))]
    { pext_soft(x, mask) }
}

#[inline]
#[target_feature(enable = "bmi2")]
unsafe fn pdep_bmi2(x: u32, mask: u32) -> u32 {
    #[cfg(target_arch = "x86_64")]
    { std::arch::x86_64::_pdep_u32(x, mask) }
    #[cfg(not(target_arch = "x86_64"))]
    { pdep_soft(x, mask) }
}

struct Ctx {
    b: Vec<Vec<u64>>,
    wolf_masks: Vec<u32>,
    wolf_rank: Vec<u16>,
    sheep_masks: Vec<Vec<u32>>,
    neigh: [[i8; 4]; CELLS],
    jump: [[i8; 4]; CELLS],
    bmi2: bool,
}

impl Ctx {
    fn new() -> Self {
        let b = binom_table(CELLS);
        let mut wolf_masks = vec![0u32; b[CELLS][3] as usize];
        let mut wolf_rank = vec![u16::MAX; 1usize << CELLS];
        let mut pos = [0usize; 3];
        for r in 0..wolf_masks.len() {
            unrank_comb(r as u64, 3, CELLS, &b, &mut pos);
            let mask = (1u32 << pos[0]) | (1u32 << pos[1]) | (1u32 << pos[2]);
            wolf_masks[r] = mask;
            wolf_rank[mask as usize] = r as u16;
        }
        let mut sheep_masks = vec![Vec::new(); FREE + 1];
        for s in 0..=FREE {
            let count = b[FREE][s] as usize;
            sheep_masks[s].resize(count, 0);
            let mut slots = vec![0usize; s];
            for r in 0..count {
                if s > 0 { unrank_comb(r as u64, s, FREE, &b, &mut slots); }
                let mut mask = 0u32;
                for &p in &slots { mask |= 1u32 << p; }
                sheep_masks[s][r] = mask;
            }
        }
        let mut neigh = [[-1i8; 4]; CELLS];
        let mut jump = [[-1i8; 4]; CELLS];
        for r in 0..N {
            for c in 0..N {
                let i = (r * N + c) as usize;
                let rr = [r - 1, r + 1, r, r];
                let cc = [c, c, c - 1, c + 1];
                let dr = [-1, 1, 0, 0];
                let dc = [0, 0, -1, 1];
                for d in 0..4 {
                    let (nr, nc) = (rr[d], cc[d]);
                    if nr >= 0 && nr < N && nc >= 0 && nc < N {
                        neigh[i][d] = (nr * N + nc) as i8;
                        let (jr, jc) = (nr + dr[d], nc + dc[d]);
                        if jr >= 0 && jr < N && jc >= 0 && jc < N {
                            jump[i][d] = (jr * N + jc) as i8;
                        }
                    }
                }
            }
        }
        let bmi2 = cfg!(target_arch = "x86_64") && std::is_x86_feature_detected!("bmi2");
        eprintln!("index codec: BMI2 PDEP/PEXT={bmi2}");
        Self { b, wolf_masks, wolf_rank, sheep_masks, neigh, jump, bmi2 }
    }

    #[inline]
    fn pext(&self, x: u32, mask: u32) -> u32 {
        if self.bmi2 { unsafe { pext_bmi2(x, mask) } } else { pext_soft(x, mask) }
    }
    #[inline]
    fn pdep(&self, x: u32, mask: u32) -> u32 {
        if self.bmi2 { unsafe { pdep_bmi2(x, mask) } } else { pdep_soft(x, mask) }
    }
    #[inline]
    fn decode(&self, idx: u64, s: usize, ns: u64) -> (u32, u32, u8) {
        let side = (idx & 1) as u8;
        let t = idx >> 1;
        let wr = (t / ns) as usize;
        let sr = (t % ns) as usize;
        let wm = self.wolf_masks[wr];
        let free = BOARD_MASK ^ wm;
        let sm = self.pdep(self.sheep_masks[s][sr], free);
        (wm, sm, side)
    }
    #[inline]
    fn encode_ranked(&self, wm: u32, sm: u32, side: u8, ns: u64, srank: &[u32]) -> Option<u64> {
        let wr = *self.wolf_rank.get(wm as usize)?;
        if wr == u16::MAX { return None; }
        let slots = self.pext(sm, BOARD_MASK ^ wm);
        let sr = *srank.get(slots as usize)?;
        if sr == u32::MAX { return None; }
        Some(((wr as u64 * ns + sr as u64) * 2) + side as u64)
    }
    #[inline]
    fn for_each_child<F>(&self, wm: u32, sm: u32, side: u8, s: usize, mut f: F)
    where F: FnMut(u32, u32, u8, usize, bool) {
        let occ = wm | sm;
        if side == WOLF {
            let mut pieces = wm;
            while pieces != 0 {
                let from = pieces.trailing_zeros() as usize;
                pieces &= pieces - 1;
                for d in 0..4 {
                    let n = self.neigh[from][d];
                    if n < 0 { continue; }
                    let n = n as usize;
                    if (occ & (1u32 << n)) != 0 { continue; }
                    let moved = (wm & !(1u32 << from)) | (1u32 << n);
                    f(moved, sm, SHEEP, s, false);
                    let j = self.jump[from][d];
                    if j >= 0 && (sm & (1u32 << (j as usize))) != 0 {
                        let j = j as usize;
                        let cap = (wm & !(1u32 << from)) | (1u32 << j);
                        f(cap, sm & !(1u32 << j), SHEEP, s - 1, true);
                    }
                }
            }
        } else {
            let mut pieces = sm;
            while pieces != 0 {
                let from = pieces.trailing_zeros() as usize;
                pieces &= pieces - 1;
                for d in 0..4 {
                    let n = self.neigh[from][d];
                    if n < 0 { continue; }
                    let n = n as usize;
                    if (occ & (1u32 << n)) == 0 {
                        f(wm, (sm & !(1u32 << from)) | (1u32 << n), WOLF, s, false);
                    }
                }
            }
        }
    }

    #[inline]
    fn for_each_reverse<F>(&self, wm: u32, sm: u32, child_side: u8, mut f: F)
    where F: FnMut(u32, u32, u8) {
        let occ = wm | sm;
        let parent_side = 1 - child_side;
        let pieces = if child_side == WOLF { sm } else { wm };
        let mut left = pieces;
        while left != 0 {
            let to = left.trailing_zeros() as usize;
            left &= left - 1;
            for d in 0..4 {
                let from = self.neigh[to][d];
                if from < 0 { continue; }
                let from = from as usize;
                if (occ & (1u32 << from)) != 0 { continue; }
                if child_side == WOLF {
                    let parent_sm = (sm & !(1u32 << to)) | (1u32 << from);
                    f(wm, parent_sm, parent_side);
                } else {
                    let parent_wm = (wm & !(1u32 << to)) | (1u32 << from);
                    f(parent_wm, sm, parent_side);
                }
            }
        }
    }
}

#[inline]
fn wdl_get(data: &[u8], idx: u64) -> u8 {
    (data[(idx >> 2) as usize] >> ((idx & 3) * 2)) & 3
}

#[inline]
fn is_or_value(value: u8, idx: u64) -> bool {
    (value == WOLF_WIN && idx & 1 == WOLF as u64) || (value == SHEEP_WIN && idx & 1 == SHEEP as u64)
}

const fn build_or_lut() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut byte = 0usize;
    while byte < 256 {
        let mut slot = 0usize;
        let mut count = 0u8;
        while slot < 4 {
            let value = ((byte >> (slot * 2)) & 3) as u8;
            if (value == WOLF_WIN && slot & 1 == 0) || (value == SHEEP_WIN && slot & 1 == 1) {
                count += 1;
            }
            slot += 1;
        }
        table[byte] = count;
        byte += 1;
    }
    table
}
const OR_LUT: [u8; 256] = build_or_lut();

#[inline]
fn count_or_range(wdl: &[u8], start: u64, end: u64) -> u32 {
    debug_assert!(start % 4 == 0);
    let full_end = end & !3;
    let mut total = 0u32;
    let mut byte = start >> 2;
    let byte_end = full_end >> 2;
    while byte < byte_end {
        total += OR_LUT[wdl[byte as usize] as usize] as u32;
        byte += 1;
    }
    let mut idx = full_end;
    while idx < end {
        total += is_or_value(wdl_get(wdl, idx), idx) as u32;
        idx += 1;
    }
    total
}

fn build_or_prefix(wdl: &[u8], size: u64, threads: usize) -> Vec<u32> {
    let groups = ((size + OR_GROUP - 1) / OR_GROUP) as usize;
    let mut counts = vec![0u16; groups];
    let per = (groups + threads - 1) / threads;
    std::thread::scope(|scope| {
        let mut rest = counts.as_mut_slice();
        for t in 0..threads {
            let lo = t * per;
            let hi = ((t + 1) * per).min(groups);
            if lo >= hi { continue; }
            let (mine, tail) = rest.split_at_mut(hi - lo);
            rest = tail;
            let wdl = wdl;
            scope.spawn(move || {
                for (j, cell) in mine.iter_mut().enumerate() {
                    let group = lo + j;
                    let start = group as u64 * OR_GROUP;
                    let end = (start + OR_GROUP).min(size);
                    *cell = count_or_range(wdl, start, end) as u16;
                }
            });
        }
    });
    let mut prefix = vec![0u32; groups + 1];
    for i in 0..groups { prefix[i + 1] = prefix[i] + counts[i] as u32; }
    prefix
}

#[inline]
fn packed_get(data: &[u8], bits: u8, idx: u64) -> u16 {
    let bit = idx * bits as u64;
    let byte = (bit >> 3) as usize;
    let shift = (bit & 7) as u32;
    let mut word = data[byte] as u64;
    if byte + 1 < data.len() { word |= (data[byte + 1] as u64) << 8; }
    if byte + 2 < data.len() { word |= (data[byte + 2] as u64) << 16; }
    ((word >> shift) & ((1u64 << bits) - 1)) as u16
}

fn write_u32s(path: &str, data: &[u32]) -> Result<(), String> {
    use std::io::Write;
    let file = fs::File::create(path).map_err(|e| e.to_string())?;
    let mut out = std::io::BufWriter::new(file);
    for x in data { out.write_all(&x.to_le_bytes()).map_err(|e| e.to_string())?; }
    out.flush().map_err(|e| e.to_string())
}

struct FullDtm {
    packed: Vec<u8>,
    bits: u8,
}

fn add_candidate(buckets: &mut Vec<Vec<u32>>, d: u16, idx: u32) -> Result<(), String> {
    if d as usize > MAX_DTM { return Err(format!("DTM 超出安全表示上限 {}: {}", MAX_DTM, d)); }
    if buckets.len() <= d as usize { buckets.resize_with(d as usize + 1, Vec::new); }
    buckets[d as usize].push(idx);
    Ok(())
}

fn pack_full_dist(dist: &[AtomicU16], size: u64, bits: u8, threads: usize) -> Vec<u8> {
    let packed_len = ((size * bits as u64 + 7) / 8) as usize;
    let mut packed = vec![0u8; packed_len];
    let block_states: usize = 1 << 18;
    let block_bytes = block_states * bits as usize / 8;
    let nblocks = (size as usize + block_states - 1) / block_states;
    let workers = threads.min(nblocks.max(1));
    std::thread::scope(|scope| {
        let mut rest = packed.as_mut_slice();
        let blocks_per = (nblocks + workers - 1) / workers;
        for t in 0..workers {
            let blo = t * blocks_per;
            let bhi = ((t + 1) * blocks_per).min(nblocks);
            if blo >= bhi { continue; }
            let byte_lo = blo * block_bytes;
            let byte_hi = if bhi == nblocks { packed_len } else { bhi * block_bytes };
            let (mine, tail) = rest.split_at_mut(byte_hi - byte_lo);
            rest = tail;
            let dist = dist;
            scope.spawn(move || {
                for b in blo..bhi {
                    let state_lo = b * block_states;
                    let state_hi = ((b + 1) * block_states).min(size as usize);
                    let out_lo = (b - blo) * block_bytes;
                    let out_hi = if b + 1 == nblocks { mine.len() } else { out_lo + block_bytes };
                    let out = &mut mine[out_lo..out_hi];
                    let mut acc = 0u64;
                    let mut fill = 0u32;
                    let mut byte = 0usize;
                    for i in state_lo..state_hi {
                        let d = dist[i].load(Ordering::Relaxed);
                        let value = if d == INF { 0 } else { d as u64 };
                        acc |= value << fill;
                        fill += bits as u32;
                        while fill >= 8 {
                            out[byte] = acc as u8;
                            byte += 1;
                            acc >>= 8;
                            fill -= 8;
                        }
                    }
                    if fill != 0 { out[byte] = acc as u8; }
                }
            });
        }
    });
    packed
}

fn compress_compact(raw_path: &str, target: &str, states: u64, values: usize, bits: u8) -> Result<(), String> {
    use std::process::Command;
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let solver_dir = exe.parent().ok_or("solver path has no parent")?;
    let script = solver_dir.join("compress_dtm.py");
    let mut last_error = String::new();
    for python in ["python", "python3", "py"] {
        match Command::new(python)
            .arg(&script)
            .arg(raw_path)
            .arg(target)
            .arg(states.to_string())
            .arg(values.to_string())
            .arg(bits.to_string())
            .current_dir(solver_dir)
            .status()
        {
            Ok(status) if status.success() => {
                fs::remove_file(raw_path).map_err(|e| format!("删除本程序临时紧凑流失败: {e}"))?;
                return Ok(());
            }
            Ok(status) => return Err(format!("DTM 分块压缩失败: {status}")),
            Err(error) => last_error = format!("{python}: {error}"),
        }
    }
    Err(format!("找不到可用的 Python 解释器: {last_error}"))
}

fn tree_bytes(path: &std::path::Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else { return 0; };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if let Ok(kind) = entry.file_type() {
            if kind.is_dir() { total = total.saturating_add(tree_bytes(&p)); }
            else if kind.is_file() { total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0)); }
        }
    }
    total
}

fn workspace_root() -> Result<std::path::PathBuf, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let solver_dir = exe.parent().ok_or("solver executable has no parent")?;
    Ok(solver_dir.parent().ok_or("solver directory has no parent")?.to_path_buf())
}

fn verify_layer(
    ctx: &Ctx, s: usize, threads: usize, ns: u64, wdl: &[u8], dist: &[AtomicU16], srank: &[u32],
    prev_wdl: Option<&[u8]>, prev_dtm: Option<&(Vec<u8>, u8)>, prev_srank: Option<&[u32]>, prev_ns: u64,
) -> Result<(), String> {
    let size = 2300u64 * ns * 2;
    let per = (size + threads as u64 - 1) / threads as u64;
    let bad = AtomicU64::new(0);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for t in 0..threads {
            let lo = t as u64 * per;
            let hi = ((t as u64 + 1) * per).min(size);
            if lo >= hi { continue; }
            let bad_ref = &bad;
            handles.push(scope.spawn(move || {
                let mut local_bad = 0u64;
                for idx in lo..hi {
                    let target = wdl_get(wdl, idx);
                    if target == DRAW { continue; }
                    let side = (idx & 1) as u8;
                    let (wm, sm, _) = ctx.decode(idx, s, ns);
                    let is_or = (target == WOLF_WIN && side == WOLF) || (target == SHEEP_WIN && side == SHEEP);
                    let mut any = false;
                    let mut best = u16::MAX;
                    let mut worst = 0u16;
                    let mut invalid = false;
                    ctx.for_each_child(wm, sm, side, s, |cwm, csm, cs, _child_s, capture| {
                        any = true;
                        let (cv, cd) = if capture && s == 4 {
                            (WOLF_WIN, 0u16)
                        } else if capture {
                            let Some(pw) = prev_wdl else { invalid = true; return; };
                            let Some((pd, bits)) = prev_dtm else { invalid = true; return; };
                            let Some(psrank) = prev_srank else { invalid = true; return; };
                            let Some(pidx) = ctx.encode_ranked(cwm, csm, cs, prev_ns, psrank) else { invalid = true; return; };
                            let v = wdl_get(pw, pidx);
                            let d = if v == target { packed_get(pd, *bits, pidx) } else { 0 };
                            (v, d)
                        } else {
                            let Some(cidx) = ctx.encode_ranked(cwm, csm, cs, ns, srank) else { invalid = true; return; };
                            let v = wdl_get(wdl, cidx);
                            let d = if v == target { dist[cidx as usize].load(Ordering::Acquire) } else { 0 };
                            (v, d)
                        };
                        if is_or {
                            if cv == target {
                                if cd == INF { invalid = true; }
                                else { best = best.min(cd); }
                            }
                        } else if cv != target || cd == INF {
                            invalid = true;
                        } else {
                            worst = worst.max(cd);
                        }
                    });
                    let expected = if !any {
                        0
                    } else if is_or {
                        if best == u16::MAX { invalid = true; 0 } else { best.saturating_add(1) }
                    } else {
                        worst.saturating_add(1)
                    };
                    if invalid || dist[idx as usize].load(Ordering::Acquire) != expected { local_bad += 1; }
                }
                bad_ref.fetch_add(local_bad, Ordering::Relaxed);
            }));
        }
        for h in handles { h.join().unwrap(); }
    });
    let n = bad.load(Ordering::Relaxed);
    if n == 0 { Ok(()) } else { Err(format!("s={s} Bellman 校验不一致: {n} 个局面")) }
}

fn solve_layer(ctx: &Ctx, s: usize, threads: usize, prev_full: Option<FullDtm>) -> Result<FullDtm, String> {
    let ns = ctx.b[FREE][s];
    let size = 2300u64 * ns * 2;
    let wdl = load_wdl_layer(Path::new("."), s).map_err(|e| format!("读 WDL s={s}: {e}"))?;
    if wdl.len() as u64 != (size + 3) / 4 { return Err(format!("WDL s={s} 长度错误")); }
    let prev_wdl = if s > 4 { Some(load_wdl_layer(Path::new("."), s - 1).map_err(|e| e.to_string())?) } else { None };
    let srank_len = 1usize << FREE;
    let prev_dtm: Option<(Vec<u8>, u8)> = prev_full.map(|x| (x.packed, x.bits));
    let mut prev_srank: Option<Vec<u32>> = None;
    if s > 4 {
        let (raw, bits) = prev_dtm.as_ref().ok_or_else(|| format!("s={s} 缺少前层完整 DTM"))?;
        let prev_size = 2300u64 * ctx.b[FREE][s - 1] * 2;
        if raw.len() as u64 != (prev_size * *bits as u64 + 7) / 8 { return Err(format!("前层完整临时 DTM 长度错误: s={}", s - 1)); }
        let mut ranks = vec![u32::MAX; srank_len];
        for (r, &mask) in ctx.sheep_masks[s - 1].iter().enumerate() { ranks[mask as usize] = r as u32; }
        prev_srank = Some(ranks);
    }
    let prev_ns = if s > 4 { ctx.b[FREE][s - 1] } else { 0 };
    let mut srank = vec![u32::MAX; srank_len];
    for (r, &mask) in ctx.sheep_masks[s].iter().enumerate() { srank[mask as usize] = r as u32; }

    let base_arrays = size.checked_mul(3).ok_or("layer memory size overflow")?;
    if let Some(avail) = available_ram_bytes() {
        let required = base_arrays.saturating_add(MIN_FREE_RAM);
        if avail < required {
            return Err(format!("RAM guard: s={s} needs about {:.2} GiB for core arrays plus 2 GiB headroom; only {:.2} GiB available",
                required as f64 / 1073741824.0, avail as f64 / 1073741824.0));
        }
        eprintln!("s={s} RAM check: {:.2} GiB available before allocating arrays", avail as f64 / 1073741824.0);
    }

    eprintln!("s={s} states={size} threads={threads} initializing");
    let started = Instant::now();
    let dist: Vec<AtomicU16> = (0..size).map(|_| AtomicU16::new(INF)).collect();
    let cnt: Vec<AtomicU8> = (0..size).map(|_| AtomicU8::new(0)).collect();
    let mut buckets: Vec<Vec<u32>> = (0..64).map(|_| Vec::new()).collect();
    let errors = AtomicU64::new(0);
    let per = (size + threads as u64 - 1) / threads as u64;

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads).map(|t| {
            let lo = t as u64 * per;
            let hi = ((t as u64 + 1) * per).min(size);
            let (dist, cnt, wdl, prev_wdl, prev_dtm, prev_srank, errors) =
                (&dist, &cnt, &wdl, &prev_wdl, &prev_dtm, &prev_srank, &errors);
            scope.spawn(move || {
                let mut local: Vec<(u16, u32)> = Vec::new();
                for idx in lo..hi {
                    let target = wdl_get(wdl, idx);
                    if target == DRAW { continue; }
                    let side = (idx & 1) as u8;
                    let (wm, sm, _) = ctx.decode(idx, s, ns);
                    let is_or = (target == WOLF_WIN && side == WOLF) || (target == SHEEP_WIN && side == SHEEP);
                    let mut normal_count = 0u8;
                    let mut best_cross: Option<u16> = None;
                    let mut max_cross = 0u16;
                    let mut has_cross = false;
                    ctx.for_each_child(wm, sm, side, s, |cwm, csm, cs, _child_s, capture| {
                        if !capture {
                            normal_count = normal_count.saturating_add(1);
                            return;
                        }
                        let (cv, cd) = if s == 4 {
                            (WOLF_WIN, 0u16)
                        } else {
                            let pr = prev_srank.as_ref().unwrap();
                            let cidx = match ctx.encode_ranked(cwm, csm, cs, prev_ns, pr) {
                                Some(v) => v,
                                None => { errors.fetch_add(1, Ordering::Relaxed); return; }
                            };
                            let pv = wdl_get(prev_wdl.as_ref().unwrap(), cidx);
                            let pd = if pv == target {
                                let (pdat, pbits) = prev_dtm.as_ref().unwrap();
                                packed_get(pdat, *pbits, cidx)
                            } else { 0 };
                            (pv, pd)
                        };
                        if cv == target {
                            has_cross = true;
                            if is_or {
                                let cand = cd.saturating_add(1);
                                best_cross = Some(best_cross.map_or(cand, |x| x.min(cand)));
                            } else {
                                max_cross = max_cross.max(cd);
                            }
                        } else if !is_or {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                    if is_or {
                        if let Some(d) = best_cross {
                            dist[idx as usize].store(d, Ordering::Relaxed);
                            local.push((d, idx as u32));
                        }
                    } else {
                        cnt[idx as usize].store(normal_count, Ordering::Relaxed);
                        dist[idx as usize].store(max_cross, Ordering::Relaxed);
                        let terminal = (target == SHEEP_WIN && side == WOLF) || (target == WOLF_WIN && side == SHEEP);
                        if terminal && normal_count == 0 && !has_cross {
                            dist[idx as usize].store(0, Ordering::Relaxed);
                            local.push((0, idx as u32));
                        } else if normal_count == 0 && has_cross {
                            let d = max_cross.saturating_add(1);
                            dist[idx as usize].store(d, Ordering::Relaxed);
                            local.push((d, idx as u32));
                        }
                    }
                }
                local
            })
        }).collect();
        for h in handles {
            for (d, idx) in h.join().unwrap() {
                if add_candidate(&mut buckets, d, idx).is_err() { errors.fetch_add(1, Ordering::Relaxed); }
            }
        }
    });
    if errors.load(Ordering::Relaxed) != 0 { return Err(format!("s={s} 初始化异常 {}", errors.load(Ordering::Relaxed))); }
    let seed_count: usize = buckets.iter().map(Vec::len).sum();
    eprintln!("s={s} initial candidates={seed_count}; t={:.1}s", started.elapsed().as_secs_f64());
    if let Some(avail) = available_ram_bytes() {
        if avail < MIN_FREE_RAM { return Err(format!("RAM guard after seeding: only {:.2} GiB available", avail as f64 / 1073741824.0)); }
    }
    let mut max_processed = 0usize;
    let mut total_popped = 0u64;
    let duplicate_edges = AtomicU64::new(0);
    for d in 0..=MAX_DTM {
        if d >= buckets.len() { break; }
        let frontier = std::mem::take(&mut buckets[d]);
        if frontier.is_empty() { continue; }
        max_processed = d;
        total_popped += frontier.len() as u64;
        let workers = threads.min(frontier.len().max(1));
        let per_f = (frontier.len() + workers - 1) / workers;
        let mut outputs: Vec<Vec<(u16, u32)>> = Vec::with_capacity(workers);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for t in 0..workers {
                let lo = t * per_f;
                let hi = ((t + 1) * per_f).min(frontier.len());
                if lo >= hi { continue; }
                let (dist, cnt, wdl, srank, duplicate_edges) = (&dist, &cnt, &wdl, &srank, &duplicate_edges);
                let fr = &frontier[lo..hi];
                handles.push(scope.spawn(move || {
                    let mut local = Vec::new();
                    for &u in fr {
                        if dist[u as usize].load(Ordering::Acquire) as usize != d { continue; }
                        let (wm, sm, side) = ctx.decode(u as u64, s, ns);
                        let target = wdl_get(wdl, u as u64);
                        if target == DRAW { continue; }
                        ctx.for_each_reverse(wm, sm, side, |pwm, psm, pside| {
                            let Some(pidx) = ctx.encode_ranked(pwm, psm, pside, ns, srank) else { return; };
                            if wdl_get(wdl, pidx) != target { return; }
                            let is_or = (target == WOLF_WIN && pside == WOLF) || (target == SHEEP_WIN && pside == SHEEP);
                            if is_or {
                                let cand = (d + 1) as u16;
                                let slot = &dist[pidx as usize];
                                let mut old = slot.load(Ordering::Relaxed);
                                while cand < old {
                                    match slot.compare_exchange_weak(old, cand, Ordering::AcqRel, Ordering::Relaxed) {
                                        Ok(_) => { local.push((cand, pidx as u32)); break; }
                                        Err(now) => old = now,
                                    }
                                }
                            } else {
                                let old = cnt[pidx as usize].fetch_sub(1, Ordering::AcqRel);
                                if old == 0 {
                                    duplicate_edges.fetch_add(1, Ordering::Relaxed);
                                } else if old == 1 {
                                    let cross = dist[pidx as usize].load(Ordering::Acquire);
                                    let pd = (cross.max(d as u16) as usize + 1) as u16;
                                    dist[pidx as usize].store(pd, Ordering::Release);
                                    local.push((pd, pidx as u32));
                                }
                            }
                        });
                    }
                    local
                }));
            }
            for h in handles { outputs.push(h.join().unwrap()); }
        });
        for local in outputs {
            for (nd, idx) in local {
                if add_candidate(&mut buckets, nd, idx).is_err() {
                    return Err(format!("s={s} propagation DTM exceeds {MAX_DTM}"));
                }
            }
        }
        if d % 8 == 0 {
            if let Some(avail) = available_ram_bytes() {
                if avail < MIN_FREE_RAM {
                    return Err(format!("RAM guard during s={s}, d={d}: only {:.2} GiB available",
                        avail as f64 / 1073741824.0));
                }
            }
        }
        if d % 32 == 0 {
            eprintln!("s={s} d={d} frontier={} queue_popped={} t={:.1}s", frontier.len(), total_popped, started.elapsed().as_secs_f64());
        }
    }
    if duplicate_edges.load(Ordering::Relaxed) != 0 {
        return Err(format!("s={s} duplicate predecessor notifications={}", duplicate_edges.load(Ordering::Relaxed)));
    }

    let mut max_d = 0u16;
    let mut max_or = 0u16;
    let mut unresolved = 0u64;
    let mut decisive = 0u64;
    for idx in 0..size {
        let v = wdl_get(&wdl, idx);
        if v == DRAW { continue; }
        decisive += 1;
        let d = dist[idx as usize].load(Ordering::Acquire);
        if d == INF {
            unresolved += 1;
        } else {
            max_d = max_d.max(d);
            if is_or_value(v, idx) { max_or = max_or.max(d); }
        }
    }
    if unresolved != 0 { return Err(format!("s={s} unresolved decisive states={unresolved}/{decisive}")); }
    if max_d as usize > MAX_DTM { return Err(format!("s={s} max DTM {max_d} over cap")); }
    eprintln!("s={s} solved decisive={decisive} max_dtm={max_d} max_bucket={max_processed} popped={total_popped} t={:.1}s", started.elapsed().as_secs_f64());

    verify_layer(ctx, s, threads, ns, &wdl, &dist, &srank, prev_wdl.as_deref(), prev_dtm.as_ref(), prev_srank.as_deref(), prev_ns)?;
    eprintln!("s={s} Bellman check passed; t={:.1}s", started.elapsed().as_secs_f64());
    drop(cnt);
    drop(prev_wdl);
    drop(prev_dtm);
    drop(prev_srank);
    drop(srank);
    let or_prefix = build_or_prefix(&wdl, size, threads);
    let or_count = *or_prefix.last().unwrap() as usize;
    let bits = (16 - max_or.leading_zeros() as u8).max(1);
    let packed_len = ((or_count as u64 * bits as u64 + 7) / 8) as usize;
    let root = workspace_root()?;
    let existing = tree_bytes(&root);
    let blocks = ((packed_len as u64 + (1 << 18) - 1) / (1 << 18)) + 1;
    let temp_upper = packed_len as u64 + packed_len as u64 / 500 + blocks * 64 + 4096;
    let projected = existing.saturating_add((or_prefix.len() * 4) as u64)
        .saturating_add(packed_len as u64).saturating_add(temp_upper);
    if projected > 15_000_000_000 {
        return Err(format!("15 GB workspace guard before s={s} output: projected {:.2} GB", projected as f64 / 1e9));
    }
    if let Some(avail) = available_ram_bytes() {
        let need = (or_count as u64).saturating_mul(2).saturating_add(packed_len as u64).saturating_add(MIN_FREE_RAM);
        if avail < need {
            return Err(format!("RAM guard before table packing: s={s} needs {:.2} GiB extra; only {:.2} GiB available",
                need as f64 / 1073741824.0, avail as f64 / 1073741824.0));
        }
    }
    let mut or_values = vec![0u16; or_count];
    let groups = or_prefix.len() - 1;
    let workers = threads.min(groups.max(1));
    std::thread::scope(|scope| {
        let mut rest = or_values.as_mut_slice();
        let blocks_per = (groups + workers - 1) / workers;
        for t in 0..workers {
            let blo = t * blocks_per;
            let bhi = ((t + 1) * blocks_per).min(groups);
            if blo >= bhi { continue; }
            let value_lo = or_prefix[blo] as usize;
            let value_hi = or_prefix[bhi] as usize;
            let take = value_hi - value_lo;
            let (mine, tail) = rest.split_at_mut(take);
            rest = tail;
            let (dist, wdl, prefix) = (&dist, &wdl, &or_prefix);
            scope.spawn(move || {
                let mut local_max = 0u16;
                for g in blo..bhi {
                    let start = g as u64 * OR_GROUP;
                    let end = (start + OR_GROUP).min(size);
                    let mut out = (prefix[g] - value_lo as u32) as usize;
                    for idx in start..end {
                        let v = wdl_get(wdl, idx);
                        if is_or_value(v, idx) {
                            let d = dist[idx as usize].load(Ordering::Relaxed);
                            mine[out] = d;
                            out += 1;
                            local_max = local_max.max(d);
                        }
                    }
                }
                local_max
            });
        }
    });

    if or_values.len() != or_count { return Err("OR-distance compaction count mismatch".into()); }
    let mut packed = vec![0u8; packed_len];
    let mut acc = 0u64;
    let mut fill = 0u32;
    let mut out_byte = 0usize;
    for &v in &or_values {
        acc |= (v as u64) << fill;
        fill += bits as u32;
        while fill >= 8 {
            packed[out_byte] = acc as u8;
            out_byte += 1;
            acc >>= 8;
            fill -= 8;
        }
    }
    if fill != 0 { packed[out_byte] = acc as u8; }
    drop(or_values);
    write_u32s(&format!("dtm_exact_s{s}.orpre"), &or_prefix)?;
    drop(or_prefix);
    drop(wdl);

    let raw_path = format!("dtm_exact_s{s}.packed");
    fs::write(&raw_path, &packed).map_err(|e| e.to_string())?;
    let final_path = format!("dtm_exact_s{s}.dtmz");
    compress_compact(&raw_path, &final_path, size, or_count, bits)?;
    pack_or_prefix(Path::new("."), s)?;
    let compressed_size = fs::metadata(&final_path).map(|x| x.len()).unwrap_or(0);
    eprintln!("s={s} exact OR table: {} values, {} bits/value, {} -> {} bytes; max DTM={max_d}; t={:.1}s",
        or_count, bits, packed_len, compressed_size, started.elapsed().as_secs_f64());
    drop(packed);
    let full_bits = (16 - max_d.leading_zeros() as u8).max(1);
    let full = if s < 15 {
        let full_len = ((size * full_bits as u64 + 7) / 8) as u64;
        if let Some(avail) = available_ram_bytes() {
            if avail < full_len.saturating_add(MIN_FREE_RAM) {
                return Err(format!("RAM guard before next-layer handoff: s={s} needs {:.2} GiB; only {:.2} GiB available",
                    (full_len + MIN_FREE_RAM) as f64 / 1073741824.0, avail as f64 / 1073741824.0));
            }
        }
        let data = pack_full_dist(&dist, size, full_bits, threads);
        eprintln!("s={s} in-memory next-layer handoff: {} bytes, {} bits/value", data.len(), full_bits);
        data
    } else { Vec::new() };
    drop(dist);
    Ok(FullDtm { packed: full, bits: full_bits })
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() { let _ = env::set_current_dir(dir); }
    }
    let get = |name: &str, default: usize| -> usize {
        args.iter().position(|x| x == name).and_then(|i| args.get(i + 1)).and_then(|x| x.parse().ok()).unwrap_or(default)
    };
    let from = get("--from", 4).max(4);
    let to = get("--to", 15).min(15);
    let threads = get("--threads", std::thread::available_parallelism().map(|x| x.get()).unwrap_or(8)).max(1);
    let ctx = Ctx::new();
    let t0 = Instant::now();
    let mut prev_full: Option<FullDtm> = None;
    for s in from..=to {
        match solve_layer(&ctx, s, threads, prev_full.take()) {
            Ok(full) => prev_full = Some(full),
            Err(e) => {
                eprintln!("FAILED s={s}: {e}");
                std::process::exit(2);
            }
        }
    }
    eprintln!("DONE {:.1}s", t0.elapsed().as_secs_f64());
}

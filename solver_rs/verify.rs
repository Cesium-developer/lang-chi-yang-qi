// 全表校验：逐个局面检查标签是否符合逆向传播的定义。
//
// 对每个局面（盘面 + 轮到谁走），检查：
//   轮到狼走：
//     标狼胜 -> 必须存在一个走法，走到「狼胜」的子局面
//     标羊胜 -> 必须所有走法都走到「羊胜」的子局面
//     标和棋 -> 不能存在「狼胜」子局面，也不能所有子局面都是「羊胜」
//   轮到羊走：
//     标羊胜 -> 必须存在一个走法，走到「羊胜」的子局面
//     标狼胜 -> 必须所有走法都走到「狼胜」的子局面
//     标和棋 -> 不能存在「羊胜」子局面，也不能所有子局面都是「狼胜」
//
// 子局面的归属：吃子会让羊数 -1，查下一层；移动留在本层。
//
// 用法： verify.exe [--threads 16]
use std::env;
mod wdl_storage;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use wdl_storage::load_wdl_layer;

const N: i32 = 5;
const CELLS: usize = 25;
const FREE: usize = 22;
const EMPTY: u8 = 0;
const SHEEP: u8 = 1;
const WOLF: u8 = 2;
const WW: u8 = 1;
const SW: u8 = 2;
const DINF: u8 = 0; // 和棋 / 未定

fn binom_table(n: usize) -> Vec<Vec<u64>> {
    let mut c = vec![vec![0u64; n + 1]; n + 1];
    for i in 0..=n {
        c[i][0] = 1;
        for j in 1..=i {
            let l = if j < i { c[i - 1][j] } else { 0 };
            c[i][j] = c[i - 1][j - 1] + l;
        }
    }
    c
}
fn unrank_comb(mut r: u64, k: usize, n: usize, b: &Vec<Vec<u64>>, out: &mut [usize]) {
    let mut c = n as i64 - 1;
    for i in (0..k).rev() {
        while c >= 0 && b[c as usize][i + 1] > r {
            c -= 1;
        }
        out[i] = c as usize;
        r -= b[c as usize][i + 1];
        c -= 1;
    }
}
#[inline]
fn rank_comb(cells: &[usize], b: &Vec<Vec<u64>>) -> u64 {
    let mut r = 0u64;
    for i in 0..cells.len() {
        r += b[cells[i]][i + 1];
    }
    r
}

struct Geo {
    neigh: [[i8; 4]; CELLS],
    jump: [[i8; 4]; CELLS],
}
fn build_geo() -> Geo {
    let mut g = Geo { neigh: [[-1; 4]; CELLS], jump: [[-1; 4]; CELLS] };
    for r in 0..N {
        for c in 0..N {
            let i = (r * N + c) as usize;
            let rr = [r - 1, r + 1, r, r];
            let cc = [c, c, c - 1, c + 1];
            let dd = [(-1, 0), (1, 0), (0, -1), (0, 1)];
            for d in 0..4 {
                let (nr, nc) = (rr[d], cc[d]);
                if nr >= 0 && nr < N && nc >= 0 && nc < N {
                    g.neigh[i][d] = (nr * N + nc) as i8;
                    let (jr, jc) = (nr + dd[d].0, nc + dd[d].1);
                    if jr >= 0 && jr < N && jc >= 0 && jc < N {
                        g.jump[i][d] = (jr * N + jc) as i8;
                    }
                }
            }
        }
    }
    g
}

struct Ctx {
    b: Vec<Vec<u64>>,
    g: Geo,
}

impl Ctx {
    #[inline]
    fn decode(&self, idx: u64, s: usize, board: &mut [u8; CELLS]) -> u8 {
        let ns = self.b[FREE][s];
        let side = (idx & 1) as u8;
        let t = idx >> 1;
        let wr = t / ns;
        let sr = t % ns;
        let mut wp = [0usize; 3];
        unrank_comb(wr, 3, CELLS, &self.b, &mut wp);
        for x in board.iter_mut() {
            *x = EMPTY;
        }
        for &p in wp.iter() {
            board[p] = WOLF;
        }
        let mut free = [0usize; FREE];
        let mut fi = 0;
        for i in 0..CELLS {
            if board[i] == EMPTY {
                free[fi] = i;
                fi += 1;
            }
        }
        let mut sp = [0usize; 16];
        unrank_comb(sr, s, FREE, &self.b, &mut sp[..s]);
        for k in 0..s {
            board[free[sp[k]]] = SHEEP;
        }
        side
    }
    #[inline]
    fn encode(&self, board: &[u8; CELLS], side: u8, s: usize) -> Option<u64> {
        let ns = self.b[FREE][s];
        let mut wp = [0usize; 3];
        let mut wc = 0;
        for i in 0..CELLS {
            if board[i] == WOLF {
                if wc >= 3 {
                    return None;
                }
                wp[wc] = i;
                wc += 1;
            }
        }
        if wc != 3 {
            return None;
        }
        let mut pos = [0usize; FREE];
        let mut fi = 0;
        for i in 0..CELLS {
            if board[i] != WOLF {
                pos[fi] = i;
                fi += 1;
            }
        }
        let mut sp = [0usize; 16];
        let mut sc = 0;
        for i in 0..CELLS {
            if board[i] == SHEEP {
                let mut lo = 0usize;
                while lo < FREE && pos[lo] != i {
                    lo += 1;
                }
                if lo >= FREE || sc >= 16 {
                    return None;
                }
                sp[sc] = lo;
                sc += 1;
            }
        }
        if sc != s {
            return None;
        }
        Some((rank_comb(&wp, &self.b) * ns + rank_comb(&sp[..s], &self.b)) * 2 + side as u64)
    }
}

#[inline]
fn getv(data: &[u8], idx: u64) -> u8 {
    (data[(idx >> 2) as usize] >> ((idx & 3) * 2)) & 3
}

fn main() {
    let a: Vec<String> = env::args().collect();
    let getv2 = |name: &str, def: u64| -> u64 {
        let mut it = a.iter();
        while let Some(x) = it.next() {
            if x == name {
                return it.next().and_then(|y| y.parse().ok()).unwrap_or(def);
            }
        }
        def
    };
    let nthreads = getv2("--threads", std::thread::available_parallelism().map(|x| x.get() as u64).unwrap_or(8)) as usize;
    let ctx = Ctx { b: binom_table(CELLS), g: build_geo() };
    let t0 = Instant::now();
    println!("verify: threads={}", nthreads);

    let mut prev: Vec<u8> = Vec::new();
    let mut prev_s = 0usize;

    for s in 4..=15usize {
        let raw_path = format!("wdl_s{}.bin", s);
        let packed_path = format!("wdl_s{}.wdlz", s);
        if !Path::new(&raw_path).exists() && !Path::new(&packed_path).exists() {
            println!("(no WDL layer s={}, stop)", s);
            break;
        }
        let cur = load_wdl_layer(Path::new("."), s).unwrap();
        let size = 2300 * ctx.b[FREE][s] * 2;
        println!("layer s={} states={} t={:.1}s", s, size, t0.elapsed().as_secs_f64());

        let bad = AtomicU64::new(0);
        let checked = AtomicU64::new(0);
        let samples: Mutex<Vec<String>> = Mutex::new(Vec::new());

        let per = (size + nthreads as u64 - 1) / nthreads as u64;
        std::thread::scope(|sc| {
            for t in 0..nthreads {
                let lo = t as u64 * per;
                let hi = ((t as u64 + 1) * per).min(size);
                if lo >= hi {
                    continue;
                }
                let cur = &cur;
                let prev = &prev;
                let ctxr = &ctx;
                let bad = &bad;
                let checked = &checked;
                let samples = &samples;
                sc.spawn(move || {
                    let mut board = [EMPTY; CELLS];
                    let mut nb = [EMPTY; CELLS];
                    let mut local_bad: Vec<String> = Vec::new();
                    let mut n_local = 0u64;
                    for idx in lo..hi {
                        let side = ctxr.decode(idx, s, &mut board);
                        let v = getv(cur, idx);
                        // 生成走法
                        let me = if side == 0 { WOLF } else { SHEEP };
                        let mut cnt = 0u32;
                        let mut has_ww = false;
                        let mut has_sw = false;
                        let mut all_ww = true;
                        let mut all_sw = true;
                        for i in 0..CELLS {
                            if board[i] != me {
                                continue;
                            }
                            for d in 0..4 {
                                let nn = ctxr.g.neigh[i][d];
                                if nn < 0 {
                                    continue;
                                }
                                let ni = nn as usize;
                                if board[ni] != EMPTY {
                                    continue;
                                }
                                // 移动
                                nb = board;
                                nb[i] = EMPTY;
                                nb[ni] = me;
                                if let Some(c) = ctxr.encode(&nb, 1 - side, s) {
                                    let cv = getv(cur, c);
                                    cnt += 1;
                                    if cv == WW {
                                        has_ww = true;
                                    }
                                    if cv == SW {
                                        has_sw = true;
                                    }
                                    if cv != WW {
                                        all_ww = false;
                                    }
                                    if cv != SW {
                                        all_sw = false;
                                    }
                                }
                                // 吃子（只有狼）
                                if side == 0 {
                                    let j = ctxr.g.jump[i][d];
                                    if j >= 0 && board[j as usize] == SHEEP {
                                        nb = board;
                                        nb[i] = EMPTY;
                                        nb[j as usize] = WOLF;
                                        if s == 4 {
                                            // 吃子后羊剩 3 只 = 终局狼胜，不在表里
                                            cnt += 1;
                                            has_ww = true;
                                            all_sw = false;
                                        } else if let Some(c) = ctxr.encode(&nb, 1, s - 1) {
                                            let cv = getv(prev, c);
                                            cnt += 1;
                                            if cv == WW {
                                                has_ww = true;
                                            }
                                            if cv == SW {
                                                has_sw = true;
                                            }
                                            if cv != WW {
                                                all_ww = false;
                                            }
                                            if cv != SW {
                                                all_sw = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // 判定
                        let ok = if side == 0 {
                            // 狼层：无走法 = 羊胜
                            if cnt == 0 {
                                v == SW
                            } else {
                                match v {
                                    WW => has_ww,
                                    SW => all_sw,
                                    _ => !has_ww && !all_sw,
                                }
                            }
                        } else {
                            match v {
                                SW => has_sw,
                                WW => all_ww,
                                _ => !has_sw && !all_ww,
                            }
                        };
                        if !ok {
                            if n_local < 5 {
                                local_bad.push(format!(
                                    "s={} idx={} side={} val={} cnt={} hasW={} hasS={} allW={} allS={}",
                                    s, idx, side, v, cnt, has_ww, has_sw, all_ww, all_sw
                                ));
                            }
                            n_local += 1;
                        }
                    }
                    if n_local > 0 {
                        bad.fetch_add(n_local, Ordering::Relaxed);
                        samples.lock().unwrap().extend(local_bad);
                    }
                    checked.fetch_add(hi - lo, Ordering::Relaxed);
                });
            }
        });
        println!("  s={} checked={} BAD={} t={:.1}s",
            s, checked.load(Ordering::Relaxed), bad.load(Ordering::Relaxed),
            t0.elapsed().as_secs_f64());
        let sm = samples.lock().unwrap();
        for line in sm.iter().take(5) {
            println!("    {}", line);
        }
        drop(sm);
        if bad.load(Ordering::Relaxed) > 0 {
            println!("STOP: layer s={} has inconsistencies, table is WRONG", s);
            return;
        }
        prev = cur;
        prev_s = s;
        let _ = prev_s;
    }
    println!("ALL LAYERS CONSISTENT. t={:.1}s", t0.elapsed().as_secs_f64());
}

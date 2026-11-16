// 逆向分析（WDL）多线程版：按羊数分层，层内 BSP 轮次并行传播。
//
// 层 s：吃子 → s-1（已算完），移动留本层；层内环 → 和棋。
// 层内下标 idx = (wolf_rank * C(22,s) + sheep_rank) * 2 + side
//
// 并行结构（每层三阶段，用 std::thread::scope 分块）：
//   A 播种     每线程只写自己那段，安全
//   B 计数器   读全局只读的值，写自己那段
//   C 传播     按轮：frontier 切块给各线程，结果汇总成下一轮 frontier
//
// 用法： retro5.exe --sheep 15 --lose 4 [--time 18000] [--threads 8]
use std::env;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const N: i32 = 5;
const CELLS: usize = 25;
const FREE: usize = 22;
const EMPTY: u8 = 0;
const SHEEP: u8 = 1;
const WOLF: u8 = 2;
const WS: usize = 0;
const SS: usize = 1;
const UNK: u8 = 0;
#[inline(always)] fn peek2(data:&[u8],idx:u64)->u8{(data[(idx>>2) as usize]>>((idx&3)*2))&3}
const WW: u8 = 1;
const SW: u8 = 2;

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
#[inline]
fn rank_comb(cells: &[usize], b: &Vec<Vec<u64>>) -> u64 {
    let mut r = 0u64;
    for i in 0..cells.len() {
        r += b[cells[i]][i + 1];
    }
    r
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
    fn decode(&self, idx: u64, s: usize, ns: u64, board: &mut [u8; CELLS]) -> u8 {
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
        let wr = rank_comb(&wp, &self.b);
        let sr = rank_comb(&sp[..s], &self.b);
        Some((wr * ns + sr) * 2 + side as u64)
    }
}

fn init_board() -> [u8; CELLS] {
    let rows = [
        [1u8, 1, 1, 1, 1],
        [1, 1, 1, 1, 1],
        [1, 1, 1, 1, 1],
        [0, 0, 0, 0, 0],
        [0, 2, 2, 2, 0],
    ];
    let mut b = [EMPTY; CELLS];
    for r in 0..5 {
        for c in 0..5 {
            b[r * 5 + c] = rows[r][c];
        }
    }
    b
}

// 把 [0,size) 切成 n 段
fn chunks(size: u64, n: usize) -> Vec<(u64, u64)> {
    let mut v = Vec::with_capacity(n);
    let per = (size + n as u64 - 1) / n as u64;
    let mut i = 0u64;
    while i < size {
        let e = (i + per).min(size);
        v.push((i, e));
        i = e;
    }
    v
}

fn main() {
    let a: Vec<String> = env::args().collect();
    let getv = |name: &str, def: u64| -> u64 {
        let mut it = a.iter();
        while let Some(x) = it.next() {
            if x == name {
                return it.next().and_then(|y| y.parse().ok()).unwrap_or(def);
            }
        }
        def
    };
    let ns0 = getv("--sheep", 15) as usize;
    let lose = getv("--lose", 4) as usize;
    let time_cap = getv("--time", 18000) as f64;
    let nthreads = getv("--threads", std::thread::available_parallelism().map(|x| x.get() as u64).unwrap_or(8)) as usize;
    let ctx = Ctx { b: binom_table(CELLS), g: build_geo() };
    let t0 = Instant::now();

    let mut total = 0u64;
    for s in lose..=ns0 {
        total += 2300 * ctx.b[FREE][s] * 2;
    }
    println!("retro5 threads={} sheep {}..{} lose<{} total {} ({:.2} GiB)",
        nthreads, lose, ns0, lose, total, total as f64 / 1073741824.0);

    let ib = init_board();
    let init_s: usize = ib.iter().filter(|&&x| x == SHEEP).count();
    let mut prev_w: Vec<u8> = Vec::new();
    let mut prev_size: u64 = 0;

    for s in lose..=ns0 {
        let nsheep = ctx.b[FREE][s];
        let size = 2300 * nsheep * 2;
        let val: Vec<AtomicU8> = (0..size).map(|_| AtomicU8::new(UNK)).collect();
        let cnt: Vec<AtomicU8> = (0..size).map(|_| AtomicU8::new(0)).collect();
        let ranges = chunks(size, nthreads);

        for target in [WW, SW] {
            // ---------- A. 播种 ----------
            std::thread::scope(|sc| {
                for &(lo, hi) in ranges.iter() {
                    let val = &val;
                    let ctxr = &ctx;
                    let prev_w_r = &prev_w;
                    sc.spawn(move || {
                        let mut board = [EMPTY; CELLS];
                        let mut nb: [u8; CELLS];
                        for idx in lo..hi {
                            let side = ctxr.decode(idx, s, nsheep, &mut board);
                            if side as usize != WS {
                                continue;
                            }
                            // 狼的合法走法（含吃子）
                            let mut has_move = false;
                            let mut cap_win = false;
                            'outer: for i in 0..CELLS {
                                if board[i] != WOLF {
                                    continue;
                                }
                                for d in 0..4 {
                                    let n = ctxr.g.neigh[i][d];
                                    if n < 0 {
                                        continue;
                                    }
                                    let ni = n as usize;
                                    if board[ni] != EMPTY {
                                        continue;
                                    }
                                    has_move = true;
                                    // 吃子？
                                    let j = ctxr.g.jump[i][d];
                                    if j < 0 || board[j as usize] != SHEEP {
                                        continue;
                                    }
                                    if target == WW {
                                        if s == lose {
                                            cap_win = true;
                                            break 'outer;
                                        }
                                        nb = board;
                                        nb[i] = EMPTY;
                                        nb[j as usize] = WOLF;
                                        if let Some(c) = ctxr.encode(&nb, SS as u8, s - 1) {
                                            if c < prev_size && peek2(prev_w_r, c) == 1 {
                                                cap_win = true;
                                                break 'outer;
                                            }
                                        }
                                    }
                                }
                            }
                            if target == SW && !has_move {
                                val[idx as usize].store(SW, Ordering::Relaxed);
                            } else if target == WW && cap_win {
                                val[idx as usize].store(WW, Ordering::Relaxed);
                            }
                        }
                    });
                }
            });

            // ---------- B. AND 节点计数器 ----------
            std::thread::scope(|sc| {
                for &(lo, hi) in ranges.iter() {
                    let val = &val;
                    let cnt = &cnt;
                    let ctxr = &ctx;
                    let prev_r: &Vec<u8> = &prev_w;
                    sc.spawn(move || {
                        let mut board = [EMPTY; CELLS];
                        let mut nb: [u8; CELLS];
                        let mut mv: Vec<(usize, usize)> = Vec::new();
                        for idx in lo..hi {
                            if val[idx as usize].load(Ordering::Relaxed) != UNK {
                                continue;
                            }
                            let side = ctxr.decode(idx, s, nsheep, &mut board);
                            let and_node = if target == WW {
                                side as usize == SS
                            } else {
                                side as usize == WS
                            };
                            if !and_node {
                                cnt[idx as usize].store(255, Ordering::Relaxed); // 标记：非 AND 节点
                                continue;
                            }
                            mv.clear();
                            let me = if side as usize == WS { WOLF } else { SHEEP };
                            for i in 0..CELLS {
                                if board[i] != me {
                                    continue;
                                }
                                for d in 0..4 {
                                    let n = ctxr.g.neigh[i][d];
                                    if n < 0 {
                                        continue;
                                    }
                                    let ni = n as usize;
                                    if board[ni] == EMPTY {
                                        mv.push((i, ni));
                                    }
                                }
                            }
                            let child_side = if side as usize == WS { SS } else { WS };
                            // 同层孩子全部计数：传播阶段每个孩子变成目标值时都会触发一次递减，
                            // 所以初值必须是「孩子总数」，不能只数「还没达成的」。
                            let mut c = 0u32;
                            for &(f, t) in mv.iter() {
                                nb = board;
                                nb[f] = EMPTY;
                                nb[t] = me;
                                if ctxr.encode(&nb, child_side as u8, s).is_some() {
                                    c += 1;
                                }
                            }
                            // 跨层吃子（只有狼层有）：这些孩子不会被同层传播通知，
                            // 若它们不是目标值，就是永远达不成，必须计入。
                            if side as usize == WS {
                                for i in 0..CELLS {
                                    if board[i] != WOLF {
                                        continue;
                                    }
                                    for d in 0..4 {
                                        let n2 = ctxr.g.neigh[i][d];
                                        if n2 < 0 {
                                            continue;
                                        }
                                        let ni2 = n2 as usize;
                                        if board[ni2] != EMPTY {
                                            continue;
                                        }
                                        let j2 = ctxr.g.jump[i][d];
                                        if j2 < 0 || board[j2 as usize] != SHEEP {
                                            continue;
                                        }
                                        let cv = if s == lose {
                                            1u8
                                        } else {
                                            nb = board;
                                            nb[i] = EMPTY;
                                            nb[j2 as usize] = WOLF;
                                            match ctxr.encode(&nb, SS as u8, s - 1) {
                                                Some(ci) if ci < prev_size => peek2(prev_r, ci),
                                                _ => 0u8,
                                            }
                                        };
                                        if cv != target {
                                            c += 1;
                                        }
                                    }
                                }
                            }
                            if mv.is_empty() {
                                cnt[idx as usize].store(0, Ordering::Relaxed);
                            } else {
                                cnt[idx as usize].store(c.min(254) as u8, Ordering::Relaxed);
                            }
                        }
                    });
                }
            });

            // ---------- B2. 计数器为 0 的 AND 节点直接定值 ----------
            std::thread::scope(|sc| {
                for &(lo, hi) in ranges.iter() {
                    let val = &val;
                    let cnt = &cnt;
                    sc.spawn(move || {
                        for idx in lo..hi {
                            if val[idx as usize].load(Ordering::Relaxed) == UNK {
                                let c = cnt[idx as usize].load(Ordering::Relaxed);
                                if c == 0 {
                                    let _ = val[idx as usize].compare_exchange(
                                        UNK, target, Ordering::Relaxed, Ordering::Relaxed);
                                }
                            }
                        }
                    });
                }
            });

            // ---------- C. 轮次并行传播 ----------
            // 初始 frontier = 已确定为目标值的所有状态
            let mut frontier: Vec<u32> = Vec::new();
            {
                let mut locals: Vec<Vec<u32>> = (0..nthreads).map(|_| Vec::new()).collect();
                std::thread::scope(|sc| {
                    let handles: Vec<_> = ranges
                        .iter()
                        .enumerate()
                        .map(|(_, &(lo, hi))| {
                            let val = &val;
                            sc.spawn(move || {
                                let mut v = Vec::new();
                                for idx in lo..hi {
                                    if val[idx as usize].load(Ordering::Relaxed) == target {
                                        v.push(idx as u32);
                                    }
                                }
                                v
                            })
                        })
                        .collect();
                    for (t, h) in handles.into_iter().enumerate() {
                        locals[t] = h.join().unwrap();
                    }
                });
                for v in locals {
                    frontier.extend(v);
                }
            }
            println!("  s={} target={} seeds={} t={:.1}s",
                s, if target == WW { "W" } else { "S" }, frontier.len(),
                t0.elapsed().as_secs_f64());

            let mut rounds = 0u32;
            while !frontier.is_empty() {
                rounds += 1;
                if t0.elapsed().as_secs_f64() > time_cap {
                    println!("ABORT s={} target={} rounds={} t={:.1}s", s, target, rounds,
                        t0.elapsed().as_secs_f64());
                    return;
                }
                let ch = chunks(frontier.len() as u64, nthreads);
                let nexts: Vec<Mutex<Vec<u32>>> = (0..nthreads).map(|_| Mutex::new(Vec::new())).collect();
                std::thread::scope(|sc| {
                    for (t, &(lo, hi)) in ch.iter().enumerate() {
                        let val = &val;
                        let cnt = &cnt;
                        let ctxr = &ctx;
                        let fr = &frontier;
                        let nx = &nexts[t];
                        let tgt = target;
                        sc.spawn(move || {
                            let mut board = [EMPTY; CELLS];
                            let mut nb: [u8; CELLS];
                            let mut local: Vec<u32> = Vec::new();
                            for fi in lo..hi {
                                let u = fr[fi as usize] as usize;
                                if val[u].load(Ordering::Relaxed) != tgt {
                                    continue;
                                }
                                let uside = ctxr.decode(u as u64, s, nsheep, &mut board);
                                // 前驱：上一手是 1-uside 那一方走的
                                let me = if uside as usize == WS { SHEEP } else { WOLF };
                                let pside = if uside as usize == WS { SS } else { WS };
                                let p_and = if tgt == WW {
                                    pside as usize == SS
                                } else {
                                    pside as usize == WS
                                };
                                for b in 0..CELLS {
                                    if board[b] != me {
                                        continue;
                                    }
                                    for d in 0..4 {
                                        let n = ctxr.g.neigh[b][d];
                                        if n < 0 {
                                            continue;
                                        }
                                        let ai = n as usize;
                                        if board[ai] != EMPTY {
                                            continue;
                                        }
                                        nb = board;
                                        nb[b] = EMPTY;
                                        nb[ai] = me;
                                        let c = match ctxr.encode(&nb, pside as u8, s) {
                                            Some(c) => c,
                                            None => continue,
                                        };
                                        if val[c as usize].load(Ordering::Relaxed) != UNK {
                                            continue;
                                        }
                                        if !p_and {
                                            if val[c as usize]
                                                .compare_exchange(UNK, tgt, Ordering::Relaxed, Ordering::Relaxed)
                                                .is_ok()
                                            {
                                                local.push(c as u32);
                                            }
                                        } else {
                                            let old = cnt[c as usize]
                                                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
                                                    if x > 0 { Some(x - 1) } else { None }
                                                })
                                                .unwrap_or(0);
                                            if old == 1 {
                                                if val[c as usize]
                                                    .compare_exchange(UNK, tgt, Ordering::Relaxed, Ordering::Relaxed)
                                                    .is_ok()
                                                {
                                                    local.push(c as u32);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            *nx.lock().unwrap() = local;
                        });
                    }
                });
                frontier.clear();
                for m in nexts.iter() {
                    let v = m.lock().unwrap();
                    frontier.extend(v.iter().copied());
                }
            }

            // 统计
            let (mut cw, mut cs, mut cd) = (0u64, 0u64, 0u64);
            for x in val.iter() {
                match x.load(Ordering::Relaxed) {
                    WW => cw += 1,
                    SW => cs += 1,
                    _ => cd += 1,
                }
            }
            println!("s={} target={} rounds={} W={} S={} D={} t={:.1}s",
                s, if target == WW { "W" } else { "S" }, rounds, cw, cs, cd,
                t0.elapsed().as_secs_f64());
        }

        if a.iter().any(|x| x == "--save") {
            // 2 bit/局面打包落盘，每层一个文件
            let nb = ((size + 3) / 4) as usize;
            let mut buf = vec![0u8; nb];
            for i in 0..size as usize {
                let v = val[i].load(Ordering::Relaxed) & 3;
                buf[i >> 2] |= v << ((i & 3) * 2);
            }
            let path = format!("wdl_s{}.bin", s);
            std::fs::write(&path, &buf).unwrap();
            println!("saved {} ({} bytes)", path, nb);
            drop(buf);
        }
        if a.iter().any(|x| x == "--dump") {
            let mut out = Vec::with_capacity(size as usize);
            for i in 0..size as usize { out.push(val[i].load(Ordering::Relaxed)); }
            std::fs::write(format!("dump_s{}.bin", s), &out).unwrap();
            println!("dumped s={} ({} bytes)", s, out.len());
        }
        if s == init_s {
            if let Some(idx) = ctx.encode(&ib, WS as u8, s) {
                println!(">>> INITIAL = {}", match val[idx as usize].load(Ordering::Relaxed) {
                    WW => "WOLF_WIN", SW => "SHEEP_WIN", _ => "DRAW",
                });
            }
        }

        prev_w = vec![0u8; ((size + 3) / 4) as usize];
        for i in 0..size as usize {
            let v = val[i].load(Ordering::Relaxed) & 3;
            prev_w[i >> 2] |= v << ((i & 3) * 2);
        }
        prev_size = size;
    }
    println!("done t={:.1}s", t0.elapsed().as_secs_f64());
}

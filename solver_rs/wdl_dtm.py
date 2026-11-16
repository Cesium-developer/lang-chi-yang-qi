"""带距离（DTM）的查询：用 WDL 表剪枝，DFS 现算「到终局还有多少步」。

WDL 表只给胜负，不给距离。直接用会出现「始终选一个仍然必胜、
但绕路的走法」→ 三次重复判和。这里补上距离：

  dtm(局面) = 0                      终局
            = 1 + min(必胜子局面)     轮到狼（狼要快）
            = 1 + max(全部子局面)     轮到羊（羊要拖）
  回到自己（成环）→ 无穷大，说明这条路赢不了，弃掉

然后每一步选「距离最小的必胜子局面」，就能保证严格缩短，不会重复。
"""
import mmap
import os
import struct
from math import comb
from table_storage import WdlzReader

HERE = os.path.dirname(os.path.abspath(__file__))
N = 5
CELLS = 25
FREE = 22
EMPTY, SHEEP, WOLF = 0, 1, 2
INF = 10 ** 9

_maps = {}


def _region(s):
    """mmap 一层的表文件。"""
    if s in _maps:
        return _maps[s]
    raw_path = os.path.join(HERE, "wdl_s%d.bin" % s)
    packed_path = os.path.join(HERE, "wdl_s%d.wdlz" % s)
    if os.path.exists(raw_path):
        f = open(raw_path, "rb")
        m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
    elif os.path.exists(packed_path):
        f = WdlzReader(packed_path)
        m = f
    else:
        return None
    _maps[s] = (f, m)
    return _maps[s]


def rank_comb(cells):
    r = 0
    for i, c in enumerate(cells):
        r += comb(c, i + 1)
    return r


def unrank_comb(r, k, n):
    out = [0] * k
    c = n - 1
    for i in range(k - 1, -1, -1):
        while c >= 0 and comb(c, i + 1) > r:
            c -= 1
        out[i] = c
        r -= comb(c, i + 1)
        c -= 1
    return out


def _layout(board):
    """返回 (s, wolf 列表, sheep 本地位置列表) 或 (s, None, None)。"""
    wolf, sheep = [], []
    for r in range(N):
        for c in range(N):
            v = board[r][c]
            if v == WOLF:
                wolf.append(r * N + c)
            elif v == SHEEP:
                sheep.append(r * N + c)
    if len(wolf) != 3:
        return None, None, None
    s = len(sheep)
    if s < 4 or s > 15:
        return s, None, None
    wset = set(wolf)
    free = [i for i in range(CELLS) if i not in wset]
    pos = {cell: k for k, cell in enumerate(free)}
    return s, sorted(wolf), sorted(pos[c] for c in sheep)


def value(board, side):
    """查表：0 和 / 1 狼胜 / 2 羊胜 / None 不在表内。"""
    s, wolf, sh = _layout(board)
    if wolf is None:
        return None
    reg = _region(s)
    if reg is None:
        return None
    idx = (rank_comb(wolf) * comb(FREE, s) + rank_comb(sh)) * 2 + side
    m = reg[1]
    b = m[idx >> 2]
    return (b >> ((idx & 3) * 2)) & 3


def moves(board, side):
    out = []
    me = WOLF if side == 0 else SHEEP
    for r in range(N):
        for c in range(N):
            if board[r][c] != me:
                continue
            for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
                mr, mc = r + dr, c + dc
                if not (0 <= mr < N and 0 <= mc < N):
                    continue
                if board[mr][mc] != EMPTY:
                    continue
                out.append((r, c, mr, mc))
                if side == 0:
                    tr, tc = r + 2 * dr, c + 2 * dc
                    if 0 <= tr < N and 0 <= tc < N and board[tr][tc] == SHEEP:
                        out.append((r, c, tr, tc))
    return out


def apply(board, m):
    fr, fc, tr, tc = m
    nb = [row[:] for row in board]
    nb[tr][tc] = nb[fr][fc]
    nb[fr][fc] = EMPTY
    return nb


def key(board, side):
    return (tuple(tuple(r) for r in board), side)


def dtm(board, side, memo, stack, node_cap, counter):
    """返回 (距离, 走法)。距离 INF 表示赢不了（含成环）。"""
    k = key(board, side)
    if k in memo:
        return memo[k]
    counter[0] += 1
    if counter[0] > node_cap:
        raise RuntimeError("node cap")
    v = value(board, side)
    if v is None:
        return INF, None
    # 终局：羊 < 4
    sheep_cnt = sum(x == SHEEP for r in board for x in r)
    if sheep_cnt < 4:
        memo[k] = (0, None)
        return memo[k]
    if side == 0 and not moves(board, 0):
        # 狼无路可走 → 羊胜，狼这条线赢不了
        memo[k] = (INF, None)
        return memo[k]
    if k in stack:
        return INF, None  # 成环
    stack.add(k)
    win = 1 if side == 0 else 2  # 谁想看谁赢：狼层看 1，羊层看 2？
    # 实际：我们要的是「狼胜」的距离，所以狼层只走 value==1 的子；
    # 羊层要求所有子都是 1，取最大。
    best_d = INF
    best_m = None
    mv = moves(board, side)
    if side == 0:
        # 狼层：只考虑保持「狼胜」的走法，取距离最小
        for m in mv:
            nb = apply(board, m)
            if value(nb, 1) != 1:
                continue
            c = sum(x == SHEEP for r in nb for x in r)
            if c < 4:
                d = 1
            else:
                d, _ = dtm(nb, 1, memo, stack, node_cap, counter)
                if d >= INF:
                    continue
                d += 1
            if d < best_d:
                best_d, best_m = d, m
    else:
        # 羊层：所有子都必须是狼胜，距离取最大
        ok = True
        for m in mv:
            nb = apply(board, m)
            if value(nb, 0) != 1:
                ok = False
                break
            c = sum(x == SHEEP for r in nb for x in r)
            if c < 4:
                d = 1
            else:
                d, _ = dtm(nb, 0, memo, stack, node_cap, counter)
                if d >= INF:
                    ok = False
                    break
                d += 1
            if d > best_d or best_d == INF:
                best_d, best_m = d, m
        if not ok:
            best_d, best_m = INF, None
    stack.discard(k)
    memo[k] = (best_d, best_m)
    return memo[k]

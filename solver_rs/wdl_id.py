"""带距离的查询：迭代加深 + WDL 表剪枝。

为什么不用 DFS 直接算距离：环是「路径相关」的，同一局面从不同路径到达，
能不能靠环形取胜完全不同，普通记忆化会算错。改成有界深度：
  win_within(局面, 深度) =
      终局狼胜                     -> True
      深度用完                      -> False
      轮到狼 (OR)                  -> 存在一个子局面：表里是狼胜 且 win_within(子, 深度-1)
      轮到羊 (AND)                 -> 全部子局面：表里是狼胜 且 win_within(子, 深度-1)
深度从 1 往上加，第一次为真的深度就是「最少需要多少步」。

这样每一步都严格缩短剩余步数，不会自己走成重复判和。
"""
import mmap
import os
from math import comb
from table_storage import WdlzReader

HERE = os.path.dirname(os.path.abspath(__file__))
N = 5
CELLS = 25
FREE = 22
EMPTY, SHEEP, WOLF = 0, 1, 2

_maps = {}


def _region(s):
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
    s, wolf, sh = _layout(board)
    if wolf is None:
        return None
    reg = _region(s)
    if reg is None:
        return None
    idx = (rank_comb(wolf) * comb(FREE, s) + rank_comb(sh)) * 2 + side
    m = reg[1]
    return (m[idx >> 2] >> ((idx & 3) * 2)) & 3


def moves(board, side):
    out = []
    me = WOLF if side == 0 else SHEEP
    for r in range(N):
        for c in range(N):
            if board[r][c] != me:
                continue
            for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
                mr, mc = r + dr, c + dc
                if not (0 <= mr < N and 0 <= mc < N) or board[mr][mc] != EMPTY:
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
    return (tuple(map(tuple, board)), side)


def win_within(board, side, depth, memo, counter):
    """狼能否在 depth 步内强制取胜（WDL 表剪枝）。"""
    k = (key(board, side), depth)
    if k in memo:
        return memo[k]
    counter[0] += 1
    sheep_cnt = sum(x == SHEEP for r in board for x in r)
    if sheep_cnt < 4:
        memo[k] = True
        return True
    if depth == 0:
        memo[k] = False
        return False
    if side == 0 and not moves(board, 0):
        memo[k] = False
        return False
    res = False
    mv = moves(board, side)
    if side == 0:
        for m in mv:
            nb = apply(board, m)
            if value(nb, 1) != 1:
                continue
            if win_within(nb, 1, depth - 1, memo, counter):
                res = True
                break
    else:
        res = True
        for m in mv:
            nb = apply(board, m)
            if value(nb, 0) != 1 or not win_within(nb, 0, depth - 1, memo, counter):
                res = False
                break
    memo[k] = res
    return res


def best_move_dtm(board, side, max_depth=60, node_cap=4_000_000):
    """返回 (最少步数, 走法)。"""
    counter = [0]
    for d in range(1, max_depth + 1):
        memo = {}
        try:
            if win_within(board, side, d, memo, counter):
                # 在这一深度下挑一个成立的走法
                mv = moves(board, side)
                if side == 0:
                    for m in mv:
                        nb = apply(board, m)
                        if value(nb, 1) == 1 and win_within(nb, 1, d - 1, memo, counter):
                            return d, m, counter[0]
                else:
                    for m in mv:
                        nb = apply(board, m)
                        if value(nb, 0) == 1:
                            ok = True
                            for m2 in moves(nb, 0):
                                nb2 = apply(nb, m2)
                                if value(nb2, 1) != 1 or not win_within(nb2, 1, d - 2, memo, counter):
                                    ok = False
                                    break
                            if ok:
                                return d, m, counter[0]
        except RecursionError:
            pass
        if counter[0] > node_cap:
            return None, None, counter[0]
    return None, None, counter[0]

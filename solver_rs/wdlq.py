"""查询全量 WDL 表。

用法：
    python wdlq.py                  # 查初始局面，并打印一条最优走法
    python wdlq.py "1 1 1 1 1 / 1 1 1 1 1 / 1 1 1 1 1 / 0 0 0 0 0 / 0 2 2 2 0" wolf

表：wdl_s{羊数}.wdlz（分块压缩，可随机读取），每局面 2 bit（0=和, 1=狼胜, 2=羊胜），
   层内下标 idx = (wolf_rank * C(22,s) + sheep_rank) * 2 + side。
"""
import os
import sys
from math import comb
from table_storage import WdlzReader

HERE = os.path.dirname(os.path.abspath(__file__))
N = 5
CELLS = 25
FREE = 22
EMPTY, SHEEP, WOLF = 0, 1, 2


def rank_comb(cells):
    """k 个升序位置（取自 0..n）的组合排名。"""
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


def board_index(board, side):
    """返回 (羊数 s, 层内下标 idx)，越界返回 (s, None)。"""
    wolf, sheep = [], []
    for r in range(N):
        for c in range(N):
            v = board[r][c]
            if v == WOLF:
                wolf.append(r * N + c)
            elif v == SHEEP:
                sheep.append(r * N + c)
    if len(wolf) != 3:
        return None, None
    s = len(sheep)
    if s < 4 or s > 15:
        return s, None
    free = [i for i in range(CELLS) if i not in set(wolf)]
    pos = {cell: k for k, cell in enumerate(free)}
    sheep_local = sorted(pos[c] for c in sheep)
    wr = rank_comb(sorted(wolf))
    sr = rank_comb(sheep_local)
    return s, (wr * comb(FREE, s) + sr) * 2 + side


def lookup(board, side):
    s, idx = board_index(board, side)
    if idx is None:
        return None
    raw_path = os.path.join(HERE, "wdl_s%d.bin" % s)
    packed_path = os.path.join(HERE, "wdl_s%d.wdlz" % s)
    if os.path.exists(raw_path):
        with open(raw_path, "rb") as f:
            f.seek(idx >> 2)
            b = f.read(1)[0]
    elif os.path.exists(packed_path):
        table = WdlzReader(packed_path)
        try:
            b = table[idx >> 2]
        finally:
            table.close()
    else:
        raise SystemExit("缺少表文件 %s 或 %s" % (raw_path, packed_path))
    return (b >> ((idx & 3) * 2)) & 3


def decode_idx(s, idx):
    """层内下标 -> (board, side)"""
    side = idx & 1
    t = idx >> 1
    ns = comb(FREE, s)
    wr, sr = t // ns, t % ns
    wolf = unrank_comb(wr, 3, CELLS)
    board = [[EMPTY] * N for _ in range(N)]
    for p in wolf:
        board[p // N][p % N] = WOLF
    free = [i for i in range(CELLS) if board[i // N][i % N] == EMPTY]
    for k in unrank_comb(sr, s, FREE):
        cell = free[k]
        board[cell // N][cell % N] = SHEEP
    return board, side


def moves(board, side):
    """返回 [(from_r, from_c, to_r, to_c, is_capture)]，与引擎同构。"""
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
                out.append((r, c, mr, mc, False))
                if side == 0:
                    tr, tc = r + 2 * dr, c + 2 * dc
                    if 0 <= tr < N and 0 <= tc < N and board[tr][tc] == SHEEP:
                        out.append((r, c, tr, tc, True))
    return out


def apply(board, m):
    fr, fc, tr, tc, _ = m
    nb = [row[:] for row in board]
    nb[tr][tc] = nb[fr][fc]
    nb[fr][fc] = EMPTY
    return nb


def best_move(board, side):
    """在所有保持当前值的走法里随便挑一条。"""
    cur = lookup(board, side)
    if cur is None:
        return None
    keep = cur
    for m in moves(board, side):
        nb = apply(board, m)
        v = lookup(nb, 1 - side)
        if v is None:
            continue
        if v == keep:
            return m
    return None


NAMES = {0: "和棋", 1: "狼胜", 2: "羊胜"}
INITIAL = [[1, 1, 1, 1, 1], [1, 1, 1, 1, 1], [1, 1, 1, 1, 1], [0, 0, 0, 0, 0], [0, 2, 2, 2, 0]]


def main():
    if len(sys.argv) >= 2:
        rows = [[int(x) for x in part.split()] for part in sys.argv[1].split("/")]
        side = 0 if len(sys.argv) < 3 or sys.argv[2].lower().startswith("w") else 1
    else:
        rows = INITIAL
        side = 0
    v = lookup(rows, side)
    print("局面值：", NAMES.get(v, "越界/未收录"))
    m = best_move(rows, side)
    if m:
        print("一条保持该值的最优走法：(%d,%d) -> (%d,%d)%s" % (m[0], m[1], m[2], m[3], " 吃子" if m[4] else ""))


if __name__ == "__main__":
    main()

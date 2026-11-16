"""Pack and read the exact WDL table files without loading whole layers.

WDLZ stores independent zlib-compressed 64 KiB pages for random access.
ORIDX stores the DTM rank prefix as u32 checkpoints and 9-bit group counts.

Commands:
  python table_storage.py pack --threads 16
  python table_storage.py verify
  python table_storage.py unpack wdl_s15.wdlz wdl_s15.bin
"""

from __future__ import annotations

import argparse
import concurrent.futures
import math
import os
import struct
import sys
import zlib
from collections import OrderedDict


HERE = os.path.dirname(os.path.abspath(__file__))
WDL_MAGIC = b"WDLZ001\0"
OR_MAGIC = b"ORIDX001"
WDL_HEADER = struct.Struct("<8sQQIIII")
ENTRY = struct.Struct("<QII")
OR_HEADER = struct.Struct("<8sQIIQII")
DTM_MAGIC = b"DTMZ002\0"
DTM_HEADER = struct.Struct("<8sQQB3xIII")
PAGE_BYTES = 1 << 16
GROUP_STATES = 256
CHECKPOINT_STRIDE = 256
OR_DELTA_BITS = 9


def expected_states(sheep: int) -> int:
    return 4600 * math.comb(22, sheep)


def _compress_page(item: tuple[int, bytes]) -> tuple[int, bytes, int]:
    number, raw = item
    return number, zlib.compress(raw, 9), len(raw)


def pack_wdl(sheep: int, workers: int) -> None:
    source = os.path.join(HERE, f"wdl_s{sheep}.bin")
    target = os.path.join(HERE, f"wdl_s{sheep}.wdlz")
    states = expected_states(sheep)
    raw_bytes = states // 4
    if states % 4 or os.path.getsize(source) != raw_bytes:
        raise RuntimeError(f"s={sheep}: raw WDL size does not match {states} states")
    page_count = (raw_bytes + PAGE_BYTES - 1) // PAGE_BYTES
    tmp = target + ".tmp"
    entries: list[tuple[int, int, int]] = []
    directory_end = WDL_HEADER.size + page_count * ENTRY.size
    max_pending = max(2, workers * 2)
    raw_done = compressed_done = 0
    try:
        with open(source, "rb", buffering=1 << 20) as src, open(tmp, "w+b", buffering=1 << 20) as dst:
            dst.write(b"\0" * directory_end)
            with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
                pending: dict[int, concurrent.futures.Future] = {}
                next_submit = next_write = 0
                while next_write < page_count:
                    while next_submit < page_count and len(pending) < max_pending:
                        raw = src.read(min(PAGE_BYTES, raw_bytes - next_submit * PAGE_BYTES))
                        if not raw:
                            raise RuntimeError(f"s={sheep}: short raw WDL at page {next_submit}")
                        pending[next_submit] = pool.submit(_compress_page, (next_submit, raw))
                        next_submit += 1
                    future = pending.pop(next_write)
                    number, compressed, uncompressed_length = future.result()
                    if number != next_write:
                        raise RuntimeError("internal WDL page ordering error")
                    offset = dst.tell()
                    dst.write(compressed)
                    entries.append((offset, len(compressed), uncompressed_length))
                    raw_done += uncompressed_length
                    compressed_done += len(compressed)
                    next_write += 1
                    if next_write % 4096 == 0 or next_write == page_count:
                        print(f"s={sheep} pages={next_write}/{page_count} raw={raw_done} compressed={compressed_done}", flush=True)
            if src.read(1):
                raise RuntimeError(f"s={sheep}: unexpected trailing bytes in raw WDL")
            dst.seek(0)
            dst.write(WDL_HEADER.pack(WDL_MAGIC, states, raw_bytes, PAGE_BYTES, page_count, 0, 0))
            for entry in entries:
                dst.write(ENTRY.pack(*entry))
            dst.flush()
            os.fsync(dst.fileno())
        os.replace(tmp, target)
    except BaseException:
        try:
            os.remove(tmp)
        except FileNotFoundError:
            pass
        raise
    print(f"s={sheep} WDLZ {raw_bytes} -> {os.path.getsize(target)} bytes ({os.path.getsize(target) / raw_bytes:.2%})", flush=True)


def _read_wdl_directory(path: str, sheep: int):
    expected = expected_states(sheep)
    with open(path, "rb") as f:
        header = f.read(WDL_HEADER.size)
        if len(header) != WDL_HEADER.size:
            raise RuntimeError(f"{os.path.basename(path)}: short WDLZ header")
        magic, states, raw_bytes, page_bytes, page_count, reserved1, reserved2 = WDL_HEADER.unpack(header)
        if magic != WDL_MAGIC or states != expected or raw_bytes != states // 4 or page_bytes != PAGE_BYTES or reserved1 or reserved2:
            raise RuntimeError(f"{os.path.basename(path)}: invalid WDLZ header")
        wanted_pages = (raw_bytes + page_bytes - 1) // page_bytes
        if page_count != wanted_pages:
            raise RuntimeError(f"{os.path.basename(path)}: invalid WDLZ page count")
        directory_end = WDL_HEADER.size + page_count * ENTRY.size
        entries = []
        previous_end = directory_end
        for page in range(page_count):
            packed = f.read(ENTRY.size)
            if len(packed) != ENTRY.size:
                raise RuntimeError(f"{os.path.basename(path)}: short WDLZ directory")
            offset, compressed_length, raw_length = ENTRY.unpack(packed)
            wanted_raw = min(page_bytes, raw_bytes - page * page_bytes)
            if offset != previous_end or not compressed_length or raw_length != wanted_raw:
                raise RuntimeError(f"{os.path.basename(path)}: invalid WDLZ page {page}")
            entries.append((offset, compressed_length, raw_length))
            previous_end = offset + compressed_length
        if previous_end != os.path.getsize(path):
            raise RuntimeError(f"{os.path.basename(path)}: WDLZ file length mismatch")
        return states, raw_bytes, page_bytes, entries


def verify_wdl(sheep: int) -> None:
    raw_path = os.path.join(HERE, f"wdl_s{sheep}.bin")
    packed_path = os.path.join(HERE, f"wdl_s{sheep}.wdlz")
    states, raw_bytes, _, entries = _read_wdl_directory(packed_path, sheep)
    raw_available = os.path.isfile(raw_path)
    if raw_available and os.path.getsize(raw_path) != raw_bytes:
        raise RuntimeError(f"s={sheep}: raw WDL size mismatch")
    original = open(raw_path, "rb", buffering=1 << 20) if raw_available else None
    total = 0
    try:
        with open(packed_path, "rb", buffering=1 << 20) as packed:
            for page, (offset, compressed_length, raw_length) in enumerate(entries):
                packed.seek(offset)
                raw = zlib.decompress(packed.read(compressed_length))
                if len(raw) != raw_length:
                    raise RuntimeError(f"s={sheep}: invalid WDL page length at page {page}")
                if original is not None and raw != original.read(raw_length):
                    raise RuntimeError(f"s={sheep}: byte mismatch at WDL page {page}")
                total += raw_length
                if (page + 1) % 8192 == 0 or page + 1 == len(entries):
                    print(f"s={sheep} verified WDL pages={page + 1}/{len(entries)} bytes={total}", flush=True)
        if original is not None and original.read(1):
            raise RuntimeError(f"s={sheep}: trailing raw WDL bytes")
    finally:
        if original is not None:
            original.close()
    if total != states // 4:
        raise RuntimeError(f"s={sheep}: verified WDL byte count mismatch")
    mode = "byte-exact" if raw_available else "decompress-only"
    print(f"s={sheep} WDL verification mode={mode}", flush=True)


def unpack_wdl(source: str, target: str) -> None:
    source = os.path.abspath(source)
    target = os.path.abspath(target)
    name = os.path.basename(source)
    try:
        sheep = int(name.split("_s", 1)[1].split(".", 1)[0])
    except (IndexError, ValueError):
        raise RuntimeError(f"cannot infer sheep layer from {name}")
    states, raw_bytes, _, entries = _read_wdl_directory(source, sheep)
    tmp = target + ".tmp"
    try:
        with open(source, "rb", buffering=1 << 20) as packed, open(tmp, "wb", buffering=1 << 20) as raw:
            for page, (offset, compressed_length, raw_length) in enumerate(entries):
                packed.seek(offset)
                data = zlib.decompress(packed.read(compressed_length))
                if len(data) != raw_length:
                    raise RuntimeError(f"WDL page {page} has an invalid decompressed length")
                raw.write(data)
            raw.flush()
            os.fsync(raw.fileno())
        if os.path.getsize(tmp) != raw_bytes or states // 4 != raw_bytes:
            raise RuntimeError("unpacked WDL size mismatch")
        os.replace(tmp, target)
    except BaseException:
        try:
            os.remove(tmp)
        except FileNotFoundError:
            pass
        raise


def _pack_9bit(values, output) -> None:
    accumulator = 0
    fill = 0
    buffer = bytearray()
    for value in values:
        if value > 256:
            raise RuntimeError(f"OR group count {value} does not fit 9 bits")
        accumulator |= value << fill
        fill += OR_DELTA_BITS
        while fill >= 8:
            buffer.append(accumulator & 0xFF)
            accumulator >>= 8
            fill -= 8
        if len(buffer) >= 1 << 20:
            output.write(buffer)
            buffer.clear()
    if fill:
        buffer.append(accumulator & 0xFF)
    if buffer:
        output.write(buffer)


def pack_oridx(sheep: int) -> None:
    prefix_path = os.path.join(HERE, f"dtm_exact_s{sheep}.orpre")
    target = os.path.join(HERE, f"dtm_exact_s{sheep}.oridx")
    states = expected_states(sheep)
    groups = (states + GROUP_STATES - 1) // GROUP_STATES
    expected_prefix_size = (groups + 1) * 4
    if os.path.getsize(prefix_path) != expected_prefix_size:
        raise RuntimeError(f"s={sheep}: OR prefix length mismatch")
    with open(prefix_path, "rb") as f:
        prefix_bytes = f.read()
    prefix = struct.unpack(f"<{groups + 1}I", prefix_bytes)
    if prefix[0] != 0 or any(a > b or b - a > GROUP_STATES for a, b in zip(prefix, prefix[1:])):
        raise RuntimeError(f"s={sheep}: invalid OR prefix counts")
    or_count = prefix[-1]
    checkpoint_groups = list(range(0, groups + 1, CHECKPOINT_STRIDE))
    if checkpoint_groups[-1] != groups:
        checkpoint_groups.append(groups)
    checkpoints = [prefix[group] for group in checkpoint_groups]
    delta_bytes = (groups * OR_DELTA_BITS + 7) // 8
    tmp = target + ".tmp"
    header = OR_HEADER.pack(OR_MAGIC, states, GROUP_STATES, CHECKPOINT_STRIDE, or_count, groups, len(checkpoints))
    try:
        with open(tmp, "w+b", buffering=1 << 20) as out:
            out.write(b"\0" * (OR_HEADER.size + len(checkpoints) * 4))
            _pack_9bit((prefix[g + 1] - prefix[g] for g in range(groups)), out)
            if out.tell() != OR_HEADER.size + len(checkpoints) * 4 + delta_bytes:
                raise RuntimeError(f"s={sheep}: ORIDX packed size mismatch")
            out.seek(0)
            out.write(header)
            for count in checkpoints:
                out.write(struct.pack("<I", count))
            out.flush()
            os.fsync(out.fileno())
        os.replace(tmp, target)
    except BaseException:
        try:
            os.remove(tmp)
        except FileNotFoundError:
            pass
        raise
    print(f"s={sheep} ORIDX groups={groups} OR={or_count} bytes={os.path.getsize(target)}", flush=True)


def verify_oridx(sheep: int) -> None:
    prefix_path = os.path.join(HERE, f"dtm_exact_s{sheep}.orpre")
    path = os.path.join(HERE, f"dtm_exact_s{sheep}.oridx")
    states = expected_states(sheep)
    groups = (states + GROUP_STATES - 1) // GROUP_STATES
    expected_checkpoints = math.ceil(groups / CHECKPOINT_STRIDE) + 1
    source = open(prefix_path, "rb") if os.path.isfile(prefix_path) else None
    try:
        with open(path, "rb") as f:
            header = f.read(OR_HEADER.size)
            if len(header) != OR_HEADER.size:
                raise RuntimeError(f"s={sheep}: short ORIDX header")
            magic, stored_states, group_size, stride, or_count, stored_groups, checkpoint_count = OR_HEADER.unpack(header)
            if (magic != OR_MAGIC or stored_states != states or group_size != GROUP_STATES or stride != CHECKPOINT_STRIDE
                    or stored_groups != groups or checkpoint_count != expected_checkpoints):
                raise RuntimeError(f"s={sheep}: invalid ORIDX header")
            cp_bytes = f.read(checkpoint_count * 4)
            checkpoints = struct.unpack(f"<{checkpoint_count}I", cp_bytes)
            delta_offset = OR_HEADER.size + checkpoint_count * 4
            delta_length = (groups * OR_DELTA_BITS + 7) // 8
            if len(cp_bytes) != checkpoint_count * 4 or os.path.getsize(path) != delta_offset + delta_length:
                raise RuntimeError(f"s={sheep}: ORIDX length mismatch")
            if checkpoints[0] != 0:
                raise RuntimeError(f"s={sheep}: ORIDX initial checkpoint is not zero")
            f.seek(delta_offset)
            if source is not None:
                expected_prefix = source.read(4)
                if len(expected_prefix) != 4 or struct.unpack("<I", expected_prefix)[0] != 0:
                    raise RuntimeError(f"s={sheep}: invalid source OR prefix start")
            accumulator = 0
            available = 0
            rank = 0
            for group in range(groups):
                while available < OR_DELTA_BITS:
                    byte = f.read(1)
                    if len(byte) != 1:
                        raise RuntimeError(f"s={sheep}: short ORIDX delta stream")
                    accumulator |= byte[0] << available
                    available += 8
                delta = accumulator & 0x1FF
                accumulator >>= OR_DELTA_BITS
                available -= OR_DELTA_BITS
                rank += delta
                if source is not None:
                    expected_prefix = source.read(4)
                    if len(expected_prefix) != 4 or rank != struct.unpack("<I", expected_prefix)[0]:
                        raise RuntimeError(f"s={sheep}: ORIDX prefix mismatch at group {group + 1}")
                boundary = group + 1
                if boundary % CHECKPOINT_STRIDE == 0 or boundary == groups:
                    cp_index = boundary // CHECKPOINT_STRIDE
                    if boundary == groups and boundary % CHECKPOINT_STRIDE:
                        cp_index += 1
                    if checkpoints[cp_index] != rank:
                        raise RuntimeError(f"s={sheep}: ORIDX checkpoint mismatch at group {boundary}")
                if (boundary % 2_000_000 == 0) or boundary == groups:
                    print(f"s={sheep} verified ORIDX groups={boundary}/{groups}", flush=True)
            if rank != or_count or (source is not None and source.read(1)):
                raise RuntimeError(f"s={sheep}: ORIDX final count mismatch")
    finally:
        if source is not None:
            source.close()


def _read_dtmz(path: str, sheep: int):
    states_expected = expected_states(sheep)
    with open(path, "rb") as f:
        header = f.read(DTM_HEADER.size)
        if len(header) != DTM_HEADER.size:
            raise RuntimeError(f"{os.path.basename(path)}: short DTMZ header")
        magic, states, or_count, bits, group_states, block_raw_bytes, block_count = DTM_HEADER.unpack(header)
        if (magic != DTM_MAGIC or states != states_expected or or_count > states or not 1 <= bits <= 11
                or group_states != GROUP_STATES or block_raw_bytes < 1 or (block_raw_bytes * 8) % bits):
            raise RuntimeError(f"{os.path.basename(path)}: invalid DTMZ header")
        block_values = block_raw_bytes * 8 // bits
        expected_blocks = (or_count + block_values - 1) // block_values
        if block_count != expected_blocks:
            raise RuntimeError(f"{os.path.basename(path)}: invalid DTMZ block count")
        directory_end = DTM_HEADER.size + block_count * ENTRY.size
        entries = []
        previous_end = directory_end
        for block in range(block_count):
            raw_entry = f.read(ENTRY.size)
            if len(raw_entry) != ENTRY.size:
                raise RuntimeError(f"{os.path.basename(path)}: short DTMZ directory")
            offset, compressed_length, raw_length = ENTRY.unpack(raw_entry)
            count = min(block_values, or_count - block * block_values)
            wanted_raw = (count * bits + 7) // 8
            if offset != previous_end or not compressed_length or raw_length != wanted_raw:
                raise RuntimeError(f"{os.path.basename(path)}: invalid DTMZ block {block}")
            entries.append((offset, compressed_length, raw_length))
            previous_end = offset + compressed_length
        if previous_end != os.path.getsize(path):
            raise RuntimeError(f"{os.path.basename(path)}: DTMZ file length mismatch")
        return states, or_count, bits, group_states, block_raw_bytes, block_values, entries


def verify_dtmz(sheep: int) -> None:
    path = os.path.join(HERE, f"dtm_exact_s{sheep}.dtmz")
    _, _, _, _, _, _, entries = _read_dtmz(path, sheep)
    total_raw = 0
    with open(path, "rb", buffering=1 << 20) as f:
        for block, (offset, compressed_length, raw_length) in enumerate(entries):
            f.seek(offset)
            raw = zlib.decompress(f.read(compressed_length))
            if len(raw) != raw_length:
                raise RuntimeError(f"s={sheep}: invalid DTMZ block {block} decompressed length")
            total_raw += raw_length
    print(f"s={sheep} verified DTMZ blocks={len(entries)} raw={total_raw}", flush=True)


def recompress_dtmz(sheep: int, workers: int) -> None:
    source = os.path.join(HERE, f"dtm_exact_s{sheep}.dtmz")
    tmp = source + ".z9.tmp"
    states, or_count, bits, group_states, block_raw_bytes, block_values, entries = _read_dtmz(source, sheep)
    directory_end = DTM_HEADER.size + len(entries) * ENTRY.size
    compressed_total = raw_total = upgraded_blocks = 0
    new_entries: list[tuple[int, int, int]] = []
    max_pending = max(2, workers * 2)
    try:
        with open(source, "rb", buffering=1 << 20) as original, open(tmp, "w+b", buffering=1 << 20) as output:
            output.write(b"\0" * directory_end)
            with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
                pending: dict[int, concurrent.futures.Future] = {}
                next_submit = next_write = 0
                while next_write < len(entries):
                    while next_submit < len(entries) and len(pending) < max_pending:
                        offset, compressed_length, raw_length = entries[next_submit]
                        original.seek(offset)
                        old_compressed = original.read(compressed_length)
                        raw = zlib.decompress(old_compressed)
                        if len(raw) != raw_length:
                            raise RuntimeError(f"s={sheep}: invalid source DTMZ block {next_submit}")
                        pending[next_submit] = pool.submit(lambda pair: (pair[0], pair[2], zlib.compress(pair[1], 9), pair[1]), (next_submit, raw, old_compressed))
                        next_submit += 1
                    block, old_compressed, compressed9, raw = pending.pop(next_write).result()
                    compressed = compressed9 if len(compressed9) < len(old_compressed) else old_compressed
                    upgraded_blocks += compressed is compressed9
                    if block != next_write or zlib.decompress(compressed) != raw:
                        raise RuntimeError(f"s={sheep}: zlib-9 round-trip mismatch at DTMZ block {block}")
                    offset = output.tell()
                    output.write(compressed)
                    new_entries.append((offset, len(compressed), len(raw)))
                    raw_total += len(raw)
                    compressed_total += len(compressed)
                    next_write += 1
                    if next_write % 1024 == 0 or next_write == len(entries):
                        print(f"s={sheep} zlib-9 DTMZ blocks={next_write}/{len(entries)} raw={raw_total} compressed={compressed_total}", flush=True)
            output.seek(0)
            output.write(DTM_HEADER.pack(DTM_MAGIC, states, or_count, bits, group_states, block_raw_bytes, len(new_entries)))
            for entry in new_entries:
                output.write(ENTRY.pack(*entry))
            output.flush()
            os.fsync(output.fileno())
        if os.path.getsize(tmp) >= os.path.getsize(source):
            os.remove(tmp)
            print(f"s={sheep} zlib-9 would not reduce DTMZ; kept original", flush=True)
            return
        _read_dtmz(tmp, sheep)
        old_size = os.path.getsize(source)
        new_size = os.path.getsize(tmp)
        os.replace(tmp, source)
        print(f"s={sheep} DTMZ zlib-9 best-of-old {old_size} -> {new_size} bytes; improved blocks={upgraded_blocks}/{len(entries)}; every page round-trip verified", flush=True)
    except BaseException:
        try:
            os.remove(tmp)
        except FileNotFoundError:
            pass
        raise


def pack_index_and_remove_raw(sheep: int) -> None:
    """Create and fully verify the compact index before removing its raw source."""
    pack_oridx(sheep)
    verify_oridx(sheep)
    os.remove(os.path.join(HERE, f"dtm_exact_s{sheep}.orpre"))
    print(f"s={sheep} verified ORIDX; removed raw OR prefix", flush=True)


class WdlzReader:
    """Byte-indexable, bounded-cache reader for the compressed WDL pages."""

    def __init__(self, path: str, cache_pages: int = 8):
        self.path = os.path.abspath(path)
        name = os.path.basename(path)
        try:
            sheep = int(name.split("_s", 1)[1].split(".", 1)[0])
        except (IndexError, ValueError):
            raise ValueError(f"cannot infer sheep layer from {name}")
        self.states, self.raw_bytes, self.page_bytes, self.entries = _read_wdl_directory(self.path, sheep)
        self.file = open(self.path, "rb")
        self.cache_pages = cache_pages
        self.cache: OrderedDict[int, bytes] = OrderedDict()

    def __len__(self):
        return self.raw_bytes

    def __getitem__(self, offset: int) -> int:
        if offset < 0:
            offset += self.raw_bytes
        if offset < 0 or offset >= self.raw_bytes:
            raise IndexError(offset)
        page = offset // self.page_bytes
        if page not in self.cache:
            position, compressed_length, raw_length = self.entries[page]
            self.file.seek(position)
            raw = zlib.decompress(self.file.read(compressed_length))
            if len(raw) != raw_length:
                raise RuntimeError(f"WDLZ page {page} decompressed to an invalid length")
            self.cache[page] = raw
            while len(self.cache) > self.cache_pages:
                self.cache.popitem(last=False)
        else:
            self.cache.move_to_end(page)
        return self.cache[page][offset % self.page_bytes]

    def close(self):
        self.file.close()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    pack = sub.add_parser("pack")
    pack.add_argument("--threads", type=int, default=16)
    pack_wdl_only = sub.add_parser("pack-wdl")
    pack_wdl_only.add_argument("--threads", type=int, default=16)
    sub.add_parser("verify")
    recompress = sub.add_parser("recompress-dtm")
    recompress.add_argument("--threads", type=int, default=16)
    recompress.add_argument("--sheep", type=int, choices=range(4, 16))
    pack_index = sub.add_parser("pack-index")
    pack_index.add_argument("sheep", type=int, choices=range(4, 16))
    unpack = sub.add_parser("unpack")
    unpack.add_argument("source")
    unpack.add_argument("target")
    args = parser.parse_args()
    if args.command == "pack":
        if not 1 <= args.threads <= 32:
            raise SystemExit("--threads must be between 1 and 32")
        for sheep in range(4, 16):
            pack_wdl(sheep, args.threads)
            pack_oridx(sheep)
    elif args.command == "verify":
        for sheep in range(4, 16):
            verify_wdl(sheep)
            verify_oridx(sheep)
            verify_dtmz(sheep)
    elif args.command == "unpack":
        unpack_wdl(args.source, args.target)
    elif args.command == "pack-index":
        pack_index_and_remove_raw(args.sheep)
    elif args.command == "recompress-dtm":
        if not 1 <= args.threads <= 32:
            raise SystemExit("--threads must be between 1 and 32")
        layers = [args.sheep] if args.sheep is not None else range(4, 16)
        for sheep in layers:
            recompress_dtmz(sheep, args.threads)


if __name__ == "__main__":
    main()

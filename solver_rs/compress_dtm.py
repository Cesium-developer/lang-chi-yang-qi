import os
import math
import struct
import sys
import zlib

MAGIC = b"DTMZ002\0"
TARGET_RAW_BYTES = 1 << 18
GROUP_STATES = 256
HEADER = struct.Struct("<8sQQB3xIII")
ENTRY = struct.Struct("<QII")


def main():
    if len(sys.argv) != 6:
        raise SystemExit("usage: compress_dtm.py INPUT OUTPUT STATE_COUNT OR_COUNT BITS")
    source, target = sys.argv[1], sys.argv[2]
    states, or_count, bits = int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
    if states <= 0 or not 0 <= or_count <= states or not 1 <= bits <= 11:
        raise SystemExit("invalid state count or bit width")
    unit = bits // math.gcd(bits, 8)
    block_raw_bytes = (TARGET_RAW_BYTES // unit) * unit
    block_values = block_raw_bytes * 8 // bits
    nblocks = (or_count + block_values - 1) // block_values
    expected = (or_count * bits + 7) // 8
    if os.path.getsize(source) != expected:
        raise SystemExit(f"input size mismatch: got {os.path.getsize(source)}, expected {expected}")

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    used = sum(os.path.getsize(os.path.join(base, name)) for base, _, files in os.walk(root) for name in files)
    temp_upper_bound = expected + max(1024, expected // 500) + nblocks * 32 + HEADER.size + nblocks * ENTRY.size
    if used + temp_upper_bound > 15_000_000_000:
        raise SystemExit(f"15 GB workspace cap: compression temp would reach {used + temp_upper_bound} bytes")

    tmp = target + ".tmp"
    entries = []
    block_max = 0
    raw_total = 0
    comp_total = 0
    try:
        with open(source, "rb") as src, open(tmp, "w+b") as dst:
            dst.write(b"\0" * (HEADER.size + nblocks * ENTRY.size))
            for b in range(nblocks):
                start = b * block_values
                count = min(block_values, or_count - start)
                raw_len = (count * bits + 7) // 8
                raw = src.read(raw_len)
                if len(raw) != raw_len:
                    raise RuntimeError(f"short input at block {b}")
                fast = zlib.compress(raw, 6)
                strong = zlib.compress(raw, 9)
                comp = strong if len(strong) < len(fast) else fast
                if zlib.decompress(comp) != raw:
                    raise RuntimeError(f"round-trip mismatch at block {b}")
                offset = dst.tell()
                dst.write(comp)
                entries.append((offset, len(comp), raw_len))
                raw_total += raw_len
                comp_total += len(comp)
                block_max = max(block_max, len(comp))
                if (b + 1) % 1024 == 0 or b + 1 == nblocks:
                    print(f"blocks={b + 1}/{nblocks} raw={raw_total} compressed={comp_total}", flush=True)
            if src.read(1):
                raise RuntimeError("trailing input bytes")
            dst.seek(0)
            dst.write(HEADER.pack(MAGIC, states, or_count, bits, GROUP_STATES, block_raw_bytes, nblocks))
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
    print(f"DONE raw={raw_total} compressed={comp_total} ratio={comp_total / raw_total:.4%} max_chunk={block_max}")


if __name__ == "__main__":
    main()

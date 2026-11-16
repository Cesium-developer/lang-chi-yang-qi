from __future__ import annotations

import argparse
import hashlib
import os
import zipfile
from pathlib import Path

LAYERS = range(4, 16)
MAX_ASSET_BYTES = 2 * 1024**3


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def package_layer(data_dir: Path, output_dir: Path, sheep: int) -> Path:
    members = [
        data_dir / f"wdl_s{sheep}.wdlz",
        data_dir / f"dtm_exact_s{sheep}.dtmz",
        data_dir / f"dtm_exact_s{sheep}.oridx",
    ]
    missing = [path.name for path in members if not path.is_file()]
    if missing:
        raise FileNotFoundError(f"s={sheep}: missing required table files: {', '.join(missing)}")

    target = output_dir / f"lang-chi-yang-qi-tablebase-s{sheep:02}.zip"
    temporary = target.with_suffix(".zip.tmp")
    if target.exists() or temporary.exists():
        raise FileExistsError(f"Refusing to overwrite existing release asset: {target}")
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_STORED, allowZip64=True) as archive:
            for member in members:
                archive.write(member, arcname=member.name)
        if temporary.stat().st_size >= MAX_ASSET_BYTES:
            raise ValueError(f"{temporary.name} exceeds GitHub's per-release-asset limit")
        with zipfile.ZipFile(temporary, "r") as archive:
            bad_member = archive.testzip()
            if bad_member is not None:
                raise RuntimeError(f"{temporary.name}: CRC verification failed for {bad_member}")
            if set(archive.namelist()) != {member.name for member in members}:
                raise RuntimeError(f"{temporary.name}: unexpected archive contents")
        os.replace(temporary, target)
        return target
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def main() -> None:
    parser = argparse.ArgumentParser(description="Package the exact tablebase into GitHub Release-sized archives.")
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    data_dir = args.data_dir.resolve()
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    if any(output_dir.iterdir()):
        raise FileExistsError(f"Refusing to write into a non-empty release folder: {output_dir}")

    checksums = []
    total_bytes = 0
    for sheep in LAYERS:
        archive = package_layer(data_dir, output_dir, sheep)
        size = archive.stat().st_size
        total_bytes += size
        checksums.append(f"{sha256(archive)}  {archive.name}")
        print(f"{archive.name}: {size:,} bytes; SHA256 verified", flush=True)

    manifest = output_dir / "SHA256SUMS.txt"
    manifest.write_text("\n".join(checksums) + "\n", encoding="utf-8")
    print(f"Packaged {len(checksums)} layers; {total_bytes:,} bytes total.")
    print(f"Checksum manifest: {manifest}")


if __name__ == "__main__":
    main()

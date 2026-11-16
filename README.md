# Wolf Eats Sheep — Complete Tablebase

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](Cargo.toml)
[![Python](https://img.shields.io/badge/Python-3.10%2B-blue.svg)](scripts/recompute.ps1)
[![Release](https://img.shields.io/github/v/release/Cesium-developer/lang-chi-yang-qi)](https://github.com/Cesium-developer/lang-chi-yang-qi/releases/latest)

A complete, independently verified WDL and exact-DTM tablebase for the 5×5 board game Wolf Eats Sheep (狼吃羊棋), with an offline GUI, solver sources, and CI builds.

[中文文档](README.zh-CN.md)

## Table of Contents

- [Features](#features)
- [Solved Result](#solved-result)
- [Rules](#rules)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Usage](#usage)
- [Repository Layout](#repository-layout)
- [Testing and Verification](#testing-and-verification)
- [Reproducing the Table](#reproducing-the-table)
- [Citing](#citing)
- [License](#license)

## Features

- Full state-space coverage: 18,779,288,400 legal positions, not just those reachable from the start.
- Exact result (win / draw / loss) for every position.
- Exact DTM (distance to a forced terminal result) for every decisive position.
- WDL verified by a full scan with zero inconsistencies; DTM verified by Bellman checks on all 12 layers.
- Offline GUI with arbitrary-position analysis, all-move listing, adjustable ply limit, undo, and board flip.
- Command-line query tool for single positions.
- Compressed database with random access; archives kept out of Git history.

## Solved Result

| Metric                           | Value                                    |
| -------------------------------- | ---------------------------------------- |
| States solved                    | 18,779,288,400                           |
| Coverage                         | 3 wolves, 4–15 sheep, both sides to move |
| Initial position                | **Draw**                                 |
| WDL verification                 | Zero inconsistencies over all states     |
| DTM verification                 | Bellman check passed on all 12 layers    |
| Maximum DTM                      | 226 plies                                |
| Database size                    | 4,824,928,613 bytes (≈4.49 GiB)          |

Under optimal play the starting position is a draw. This is a strong solution: the result is known for every legal position.

The base table has no ply cap and treats repeated positions as draws. The GUI can apply an optional ply limit on top of the exact DTM values without recomputing anything.

Details: [results](docs/results.md) · [full report](docs/%E5%BC%BA%E8%A7%A3%E7%BB%93%E6%9E%9C.md) · [verification logs](docs/verification/).

## Rules

The board is 5×5. The start has 15 sheep on the top three rows, three wolves in the middle of the bottom row, and seven empty squares.

- Both sides move one step orthogonally into an empty square.
- Sheep cannot capture.
- A wolf captures by jumping over **one empty square** onto a sheep, horizontally or vertically.
- Wolves win when fewer than four sheep remain.
- Sheep win when the wolves have no legal move.

Cycles are draws. The original Chinese rule text is in [`docs/规则.txt`](docs/%E8%A7%84%E5%88%99.txt).

## Prerequisites

- For the GUI: a modern browser and the extracted data files from the release.
- For the CLI query tool: Python 3.10 or later, standard library only.
- For rebuilding: Windows, PowerShell, Python 3.10+, and the compiled tools from GitHub Actions. No local Rust toolchain is required.

## Installation

The repository contains source code only. The tablebase is distributed through GitHub Releases.

1. Download all 12 archives from the [latest release](https://github.com/Cesium-developer/lang-chi-yang-qi/releases/latest). One archive per sheep count, `s=4` through `s=15`.

2. Download `SHA256SUMS.txt` from the same release and verify the archives.

3. Extract all 12 archives. You can put them in one folder such as `D:\lcyq-tablebase`, or let Windows Explorer create one subfolder per archive — the GUI searches the selected folder recursively, so either layout works.

4. The 12 archives together contain 36 data files: for each `N` from 4 to 15, three files.

   | File | Contents |
   |---|---|
   | `wdl_s{N}.wdlz` | Compressed 2-bit WDL layer |
   | `dtm_exact_s{N}.dtmz` | Compressed exact DTM for decisive positions |
   | `dtm_exact_s{N}.oridx` | Compact index for random access |

5. To clone the source: `git clone https://github.com/Cesium-developer/lang-chi-yang-qi.git`

The archives are intentionally excluded from Git history so the repository stays small and quick to clone.

## Usage

### GUI

Open `solver_rs/启动强解GUI.cmd` (or `solver_rs/gui.html` directly) and select the directory with the extracted data files.

The GUI is fully offline. It reads the selected local files on demand, caches at most 128 pages (8 MiB), and never uploads or copies the database. It supports playing against the solved engine, analyzing any position, listing all candidate moves with exact results and distances, an adjustable ply limit, undo, side-to-move switching, and board flip.

### Command line

From the repository root:

```powershell
python solver_rs/wdlq.py
python solver_rs/wdlq.py 'SSSSS/SSSSS/SSSSS/...../.WWW.' wolf
```

Board notation: `S` = sheep, `W` = wolf, `.` = empty; rows separated by `/`. The optional second argument selects the side to move (`wolf` or `sheep`).

### Solver binaries

`Cargo.toml` declares three binaries: `retro5` (WDL solver), `retro_dtm_exact` (exact DTM solver), and `verify` (full WDL verifier). Prebuilt Windows and Linux binaries are attached to each successful CI run as downloadable artifacts.

## Repository Layout

| Path                              | Description                                          |
| --------------------------------- | ---------------------------------------------------- |
| `solver_rs/retro5.rs`             | WDL solver: layered retrograde analysis, 16 threads  |
| `solver_rs/retro_dtm_exact.rs`    | Exact DTM solver with per-layer Bellman verification |
| `solver_rs/verify.rs`             | Independent full-table WDL verifier                  |
| `solver_rs/gui.html`              | Offline interactive GUI                              |
| `solver_rs/wdlq.py`               | Command-line position query                          |
| `solver_rs/table_storage.py`      | Compression, packing, storage verification           |
| `solver_rs/compress_dtm.py`       | DTM compression helper                               |
| `scripts/recompute.ps1`           | Windows end-to-end rebuild                           |
| `scripts/package_release_data.py` | Release archives with SHA-256 manifest               |
| `.github/workflows/build.yml`     | CI build for Windows and Linux                       |

## Testing and Verification

| Check                                              | Result                       |
| -------------------------------------------------- | ---------------------------- |
| Full WDL scan over all states and legal successors | 0 inconsistencies            |
| DTM Bellman check on 12 layers                     | All passed                   |
| Compressed WDL / DTMZ / ORIDX round-trip           | Byte-identical               |
| Release ZIP CRC and SHA-256                        | Verified by packaging script |

Logs: [`verify_all.log`](docs/verification/verify_all.log) and [`retro_dtm_full.log`](docs/verification/retro_dtm_full.log).

## Reproducing the Table

Compiled tools come from CI; no local Rust installation is needed.

1. Clone this repository.
2. Open the **Actions** tab and download the `Windows compiled-tools` artifact from a successful **Build solver** run.
3. Extract it into `build/solver`.
4. Ensure Python 3.10+ is on `PATH`.
5. From the repository root, run:

```powershell
pwsh -File scripts/recompute.ps1 -Threads <thread count>
```

The script regenerates WDL, runs the full verifier, computes DTM with Bellman checks, compresses and validates everything, and writes the 12 archives plus `SHA256SUMS.txt` into the configured release directory. It refuses to overwrite existing table files or archives.

Measured on a Ryzen 7 9800X3D with 16 logical threads and 32 GB RAM: WDL solve 3,347.6 s, WDL verification 1,602.8 s, DTM solve 1,068.4 s. These are observations from the included logs, not guarantees for other hardware.

The `engine/` Python modules implement an older 200-turn rule and are kept for teaching purposes. They are not part of the tablebase.

## License

Released under the [MIT License](LICENSE). AI assistance is disclosed in [AI_USAGE.md](AI_USAGE.md).

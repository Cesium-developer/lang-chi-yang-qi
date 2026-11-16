# 狼吃羊棋 · Lang Chi Yang Qi · Wolf Eats Sheep

A complete, independently verified WDL + exact DTM tablebase for the 5×5, 3-wolf, 15-sheep game, with full state-space coverage, source code, GitHub Actions builds, and downloadable compressed data.

本项目公开狼吃羊棋的完整胜负表、精确距离表、求解与校验源码，以及可交互的离线 GUI。数据库覆盖所有符合棋子数量的局面，不只覆盖从开局可达的局面。

## Solved result

中文说明：[README.zh-CN.md](README.zh-CN.md)。


- Full state space: 18,779,288,400 states, including both sides to move, with 3 wolves and 4–15 sheep.
- Initial position: draw under perfect play.
- WDL means win, draw, or loss. The complete WDL verifier scanned all 18,779,288,400 states and found zero inconsistencies.
- DTM means distance to a forced terminal result. Every decisive state has an exact distance; all 12 sheep-count layers passed the solver's Bellman checks. The largest recorded DTM is 226 plies.
- Downloadable WDL, DTM, and index data together occupy 4,824,928,613 bytes (about 4.49 GiB).
- The base table has no ply cap. Repeated positions are draws. The GUI also supports an optional, adjustable ply-limit interpretation using DTM values.

The authoritative result and validation report is in docs/强解结果.md. Raw verification logs are in docs/verification. The historical research document contains superseded intermediate results and is marked as history.

## Rules covered

The initial position has 15 sheep filling the top three rows, three wolves in the middle of the bottom row, and seven empty squares. Both sides move orthogonally by one square to an empty square. Sheep cannot capture. A wolf may capture a sheep by jumping over one empty square in a straight line. Wolves win when fewer than four sheep remain. Sheep win when the wolves have no legal move.

The table treats cycles as draws and has no move counter. See docs/规则.txt for the original Chinese rule statement and docs/强解结果.md for scope details.

## Download the complete database

The GitHub Release contains 12 archives, one per sheep-count layer from s=4 to s=15. Each archive contains the compressed two-bit WDL layer, compressed exact DTM values for decisive positions, and the compact OR-index needed for random access.

Download all 12 archives for the full tablebase. Verify each archive against SHA256SUMS.txt included with the Release. The archives are intentionally kept out of Git history; the source repository remains small and quick to clone.

## Use the GUI

Open solver_rs/启动强解GUI.cmd, then select the folder containing the extracted WDLZ, DTMZ, and ORIDX files. The GUI is offline and reads the selected local files on demand. It does not upload or copy the database.

## Rebuild and verify

Compilation happens in GitHub Actions. The workflow builds Windows and Linux solver tools and publishes them as downloadable workflow artifacts. It does not compile on the user's computer.

To reproduce the table on Windows:

1. Clone this repository.
2. Open the Actions tab and download the Windows compiled-tools artifact from a successful Build solver workflow run.
3. Extract its contents into build/solver in the repository.
4. Install Python 3.10 or later if it is not already available on PATH.
5. Run from PowerShell:

    pwsh -File scripts/recompute.ps1 -Threads 16

The script creates the raw WDL layers, runs the full WDL verifier, computes exact DTM with per-layer Bellman checks, losslessly packs and validates the data, removes only its own verified raw intermediates, and writes the 12 release archives into release-assets/v1.0.0.

The exact solver sources are in solver_rs. Cargo.toml lists the three final Rust programs. The data helpers use only Python's standard library. The source contains a 15 GB workspace guard. The full solve is CPU- and RAM-intensive; the final report records the measured local run. No GPU backend is required.

## GitHub Actions and repository topics

The Build solver workflow runs on pushes, pull requests, and manual dispatch. It compiles the final solver binaries on both Windows and Linux GitHub-hosted runners and uploads the resulting tools as artifacts.

Suggested GitHub topics:

wolf-eats-sheep, lang-chi-yang-qi, tablebase, retrograde-analysis, game-solving, perfect-information-game, board-game, game-theory, endgame-tablebase, rust.

GitHub topics accept lowercase letters, numbers, and hyphens. The Chinese name 狼吃羊棋 is included in the repository description and this README.

## License and AI disclosure

The source, documentation, and table data are released under the MIT License. See LICENSE.

AI assistance is disclosed in AI_USAGE.md.

Keywords: 狼吃羊棋, Lang Chi Yang Qi, Wolf Eats Sheep, complete game solution, tablebase, retrograde analysis, WDL, DTM.

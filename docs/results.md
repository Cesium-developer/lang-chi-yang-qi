# Final result and validation

## Coverage

The table enumerates every combination with three wolves, four to fifteen sheep, and either side to move. It does not filter by reachability from the initial position.

Total: 18,779,288,400 states.

The terminal rule is fewer than four sheep means a wolf win. A wolf position with no legal move is a sheep win. The base table has no ply cap; unresolved cycles are draws.

## Results

The initial position with the wolves to move is a draw. The same board with the sheep to move is a sheep win.

The WDL table stores win, draw, or loss. The DTM table stores exact distance for every decisive position; draw positions have no forced terminal distance. The maximum recorded DTM is 226 plies.

## Independent checks

- The full WDL verifier scanned every state and legal successor: 18,779,288,400 states, zero inconsistencies.
- The exact DTM solver performed a Bellman check for every one of the 12 layers, s=4 through s=15.
- Storage verification checked the compressed WDL pages, DTMZ blocks, and ORIDX rank structure.
- The release packaging script verifies ZIP CRCs and writes SHA-256 hashes.

See docs/verification/verify_all.log and docs/verification/retro_dtm_full.log for the final run records.

## Data footprint

- Compressed WDL: 781,840,564 bytes.
- Exact DTM: 3,959,415,115 bytes.
- OR index: 83,672,934 bytes.
- Total downloadable database: 4,824,928,613 bytes.
- The 12 release archives are grouped by sheep count.

## Reproduction record

The recorded full WDL solve took 3,347.6 seconds, and its complete WDL verification took 1,602.8 seconds, on a Ryzen 7 9800X3D with 16 logical threads and 32 GB RAM. The exact DTM run completed in 1,068.4 seconds on that machine, with each layer passing Bellman validation. These times are measurements from the included logs, not promises for other hardware.

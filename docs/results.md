# Verification Results (`docs/results.md`)

Every comparison against the reference tools (`OpenROAD` and `OpenSTA` in `ghcr.io/librelane/librelane:3.0.16`) is recorded here, including runs that did **not** yet meet the target thresholds and the root cause identified.

## Summary of Stage Targets

| Stage | Reference Tool | Target ("Done When") | Status |
|---|---|---|---|
| **E0** (Readers) | OpenROAD / OpenSTA | Cell, net, pin, and row counts match exactly on every test design | Pending |
| **E1** (Congestion) | OpenROAD `global_route` | Overflow ranks the test variants in the same order | Pending |
| **E2** (Placement) | OpenROAD `global_placement` | OpenROAD's legalizer accepts the result; HPWL within 5% | Pending |
| **E3** (STA) | OpenSTA (`sta`) | Worst slack and every endpoint's arrival within 1% or 5 ps | Pending |

---

## Verification Log

<!--
Record every benchmark comparison below in chronological order. Do not delete failed/divergent runs; document the root cause and fix.

Template:
### YYYY-MM-DD — <Stage> — `<design>` (<sub-mode or variant>)
- **Reference Command**: `<exact openroad/sta command>`
- **Our Command**: `<exact cargo/cli command>`
- **Our Runtime**: `<ms / s>` (parse: `<ms>`, compute: `<ms>`)

| Metric | Reference (`OpenSTA` / `OpenROAD`) | Ours (`grus-eda`) | Diff (Abs / %) | Pass? |
|---|---|---|---|---|
| ... | ... | ... | ... | ... |

- **Notes / Root Cause**: ...
-->

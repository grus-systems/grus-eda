# tests — verification for the conformance repairs (revision 2)

The revision-2 code surface is a `GRUS_DRT_MEM`-gated stderr
diagnostic plus two metadata/docstring repairs — nothing on the
routing path changes. No new tests are added; the existing pins
prove the change is inert.

## Pins that must stay green

- `test_maze_census` — `GCD_ROUTE_DIGEST = 0xef2b3ff70271a2e9` (the
  census reads sizes; it cannot move the digest).
- `test_mem_census_sanity` — the census floor assertions are
  unaffected (they check `topos_footprint_bytes` /
  `route_graphs_bytes`, not the serial-arm key).
- `test_e2e_ours_def_deterministic_gcd` — byte-equality vs the
  committed `gcd.drt.ours.routed.def` golden.
- `test_net_graph_rebuild_identity`, the three `test_rayon_*` pins,
  `test_maze_determinism` — unchanged code paths.
- Fail-loudly convention: no silent skips (AGENTS §2).

## Functional checks (the actual evidence)

- Fresh `GRUS_DRT_MEM=1` gcd run: `iter=8` (the hard iteration)
  reports a nonzero `attempt_hi`; `outcomes=0` still holds there by
  construction (`docs.md` §6). Emitted DEF sha256 = the golden —
  confirms in one pass that the gated addition cannot touch routing.
- Ungated run emits zero `drt: mem` lines (the env gate is
  unchanged; no re-proof needed — round 1's forced-on/red check
  stands and stays in the log).

## Prove-it-red (AGENTS §7.7)

Not re-run. The round-1 break (`hull_scratch` `out.clear()` deleted →
rebuild-identity + digest red) is already in the log; revision 2 adds
no logic a red-proof could discriminate — the addition is a `max()`
over a capacity walk under the existing `mem_on` gate.

## Checks

`cargo test --workspace --profile verify` and `cargo clippy
--workspace --all-targets -- -D warnings` after each commit — the
`workflow.toml` verify pair.

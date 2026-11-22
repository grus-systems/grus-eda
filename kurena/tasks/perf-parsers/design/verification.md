# perf-parsers — verification

The bar is **identical parse output**: `NetParasitics`, `DesignDb`, and
the VCD/SAIF activity tables must be the same structures on the same
inputs. The cheap strictest proof is the committed test suite — the
strict integration tests already assert downstream results per field —
plus one recorded before/after-binary output comparison and one
recorded intentional break.

## 1. Existing gates (must all stay green — they are the equivalence proof)

- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`.
- SPEF consumers: every `load_spef` integration test in
  `sta/analysis.rs` + `sta/parasitics.rs` (routed + rcx SPEF on
  gcd/picorv32/aes/ibex/jpeg — they compare STA arrivals against the
  committed `*.sta.reference.json`/`dcalc_*.txt` goldens, per pin).
- DEF consumers: `test_{gcd,aes,jpeg,ibex}_placed_def` +
  `*_orientations_match_openroad_pins` (per-pin golden compare) + the
  write/re-parse roundtrips inside them; every E1/E2/E3 test that loads
  a committed DEF (congestion goldens, place+legalize, all `sta`
  designs).
- VCD consumers: `power.rs` `--vcd` tests (per-net rise/fall vs
  `mba.activity.reference.json`, `power_vcd_check.tcl` goldens) and all
  AP1–AP4 activity tests that go through `load_vcd`.
- Reader unit + error-path tests inside the three files (each parser
  doc's error-path checklist): line-numbered rejects, truncation sweeps
  (`spef.rs` 365–374, `def.rs` ~1741), malformed-input accepts.

## 2. Recorded identical-output check (real inputs, not unit scale)

Run the pre-change and post-change release binaries on the largest real
inputs and compare machine outputs:

- `sta … --spef jpeg.routed.spef --json /tmp/{before,after}.json` → `cmp`.
- `sta … --spef picorv32.rcx.spef --json` → `cmp` (covers the
  `*N:suffix` NAME_MAP path).
- `congestion … --def jpeg.placed.def` and `sta … --parasitics
  placement --json` → stdout/`--json` `cmp` (covers DEF parse through
  `cong.load` + `sta.load.def`).
- `power … --vcd gcd.vcd` → stdout `cmp` (power stdout is deterministic;
  covers `signals`/`pin_signals` through `resolve_activity`).

Any diff is a bug — investigate before landing; record "cmp clean" per
command in `docs/results.md`. These double as the spec's "identical-parse
proof" on inputs the unit fixtures cannot cover (e.g. NAME_MAP density,
`+`-attribute groups, real Icarus id reuse).

## 3. Fail-proof mutations (each recorded, each restored)

One per parser, chosen so a *plausible* mechanical slip goes red:

1. **SPEF**: `resolve` drops the `:suffix` append on `*N:s` tokens →
   `parses_the_gcd_routed_rcx_spef` / `converts_units_and_resolves…`
   red → restore. (Guards the S2 rewrite.)
2. **DEF**: `read_conn_group` accepts `( a b c )` (skips the
   three-Word reject) → the malformed-connection test / a
   truncation-adjacent error test red → restore. If no existing test
   covers the 3-word group, add the fixture check first, then mutate.
3. **VCD**: drop the left-pad branch for `bits.len() < nbits` →
   `test_short_vector_writes_left_extend` red → restore. (This exact
   bug happened historically — the comment at 582–587.)

If a mutation lands green, the suite has a hole — add the covering
assertion *first*, then re-run the break (AGENTS §7 rule 7).

## 4. New tests (only where the rewrite needs them)

- `read_conn_group` error mapping (D1): a `( comp pin extra )` input
  must bail with `must be `( component pin )`` at the right line; a
  `( comp ; ` input must bail `unexpected Semi inside `( … )``. If the
  truncation sweep already covers both, skip — check first.
- `split_ascii_whitespace` (S3): no test needed — the acceptance
  question is "same output on committed inputs", proven by §1+§2; the
  non-ASCII note is documented in `docs/results.md`.
- `bit_names` (V3): no new unit test needed — the VCD suite compares
  `signals`/`pin_signals` keys byte-for-byte already; a wrong expansion
  rule goes red immediately.

## 5. Determinism

Serial code throughout — no `rayon` is added. HashMap iteration order
does not feed output anywhere on the touched paths (`ended`/`halves`
are order-by-construction; `signals`/`pin_signals`/`nets` are
`HashMap`s whose iteration order already varies run to run — the
hasher change does not introduce *new* nondeterminism, it changes the
iteration order itself; consumers don't depend on it — verify no test
serializes map order; if one exists it is the signal, not a bug).

## 6. Honesty notes

- `cong.load` improvement is bounded by its LEF share — measure
  `cong.load` before/after, but report DEF's share via `sta.load.def`
  (same `parse_def_bytes`) rather than claiming LEF was touched.
- `cargo test --workspace` does not include the `#[ignore]`d heavy
  tests; the sta-regression verify leg (`cargo test --release -p
  grus-core sta`) covers the strict SPEF/DEF consumers — run it if the
  worktree environment allows and record which legs actually ran.

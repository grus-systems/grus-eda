# Review Fixes: F1, F7, F8 from the 2026-11-11 Project Review

Plan for step 1 of `docs/history/review-2026-11-11.md` Section 5: the three small
findings that make the status readable. This document records what the maintainer measured in the current code, the proposed
fix for each finding, and the questions an external design review must answer before
any implementation ticket is cut. Implementation follows the review; acceptance
review is by the maintainer.

All line numbers below are `dev` at `0fbbc4c`.

## Review resolution (2026-11-13, external reviewer, the design review record (not included))

The external design review answered Q1-Q5 and corrected four measured claims in
this plan; the corrections are applied in the sections below. Resolved:

- Q1: on non-convergence, write the DEF, warn on stdout and stderr, exit 0.
  `iterations` counts gradient steps executed.
- Q2: delete the four dead STA options, make `--tlef --lef --def --sdc`
  required, and update `architecture.md` Section 5 (including the omitted
  `--wire-layer`) in Ticket 03. Do not implement netlist-only STA here.
- Q3: keep `--layer-adjust` and `--gcell-tracks` with the minimal fixes,
  plus validation (`--gcell-tracks >= 1`, which also removes a
  divide-by-zero panic in `congestion/gcell.rs`; `--layer-adjust` in
  `0.0..=1.0`).
- Q4: remove the unused `rayon` and `realfft` dependencies (including
  `Cargo.lock`) in Ticket 04.
- Q5: keep the three-ticket split, run sequentially `02 -> 03 -> 04`.
  `docs/status.md` is the single source for current status, designs
  covered, tolerances actually used and known gaps; done criteria stay in
  `README.md` and the `docs/e*-*.md` Section 1 tables, and the measurement
  log stays in `docs/results.md` (its summary table becomes a link).

## F1. `place` reports convergence when it hit the iteration limit

Measured:

- `crates/grus-core/src/placement/nesterov.rs`: `NesterovResult` (lines
  47-55) has `iterations`, `initial_overflow`, `final_overflow` and no
  `converged` field. The loop is `for k in 0..config.max_iters`
  (`max_iters = 300`, `stop_overflow = 0.10`); it breaks only when
  `tau_k <= stop_overflow`. `final_iters = k` equals the number of
  gradient steps executed when the loop breaks (gcd: 178) and is exact
  (0) when the initial overflow is already below the stop value; it is
  off by one only when `max_iters` is exhausted without breaking (299
  reported for 300 steps). In that exhausted case the loop also exits
  without re-evaluating overflow after the final step, so
  `final_overflow` is stale relative to the returned coordinates
  (corrections from the external design review).
- `crates/grus-eda/src/main.rs` (place handler) prints
  `Global placement: converged in {} iters, ...` unconditionally.
- Review measurement (2026-11-11): on picorv32 the placer printed
  "converged in 299 iters" with `final_overflow = 0.2360`, i.e. it ran out
  of iterations and the legalizer absorbed the rest (max displacement
  21.75 um). Final HPWL 207,079 um vs OpenROAD's 202,261 um, but at a
  different target density (0.60 vs 0.55), so that is not a comparison yet.
- The unit test in `nesterov.rs` also prints "converged" unconditionally,
  but asserts `final_overflow <= 0.10` and `iterations < 300` on gcd.

Proposed fix:

1. Add `pub converged: bool` to `NesterovResult`. Set it only when the
   loop exits because `final_overflow <= stop_overflow`. `iterations`
   counts gradient steps executed (0 for the initial-overflow case,
   `max_iters` when exhausted). When the loop is exhausted,
   re-evaluate overflow on the returned coordinates so
   `final_overflow` (and `converged`) describe the returned placement.
2. CLI: print `converged` only when true; otherwise print
   `did not converge (iteration limit reached)` with the final overflow
   on stdout plus a `warning:` line on stderr, continue to legalization,
   write the DEF, and exit 0 (Q1, resolved).
3. Add regression tests that need no PDK: a synthetic design (following
   the synthetic fixture in `placement/wirelength.rs` tests) forced to
   exhaust a tiny `max_iters` asserts `converged == false` and
   `iterations == max_iters`; a case with `stop_overflow` at or above
   the initial overflow asserts `converged == true` and
   `iterations == 0`. Prove each can fail by reverting the flag.
4. Record in `docs/results.md` that E2 is "PASS on gcd only" until a
   picorv32 E2 test exists. Adding that test (same density as the
   reference, `check_placement`) is a separate, later ticket; it is not
   part of F1.

## F7. The CLI advertises options that do not work

Measured:

- `StaArgs` in `crates/grus-eda/src/main.rs` declares `--json`,
  `--verilog`, `--clock-pin` and `--period-ns`. The STA handler ignores
  all four: it bails unless `--tlef --lef --def --sdc` are all present,
  parses the design only from DEF, and takes the clock only from SDC.
  `docs/architecture.md` Section 5 documents the ignored forms.
- `--parasitics` is `Option<String>`, matched by hand against the
  literals `none` / `placement` / `spef` to produce a `&str` mode; a
  second match then switches on that mode string (correction from the
  external design review).
- `CongestionArgs::gcell_tracks` has `default_value_t = 15`, and the
  handler treats `15` as "not given" (`if args.gcell_tracks != 15`), so
  an explicit `--gcell-tracks 15` is indistinguishable from the default.
- `--layer-adjust` builds `vec![0.0, alpha, alpha, alpha, alpha, alpha]`:
  exactly six layers, with the first (`li1`) fixed at 0, regardless of
  how many routing layers the technology actually has.

Proposed fix:

1. STA: delete `--json`, `--verilog`, `--clock-pin`, `--period-ns`
   (Q2, resolved: delete; do not implement netlist-only STA here), make
   `--tlef --lef --def --sdc` required `PathBuf` fields, and update
   `architecture.md` Section 5 in the same ticket, including the
   `--wire-layer` option it currently omits.
2. `--parasitics`: replace the string with a `clap` `ValueEnum`
   (`None` / `Placement` / `Spef`) and resolve it in a single match.
3. `--gcell-tracks`: change to `Option<u32>` with no default value and
   a parser range of `1..`; `None` means "not given". The `>= 1` bound
   also removes a divide-by-zero panic in `congestion/gcell.rs`.
4. `--layer-adjust`: keep the single uniform factor, validate it is in
   `0.0..=1.0`, and build the vector from the technology's actual
   routing layer count instead of a hard-coded six (Q3, resolved).

## F8. Documents and code have drifted

Measured (each row verified against the tree at `0fbbc4c`):

| Document says | Code has |
|---|---|
| README goal 3: CSR graphs, no allocation per object, timing graph levelized so each level is one parallel pass | STA adjacency is `Vec<Vec<u32>>`, checks in a `BTreeMap`, Kahn order with no levels stored |
| README goals 2 and 5: rayon, NEON SIMD, Metal | `rayon` is declared in the workspace and as an optional dependency of `grus-core` that no feature or code enables; no SIMD; `realfft` is a workspace dependency no crate uses (density has its own DCT over `rustfft`) |
| `architecture.md` 3.1 layout: `sta/graph.rs`, `sta/propagate.rs` | `sta/analysis.rs`, `sta/dmp.rs`, `sta/sdc.rs` differ; `sta/mod.rs` exists and `sta/nldm.rs` / `sta/parasitics.rs` are already listed correctly |
| `architecture.md` 3.2: version `0.1.0`, `realfft` approved | `Cargo.toml` workspace version is `0.1.19` |
| README, `architecture.md`, `docs/e2-placement.md` and the CLI doc comment in `main.rs`: "greedy legalizer" | Abacus plus four local refinement rounds (`placement/legalize.rs`) |
| `AGENTS.md` Section 6 "First Steps (E0)" | E0 to E3 all exist |

Proposed fix:

1. README: split each affected goal into "target" and "today", so the
   goals stay as direction without claiming what does not exist.
2. `architecture.md`: regenerate Section 3.1 from the actual tree and
   update Section 3.2 to the current version and the dependencies
   actually used. Remove `rayon` and `realfft` from `Cargo.toml`,
   `crates/grus-core/Cargo.toml` and `Cargo.lock` (Q4, resolved).
3. Replace "greedy legalizer" wording with "Abacus legalizer with local
   refinement" in README, `architecture.md`, `docs/e2-placement.md`
   and the CLI doc comment in `main.rs`.
4. Drop `AGENTS.md` Section 6 (no incoming references; keep Section 7's
   number stable). If `docs/e3-sta.md` Section 4 is thinned, update the
   `AGENTS.md` Section 3b reference in the same ticket.
5. Add `docs/status.md` as the single source for current status,
   designs covered, tolerance actually used, and known gaps. Done
   criteria stay in `README.md` and `docs/e*-*.md` Section 1; the
   chronological log stays in `docs/results.md`, whose summary table
   is replaced by a link to `docs/status.md` (Q5, resolved).

## Questions for the external design review

- Q1 (F1): When placement does not converge, should `grus-eda place`
  exit non-zero after writing the DEF, warn only and exit zero, or not
  write the DEF at all? Consider that this is a preflight estimation
  tool, that the legalizer output is still usable, and that scripts may
  already rely on exit zero. Also: is `iterations` as a count (not an
  index) the right semantics, including the initial-overflow case?
- Q2 (F7): Delete the four dead STA options, or implement zero-wire STA
  from a netlist (`--json` / `--verilog` without DEF) in this ticket?
  If delete: is there any documented workflow in `docs/` that would
  silently break?
- Q3 (F7): For `--layer-adjust` and `--gcell-tracks`, is the proposed
  minimal fix right, or is either option better deleted? Check every
  caller in `docs/`, `tests/` and `generate_ref.sh` scripts.
- Q4 (F8): Remove the unused `rayon` / `realfft` dependencies, or keep
  them declared with a "not yet used" note? Which better matches the
  repository rule "agreement before speed"?
- Q5 (F1/F7/F8 scope): Is this bundle the right size for three
  implementation tickets (one per finding), or should anything be split
  or merged? Name any place where a proposed fix would change numbers
  in `docs/results.md` or break an existing test's assumptions (for
  example the nesterov test's `iterations < 300` assertion).

## Ticket split after the review

Run sequentially `02 -> 03 -> 04` (02 and 03 both edit `main.rs`).

- Ticket 02: F1 (nesterov flag, final-overflow re-evaluation, CLI
  wording + regression tests + `docs/results.md` E2 status line).
- Ticket 03: F7 (CLI options only; no STA or congestion algorithm
  changes; includes `architecture.md` Section 5).
- Ticket 04: F8 (documentation, `docs/status.md`, the `Cargo.toml` /
  `Cargo.lock` dependency removal, `docs/e2-placement.md` wording,
  and the `AGENTS.md` Section 3b reference if Section 4 of
  `docs/e3-sta.md` is thinned).

## Acceptance for the whole bundle

- `cargo test --release` passes (tests that need the PDK or the
  picorv32 generated inputs fail loudly when those are missing, per
  AGENTS.md Section 2; record which tests were actually run).
- `cargo clippy --release -- -D warnings` passes.
- No golden or reference file changes.
- Every comparison or status change is recorded in `docs/results.md`.

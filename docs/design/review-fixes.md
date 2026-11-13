# Review Fixes: F1, F7, F8 from the 2026-11-11 Project Review

Plan for step 1 of `docs/review-2026-11-11.md` Section 5: the three small
findings that make the status readable. This document records what the maintainer measured in the current code, the proposed
fix for each finding, and the questions an external design review must answer before
any implementation ticket is cut. Implementation follows the review; acceptance
review is by the maintainer.

All line numbers below are `dev` at `0fbbc4c`.

## F1. `place` reports convergence when it hit the iteration limit

Measured:

- `crates/grus-core/src/placement/nesterov.rs`: `NesterovResult` (lines
  47-55) has `iterations`, `initial_overflow`, `final_overflow` and no
  `converged` field. The loop is `for k in 0..config.max_iters`
  (`max_iters = 300`, `stop_overflow = 0.10`); it breaks only when
  `tau_k <= stop_overflow`. `final_iters = k`, so `iterations` is the last
  loop index, not the number of iterations executed (off by one, and 0 when
  the initial overflow is already below the stop value).
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
   loop exits because `final_overflow <= stop_overflow`. Fix `iterations`
   to be the count of iterations executed.
2. CLI: print `converged` only when true; otherwise print
   `did not converge (iteration limit reached)` with the final overflow,
   and continue to legalization (the output is still usable as an
   estimate). Whether the process should also exit non-zero is Q1.
3. Add a regression test that forces non-convergence (for example a tiny
   `max_iters`) and asserts `converged == false` and the iteration count.
   Prove it can fail by reverting the flag.
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
  literals `none` / `placement` / `spef` in two places.
- `CongestionArgs::gcell_tracks` has `default_value_t = 15`, and the
  handler treats `15` as "not given" (`if args.gcell_tracks != 15`), so
  an explicit `--gcell-tracks 15` is indistinguishable from the default.
- `--layer-adjust` builds `vec![0.0, alpha, alpha, alpha, alpha, alpha]`:
  exactly six layers, with the first (`li1`) fixed at 0, regardless of
  how many routing layers the technology actually has.

Proposed fix:

1. STA: delete `--json`, `--verilog`, `--clock-pin`, `--period-ns`
   (Q2 asks whether deleting, rather than implementing zero-wire STA
   from a netlist, is the right call for this ticket).
2. `--parasitics`: replace the string with a `clap` `ValueEnum`
   (`None` / `Placement` / `Spef`) and a single match.
3. `--gcell-tracks`: change to `Option<u32>` with no default value;
   `None` means "not given".
4. `--layer-adjust`: Q3 asks for the least surprising fix that does not
   invent a new interface: keep the single uniform factor, but build the
   vector from the technology's actual layer count instead of a
   hard-coded six, or drop the option until a per-layer form exists.

## F8. Documents and code have drifted

Measured (each row verified against the tree at `0fbbc4c`):

| Document says | Code has |
|---|---|
| README goal 3: CSR graphs, no allocation per object, timing graph levelized so each level is one parallel pass | STA adjacency is `Vec<Vec<u32>>`, checks in a `BTreeMap`, Kahn order with no levels stored |
| README goals 2 and 5: rayon, NEON SIMD, Metal | `rayon` is an optional workspace dependency no code enables; no SIMD; `realfft` is a workspace dependency no crate uses (density has its own DCT) |
| `architecture.md` 3.1 layout: `sta/graph.rs`, `sta/propagate.rs` | `sta/analysis.rs`, `sta/dmp.rs`, `sta/nldm.rs`, `sta/parasitics.rs`, `sta/sdc.rs` |
| `architecture.md` 3.2: version `0.1.0`, `realfft` approved | `Cargo.toml` workspace version is `0.1.19` |
| README and `architecture.md`: "greedy legalizer" | Abacus plus four local refinement rounds (`placement/legalize.rs`) |
| `AGENTS.md` Section 6 "First Steps (E0)" | E0 to E3 all exist |

Proposed fix:

1. README: split each affected goal into "target" and "today", so the
   goals stay as direction without claiming what does not exist.
2. `architecture.md`: regenerate Section 3.1 from the actual tree and
   update Section 3.2 to the current version; note `realfft` and `rayon`
   as declared-but-unused (Q4: remove them from `Cargo.toml`, or keep
   and document?).
3. Replace "greedy legalizer" wording with "Abacus legalizer with local
   refinement" in README and `architecture.md`.
4. Drop `AGENTS.md` Section 6 (keep the numbering of the other sections
   stable if references exist; check for references first).
5. Add `docs/status.md`: one table with stage, designs covered,
   tolerance actually used, and known gaps, sourced from
   `docs/e3-sta.md` Section 4 and `docs/results.md`. Q5 asks whether
   `docs/status.md` should be the single source that other documents
   link to, and what must stay duplicated.

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

- Ticket 02: F1 (nesterov flag + CLI wording + regression test +
  `docs/results.md` E2 status line).
- Ticket 03: F7 (CLI options only; no STA or congestion algorithm
  changes).
- Ticket 04: F8 (documentation only, plus `Cargo.toml` dependency
  changes if Q4 says remove).

## Acceptance for the whole bundle

- `cargo test --release` passes (tests that need the PDK or the
  picorv32 generated inputs fail loudly when those are missing, per
  AGENTS.md Section 2; record which tests were actually run).
- `cargo clippy --release -- -D warnings` passes.
- No golden or reference file changes.
- Every comparison or status change is recorded in `docs/results.md`.

# F8: bring the documents back in line with the code

## Goal
Fix the drift listed in `docs/review-2026-11-11.md` F8: README goals
claim machinery that does not exist, `architecture.md` describes a
different tree and version, the unused `rayon`/`realfft` dependencies
are declared, "greedy legalizer" wording survives in four places, and
`AGENTS.md` still has the E0 first-steps section. Add `docs/status.md`
as the single source for current status. Design:
`docs/design/review-fixes.md` F8 and the Review resolution section;
review: the design review record (not included) Q4 and Q5.
Run this ticket after Tickets 02 and 03.

## Files
- README.md
- AGENTS.md
- Cargo.toml
- Cargo.lock
- crates/grus-core/Cargo.toml
- docs/architecture.md
- docs/e2-placement.md
- docs/e3-sta.md
- docs/results.md
- docs/status.md
- crates/grus-eda/src/main.rs

## Interface
- `docs/status.md` (new): one table — stage (E0-E3), designs covered,
  tolerance actually used, known gaps — sourced from
  `docs/e3-sta.md` Section 4 and `docs/results.md` as they stand after
  Tickets 02 and 03. Link it from `README.md` Documentation.
- `docs/results.md`: replace the "Summary of Stage Targets" status
  table with a short paragraph linking to `docs/status.md`. The
  chronological log below it is untouched.
- `docs/e3-sta.md` Section 4: keep the per-test E3 breakdown, but add a
  line at its top stating `docs/status.md` is the cross-stage status
  source. If you instead move the table, you must also update the
  `AGENTS.md` Section 3b reference in this same ticket.
- `README.md`: in goals 2, 3 and 5, mark each claim that is not true
  today as a target (for example a "Target:" / "Today:" split):
  CSR timing graph, levelized parallel passes, rayon, NEON, Metal,
  `realfft`. Replace "lightweight greedy legalizer" with
  "Abacus legalizer with local refinement".
- `docs/architecture.md`: regenerate Section 3.1 from the actual tree
  (`sta/analysis.rs`, `sta/dmp.rs`, `sta/nldm.rs`, `sta/parasitics.rs`,
  `sta/sdc.rs`); update Section 3.2 to the workspace version current
  after Tickets 02 and 03 and to the dependencies actually used after
  the removal below; replace "greedy legalizer" wording.
- Dependencies: remove `rayon` and `realfft` from the workspace
  `Cargo.toml`, remove the `rayon` optional dependency from
  `crates/grus-core/Cargo.toml`, and refresh `Cargo.lock`. Nothing
  under `crates/` may change.
- "greedy" wording: also fix `docs/e2-placement.md` (title and the
  E2.4 rows) and the `Place` doc comment in
  `crates/grus-eda/src/main.rs` (that one word in that one comment is
  the only `crates/` change in this ticket).
- `AGENTS.md`: delete Section 6 ("First Steps (E0)") entirely, keeping
  the numbers of all other sections unchanged.

## Acceptance
- `cargo build --release` succeeds after the dependency removal
- `grep -rn 'realfft\|rayon' Cargo.toml crates/grus-core/Cargo.toml` prints nothing
- `grep -rn 'greedy' README.md docs/architecture.md docs/e2-placement.md` prints nothing
  (historical entries in `docs/results.md` and `docs/review-2026-11-11.md`
  are records and stay)
- `test -s docs/status.md`
- `cargo clippy --release -- -D warnings` passes

## Notes
- Read first: `AGENTS.md` (note Section 7 rule 8), `docs/design/review-fixes.md`
  (F8 + Review resolution), the design review record (not included) Q4 and Q5.
- Documentation and dependency-declaration changes only: no code under
  `crates/` changes, no numbers in the `docs/results.md` log change, no
  golden or reference file changes.
- `docs/review-2026-11-11.md`, the design review record (not included) and the
  dated log entries in `docs/results.md` are historical records: do not
  rewrite them.
- Do not read anything outside this repository. Do not push.

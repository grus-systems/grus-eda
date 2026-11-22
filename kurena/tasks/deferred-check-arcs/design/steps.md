# deferred-check-arcs — Steps

Ordered. Steps 0–2 produce the measurement; step 3 is the per-type branch;
steps 4–5 land docs + version. Suggested split: (a) fixture + probe
transcripts + `results.md` entry, (b) implementation (if any) + tests +
docs + version.

## Step 0 — Fixture + container run

1. Write `tests/designs/sta_edge/check_arcs/` per `fixture.md`: DEF (14
   cells, 16 nets, 6 ports — fixture.md is authoritative), `ideal.sdc`,
   `propagated.sdc`, `te_slow.sdc`, `generate_ref.sh` (golden JSON +
   `run_probe` per `probe.md`, all three modes).
2. Colima/Docker route per AGENTS.md §2; run `bash
   tests/designs/sta_edge/check_arcs/generate_ref.sh`. Rerun and confirm
   `git status` on the fixture dir is clean (byte-reproducible before
   commit, AGENTS.md §7.1).
3. Read `probe_check_arcs_*.txt`: answer Q1 (`non_seq` — data-check surface
   or ignored), Q2 (`three_state` — propagates which arcs or ignored), Q4
   (`Clear`/`Preset` confirm) from the verbatim output.

## Step 1 — Our-side runs

`grus-eda sta` on all three SDCs per `probe.md` "Our-side runs" — `--json`
captured, one `--strict` invocation. Record the full
`recognized_check_arcs` map verbatim — it becomes the pinned literal in
`test_check_arcs_deferred_counts`.

## Step 2 — `docs/results.md` entry

Write the dated entry per `docs.md` §1: census table, Q0 surface, per-type
verdicts with verbatim lines, our-side counts, the `preserve` census line,
the `Clear`/`Preset` pointer to the R6 entry. This entry exists under every
branch. Commit (a).

## Step 3 — Per-type branch

- **All deferred (types ignored by OpenSTA):** add the §A tests
  (`test_sta_edge_check_arcs_{ideal,propagated,te_slow}` STRICT +
  `test_check_arcs_deferred_counts` with the measured map). Fail-proof:
  drop one type from `is_deferred_check()` → pinned test red → restore.
- **`three_state_*` propagates (B1):** promote the measured type(s) per
  `api.md` §B1 — `is_deferred_check` loses them, the `Combinational` arm
  gains them; update the pinned map; STRICT compares cover `u_cap*/D`
  including the `te_slow` discriminator. Fail-proof: keep one arc
  deferred → `te_slow` red → restore.
- **`non_seq_*` evaluated (B2a/B2b):** implement the measured shape per
  `api.md` §B2 — endpoint rows or a `data_checks` report family + golden
  section + `EndpointKind` variant; update the pinned map; STRICT compare.
  Fail-proof: break the measured anchor or constraint mapping → red →
  restore.
- Mixed outcomes are expected to compose (e.g. three_state propagates,
  non_seq ignored): apply each type's row independently.

## Step 4 — Docs + version

Per `docs.md`: `roadmap.md` N3 tail + R6 residual line, `e3-sta.md` line,
`README.md` deferred list, `tests/designs/README.md` fixture line.
`Cargo.toml` `0.2.10` → `0.2.11` + `cargo check` for `Cargo.lock`, same
commit as the functional content.

## Step 5 — Verify

- `cargo test -p grus-core check_arcs` (new tests), then `cargo test
  --workspace` — green, STRICT never loosened.
- Existing goldens byte-identical: `git diff --exit-code -- tests/designs/`
  shows only the new fixture dir; real designs instantiate none of these
  cells (census), so any moved number elsewhere is a bug.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- Commits: conventional types (`test:`/`feat:`/`docs:`), explicit paths,
  agent trailer; name which parts of `generate_ref.sh` ran and which
  branch each type took.

## Acceptance cross-check

- [ ] Lib census recorded → `overview.md` + `results.md` §(census)
- [ ] Per-type OpenSTA behaviour measured w/ committed artifacts → Steps
  0–2 (`probe_check_arcs_*.txt`, `results.md`)
- [ ] Evaluated → implemented + STRICT golden; ignored → counted +
  explicit record → Step 3 + §A pinned test
- [ ] `cargo test --workspace` + clippy; goldens byte-identical; version;
  docs → Steps 4–5

## Risk checklist

- **Non-dedicated non_seq surface**: if `non_seq` rows only surface via
  `report_check_types` flags that exist but print zero rows, record the
  empty tables verbatim — "evaluated but no rows" vs "no surface" is a
  distinction the record must make honestly.
- **`te_slow` no-diff ambiguity**: identical `ideal`/`te_slow` endpoints
  could mean ignored arcs *or* a `te` path that never wins anyway — the
  Q2 transient `set_disable_timing A->Z` test is the unambiguous check;
  do not conclude "ignored" from the golden diff alone.
- **Data-check rows without PathEnd access**: if the surface is
  `report_check_types` text only, the golden collector regexp-parses it
  inside the same Tcl session (pulse-width precedent) — still
  container-emitted.
- **Single-pin dangling nets rejected**: fixture.md §1 contingency —
  record the resolution in `results.md`.
- **Container/PDK**: Colima running, PDK at `~/.ciel` (`PDK_ROOT`
  override supported).

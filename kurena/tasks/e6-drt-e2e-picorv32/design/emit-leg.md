# e6-drt-e2e-picorv32 — emit + check legs (r1)

Same two legs the gcd gate ran: host-side `generate_drt_ours.sh`
produces the routed DEF + emit census log; container-side
`check_drt_ours.sh` scores it with Magic `drc(full)` and emits the
golden. Nothing is hand-built; the golden's provenance fields
(`generated_by`, `image_id`, `input_def_sha_prefix`,
`input_def_provenance`) are the record.

## 1. Pre-flight checks (all present on the planning host)

- `tests/designs/picorv32/picorv32.placed.def` — gitignored,
  1,738,906 B; seeded into the worktree by `worktree_seed` or
  regenerated via `tests/designs/picorv32/generate_ref.sh`.
- `tests/designs/picorv32/picorv32.guide` — committed, 2,137,576 B
  (the OpenROAD `global_route -guide_file` input the whole E6
  chain is pinned on).
- `tests/designs/picorv32/picorv32.drt.reference.json` —
  committed, 14,098 B; `generate_drt_ours.sh` requires it
  (informational `wirelength_um`/`via.<cut>` deltas vs
  `variants.{delegated,baseline}`).
- `tests/designs/picorv32/picorv32.drc.reference.json` —
  committed, 50,296 B; step E's taxonomy join.
- PDK at `${PDK_ROOT:-~/.ciel}/sky130A` (present:
  `libs.ref`/`libs.tech`).
- Docker via Colima + cached image
  `ghcr.io/librelane/librelane:3.0.16`
  (`image_id sha256:02d50f259d483979e126ef188a50f5b61e1d285b9d538312f280b087439b0302`
  — recorded in the gcd golden; a different digest at pull time is
  recorded-not-gated).
- **Source freeze**: `grus-eda` builds `--release` inside the
  script, so no `src/drt/*`/`main.rs` emit-path edits may land
  between the emit leg and the golden commit — the committed DEF
  must equal a fresh emit on the committed tree. Test, script,
  and doc files are free to move (they don't change emit bytes).

## 2. Emit leg — `generate_drt_ours.sh picorv32 --verify`

```
bash tests/designs/generate_drt_ours.sh picorv32 --verify \
    > tests/designs/picorv32/runs/drt-ours/emit-driver.log 2>&1 &
```

Detached (the route alone measured 5,514.82 s clean on an M4 at
v0.12.6, 30,658 s under ambient load on an M1-Pro; the current
code adds the hull mask + self-spacing closure + `notch_fills`
extraction, so plan for hours, not the old number). The script:

- `cargo build --release -p grus-eda`, then runs
  `grus-eda drt --tlef … --lef … --def picorv32.placed.def
  --guide picorv32.guide --out-def picorv32.drt.ours.routed.def
  --reference picorv32.drt.reference.json --self-drc`,
  teeing the census log to
  `runs/drt-ours/picorv32.drt.ours.log` (the `cli_lines` source
  for step E).
- Prints `OURS-SHA <sha256>` of the emitted DEF.
- `--verify` re-runs the identical invocation into
  `runs/drt-ours/picorv32.drt.ours.rerun.def` and `cmp`s the two:
  `OURS-DETERMINISTIC <sha>` on byte-equality, nonzero exit with
  the first differing offset on nondeterminism — a route/emit
  nondeterminism is a bug, never a committed artifact.

**`--verify` vs wall clock**: the second route roughly doubles the
leg (~3–17 h range on the measured extremes). Default is to run
it — it is the only script-side determinism arm at picorv32
scale, and the ticket's spec asks for the determinism leg "if the
picorv32 emit time permits (record the choice)". If the first
emit's `route_ms` already projects the pair beyond an overnight
window, drop `--verify` and record the choice + the
`test_maze_determinism_picorv32` precedent (two in-process
`route_nets` runs, `#[ignore]`d) in `docs/results.md` — do not
silent-skip.

Capture from the emit log for `docs/results.md`: the `guided=…`
census line (incl. `route_ms`, `hull_masked`, `notch_fills`,
`notch_residuals`, `hole_residuals`), `unrouted_kinds`,
`unrouted_blockers`, `self_drc findings_total=`, and the
`wirelength_um`/`via.<cut>` informational deltas.

## 3. Check leg — `check_drt_ours.sh picorv32`

```
bash tests/designs/check_drt_ours.sh picorv32
```

Re-invokes itself inside the pinned container (repo at `/work`,
PDK at `/pdk`). Steps: A OpenROAD `read_def` + surface probes +
`check_antennas -verbose` on our DEF; B Magic `drc(full)` on our
DEF; C the identical Magic sequence on `picorv32.placed.def` (the
inherited baseline); D the `add_spacing_probe_wire` mutation +
Magic (sanity control); E python assembly →
`tests/designs/picorv32/picorv32.drt.ours.reference.json`.

Runtime: minutes, not hours — picorv32's e7-drc-ref measured
~61 s antenna + ~20 s per Magic pass; step E's DEF re-parse is the
largest piece but well under the emit leg by two orders of
magnitude. Per-phase seconds land in the gitignored
`runs/drt-ours/runtime.json` (transcribe into `docs/results.md`;
the golden carries no wall-clock fields).

The step-D mutation deserves one picorv32-specific note in the
results entry: `add_spacing_probe_wire` picks `net_a` = the first
signal net with an axis-aligned met2→met1→met3 segment and `net_b`
= the next signal net — on a 6,811-net DEF the probe is real but
its *placement* is arbitrary; `sanity_control.new_rules_vs_clean`
is the live-control evidence regardless of which net hosts it.

## 4. Commit decisions (measure, don't presume)

- `picorv32.drt.ours.reference.json` — **commit** (the acceptance
  item; expected well under 1 MB even with a several-hundred-net
  `unrouted`/`stub_outside_cover` census — gcd's is ~50 KB).
- `picorv32.drt.ours.routed.def` — measure first. Expected ~4–6 MB
  (delegated 7.15 MB, baseline 7.02 MB; ours emits less wire plus
  `RECT` fill paint):
  - `wc -c < 1,048,576` → add the gitignore exception
    `!tests/designs/picorv32/picorv32.drt.ours.routed.def`
    (same convention as gcd) and commit — this unlocks the
    optional byte-compare determinism test (`gate.md` §3).
  - `≥ 1 MB` → stays gitignored; `generate_drt_ours.sh picorv32`
    is the committed reproduction path (`AGENTS.md` — golden
    outputs >1 MB commit the script, not the file). Record the
    measured size in `docs/results.md`.
- `runs/drt-ours/` — gitignored (`tests/designs/picorv32/runs` is
  already ignored): emit/mutation/probe artifacts and the
  detached-run driver log live there, never committed; the
  results entry names them for reproducibility.

## 5. Cross-checks before commit

- `shasum -a 256 picorv32.drt.ours.routed.def | cut -c1-16` ==
  golden `input_def_sha_prefix` (the step-E output must score the
  measured file, not a re-emit).
- `git diff tests/designs/picorv32/picorv32.drt.ours.reference.json`
  after a re-run of step E's inputs must be empty except possibly
  `image_id` — a diff is the bug (investigate, don't re-commit;
  never rebaseline).
- The gcd golden is untouched: the script fix
  (`script-fix.md`) is a tightening on a path that never fired on
  gcd's log; optionally rerun `check_drt_ours.sh gcd` as the
  inertness proof — byte-identical JSON expected
  (`image_id` drift aside, which is recorded not committed).

## Files

- `tests/designs/generate_drt_ours.sh`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/picorv32/picorv32.drt.ours.routed.def`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json`
- `tests/designs/picorv32/runs/` (gitignored)
- `.gitignore` (only if the DEF commits)

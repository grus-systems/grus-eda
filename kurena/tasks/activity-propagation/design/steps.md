# activity-propagation — Steps

Ordered. Steps 0–1 are the measurement (container); step 2 records it;
step 3 is the branch; steps 4–5 land docs + version. Commit in logical
units: fixture+script, then probe artifacts+results entry, then docs/branch
(artifacts ride with the entry that explains them).

## Step 0 — Author the fixture (no container needed)

Create `tests/designs/microbench/mba.def`, `mba.sdc`, and the five VCDs
per `fixture.md` (it is authoritative for connectivity, waveforms, and the
`$dumpvars`-initials rule). Sanity-check locally: our own VCD reader parses
each file (`load_vcd`) — a quick unit-level parse is a cheap smoke test, do
not commit a test for it unless it stays.

## Step 1 — Container run

Write `generate_activity_ref.sh` per `fixture.md`/`probe.md`, then run it
end-to-end inside `ghcr.io/librelane/librelane:3.0.16` (Colima/Docker route
per AGENTS.md §2 — start Colima first if needed:
`mise exec docker-cli@latest colima@latest lima@latest -- colima start
--memory 12 --cpu 8`).

- Capture **all** session stdout — P0's `info commands`/`catch` output and
  every probe table feed the results entry; keep the unfiltered streams
  (e.g. under `/tmp` or pasted into the entry) alongside the committed
  filtered `mba.none.openroad.txt`.
- If P0 surfaces a per-pin accessor, use it for the P1–P5 implied rates and
  record that in the entry; if a `set_power_activity` flag spelling differs
  from `probe.md`'s guesses, the script uses what parses and the entry
  records the measured spelling.
- Rerun the script once; `git status` on `tests/designs/microbench/` must be
  clean — committed artifacts must regenerate byte-identically (AGENTS.md
  §7 rule 1). If `mba.activity.reference.json` or the transcript moves,
  investigate and record why before committing — no silent regen.

## Step 2 — `docs/results.md` entry

Write the dated entry per `docs.md` §1 while session output is at hand:
every P-verdict gets its discriminating numbers verbatim, every implied
rate its derivation method. This entry exists under both branches.

## Step 3 — Branch

- **P (plan, expected)**: `docs/roadmap.md` N7 line → the AP1–AP4 pointer
  per `docs.md` §2; no code change. The fixture + JSON + transcript commit
  anyway — they are AP1–AP4's golden.
- **I (implement, only if the measured model is one-commit-small)**: fill
  pass per `api.md` in `power.rs` (+ `main.rs` only if the no-annotation
  mode lands); `test_activity_propagation_mba_matches_opensta` per
  `fixture.md` §Test at the measured STRICT bounds; fail-proof per
  `probe.md` P6 §8 (disable the fill → red; corrupt a golden block → red;
  restore); `docs/roadmap.md` N7 → the DONE line.

## Step 4 — README + housekeeping

`tests/designs/README.md` microbench entry per `docs.md` §3. `README.md`/
`AGENTS.md` untouched (`docs.md` §4 caveat only if Branch I adds a CLI
mode). `kurena/tasks/activity-propagation/design/` docs stay as committed.

## Step 5 — Verify + version + commit

- `cargo test --workspace` — green (new tests only under Branch I).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- Existing goldens byte-identical: this ticket adds files only; `git diff`
  on `mb*`/`mbw*`/`gcd`/all `*.reference.json` must be empty — verify
  explicitly and say so in the entry's checks section.
- Version `0.2.12` → `0.2.13` (`Cargo.toml` + `cargo check` for
  `Cargo.lock`), in the same commit as the functional change (AGENTS.md §5).
- Commit: `test:`/`docs:`/`feat:` conventional type, English, explicit
  paths, agent trailer; the body names which script sections ran, which
  branch the measurement took, and the P1–P5 verdicts in one line each.

## Verification gates

- `generate_activity_ref.sh` idempotent (rerun → clean `git status`).
- `cargo test --workspace` + clippy clean; no `*.reference.json` outside
  `mba.activity.reference.json` touched.
- Branch I only: mba test red-then-green on the disabled-fill break;
  golden-corruption break → red; both restored.
- Every committed number traceable to the committed script's output —
  nothing hand-derived in `mba.activity.reference.json` (the python parse
  step emits it; `git diff` after rerun is the proof).

## Acceptance cross-check

- [ ] un-annotated defaults (input rate, sequential default) measured +
  verbatim → probe.md P1, results.md §3
- [ ] combinational + sequential propagation on discriminating fixtures
  (toggle/counter for divide-by-2) → probe.md P2–P3, fixture.md
- [ ] partial-annotation fallback semantics → probe.md P4, results.md §6
- [ ] implementation with golden coverage, or sequenced plan in docs →
  steps 3 + api.md §Plan
- [ ] `cargo test --workspace` + clippy; goldens byte-identical; version
  bumped → step 5

# e7-drc-ref — Steps

## 1. Write `tests/designs/generate_drc_ref.sh`

Implement `script.md` end to end: host wrapper + guards → step A
antenna → step B DRT report discover/regenerate → step C Magic clean →
step D mutation + Magic broken → step E assembly + taxonomy. Keep the
Tcl line-per-command and `== 0` rules; markers `DRC-`/`DRCREF-`
anchored at line start in all regexes.

## 2. Run gcd

```bash
mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8
bash tests/designs/generate_drc_ref.sh gcd tests/designs/gcd/gcd.routed.def
```

Then fix what the loud failures find, in order of likelihood:

- **`check_antennas` grammar** — pin `-verbose` acceptance and the
  violation-count line from the actual transcript; record the observed
  surface in `docs/results.md`.
- **DRT regeneration** — `gcd.drc.rpt` is absent in seeded worktrees,
  so the regen path fires on gcd: the commands mirror step 5b verbatim
  (**no** layer adjustments — those are step 5c's), and the regen DEF
  sha is compared against the committed `gcd.routed.def`
  (`regenerated_identical` expected — routing determinism evidence —
  but record whichever truth appears).
- **Magic output grammar** — `drc count`/`drc count total`/`drc
  listall why`/`--version` line shapes; parser regexes written against
  `magic_clean.log`, not assumed formats.
- **Tech-file `drc` section grammar** — section delimiters and the
  `why`-string extraction against `sky130A.tech` as mounted.
- **Broken run** — expect `met2 spacing`-family names verbatim; any
  other outcome is recorded data.

## 3. Byte-reproducibility check

Re-run the same command; `git diff` every committed artifact.
`runtime_s` is the named varying field — everything else must
byte-reproduce. Record the rerun count in `docs/results.md` (§7.1:
the committed script, run as written, produces the golden).

## 4. picorv32

`picorv32.routed.def` (gitignored, ~7 MB) exists in seeded worktrees;
`picorv32.drc.rpt` exists (0 lines — the `existing` path, no regen).

```bash
bash tests/designs/generate_drc_ref.sh picorv32 tests/designs/picorv32/picorv32.routed.def
```

- Clean run → commit `picorv32.drc.reference.json`.
- Dirty run (Magic runtime blow-up, nondeterminism, parser drift) →
  defer explicitly: no picorv32 artifacts, reason in
  `docs/results.md` + `tests/designs/README.md`.

## 5. `docs/e7-drc.md`

Author the taxonomy doc per `taxonomy.md`'s outline: the three
surfaces, the per-category table + representative verbatim names, the
claim bound for `grus-eda drc`, the sanity-control outcome. Resolve
any `unclassified` names or `violated_rules_in_checked_deck: false`
explicitly.

## 6. `docs/results.md` + `tests/designs/README.md` + `docs/roadmap.md`

- `docs/results.md`: `### 2026-11-23 — e7-drc-ref: ...` section —
  commands verbatim + image; Magic version; counts (magic total,
  drt_drc_count + its `drt_report_source`, antenna_violations, broken
  total); taxonomy summary (category counts); artifact table with
  sizes + SHA-256 prefixes; rerun count; what was run vs not
  (exploratory probes named); no version bump stated.
- `tests/designs/README.md`: artifact row for
  `<d>.drc.reference.json` in the §2 table; extend the design entries
  naming `generate_drc_ref.sh <design> <routed-def>` (mirror the
  CTS/irdrop paragraphs).
- `docs/roadmap.md`: `- Progress 2026-11-23 (ticket \`e7-drc-ref\`):
  ...` line under the E6/E7 section, same style as the E5/E8 lines.

## 7. Verify

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

Both must pass (verify commands). No version bump — reference-only,
nothing Cargo-visible changed; state that in the commit per the
acceptance item.

## Done-when map (acceptance checklist)

| Acceptance item | Where handled |
|---|---|
| `generate_drc_ref.sh` runs in the pinned image, emits `<d>.drc.reference.json` (gcd + picorv32-if-present): Magic total + per-rule, version, DRT count, antenna count — all script-emitted | `script.md` steps A–E; `golden.md` |
| Magic per-rule breakdown (`drc listall why`), not just total | step C `DRCREF-WHY`; `magic_per_rule` + `magic_listall_why_count` |
| Sanity control: named mutation → nonzero Magic DRC + named violated rule | `sanity.md`; `sanity_control` block; step-D assertions |
| Rule-category taxonomy committed (LEF-expressible vs antenna vs Magic-only) | `taxonomy.md`; `magic_rules_checked` + `docs/e7-drc.md` |
| Rerun byte-reproduces the JSON (or varying metadata named) | `runtime_s` named field; steps 3 |
| `docs/results.md`: commands verbatim, counts, taxonomy summary, runtime; `cargo test` + `clippy` pass; no-bump stated | steps 6–7 |

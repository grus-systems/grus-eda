# e8-irdrop-ref — Steps

## 1. Write `tests/designs/generate_irdrop_ref.sh`

Implement `script.md` end to end (host wrapper → step A → step B →
step C → step D). Keep the Tcl line-per-command and `== 0` rules.

## 2. Run gcd

```bash
mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8
bash tests/designs/generate_irdrop_ref.sh gcd
```

Then fix what the loud failures find, in order of likelihood:

- The `analyze_power_grid` report grammar — write the step-B regexes
  against the actual `irdrop_a.log` lines, not against any assumed
  format. The verbatim `report_lines` field exists precisely so the
  grammar is pinned in the golden.
- Whether `report_power -instances [get_cells *]` covers all 384 DEF
  components or skips the 151 physical cells — whichever it does is a
  measured fact; the parser records it (or aborts on an unexplained
  middle ground, per `script.md` step B.4).
- VGND wording — `drop` vs `bounce` semantics come from the report
  itself; do not assume.

Record the actual grammar/wording found in `docs/results.md`.

## 3. Byte-reproducibility check (the committed-script rule)

Re-run `bash tests/designs/generate_irdrop_ref.sh gcd` and `git diff`
every committed artifact. The acceptance asks for byte-reproduction on
re-run; record the rerun count in `docs/results.md` (§7.1: the
committed script, run as written, is what produces the golden). Any
nonzero diff → name the nondeterministic field and fix it (the
`*DATE`-normalization precedent) or record it as a named residual —
never commit a golden that only reproduced once by luck.

## 4. picorv32

`picorv32.placed.def` must exist (gitignored — the worktree seed or
`tests/designs/picorv32/generate_ref.sh` produces it; 10,205
components, VPWR/VGND SPECIALNETS, PIN VPWR/VGND bterms).

```bash
bash tests/designs/generate_irdrop_ref.sh picorv32
```

- Clean run → commit `picorv32.irdrop.reference.json`; CSVs per the
  < 1 MB rule (`golden.md`), `.gitignore` lines + SHA-256 prefixes if
  needed.
- Dirty run (solver runtime blow-up, nondeterminism, parser drift) →
  defer explicitly: no picorv32 artifacts committed, reason recorded in
  `docs/results.md` and `tests/designs/README.md`.

## 5. `docs/results.md` entry

New `### 2026-11-23 — e8-irdrop-ref: ...` section containing:

- The commands verbatim (`generate_irdrop_ref.sh gcd`, `picorv32` if
  run) and the container image.
- Per net × convention: connectivity verdict, total power, supply,
  avg voltage, avg + worst drop/bounce, analyzed counts — vs the P-IRD
  scoping numbers (total power 1.04 mW, supply 1.80 V, avg drop
  0.223 mV, worst 0.591 mV, 384+ CSV rows on gcd VPWR). Confirm or
  correct each scoping number; if the SDC read moves the embedded total
  off 1.04 mW, say so explicitly (`conventions.md` §"scoping gate").
- The convention decision + measured delta.
- Artifact sizes + SHA-256 prefixes (first 16 hex) for every committed
  and every gitignored CSV.
- Rerun count for byte-reproducibility; which steps were run vs not.
- picorv32: included or deferred with the reason.

## 6. `tests/designs/README.md` + `docs/roadmap.md`

- `tests/designs/README.md`: artifact rows for `gcd.irdrop.reference.json`
  and the `gcd.irdrop.voltage*.csv` set in the §2 table; extend the
  gcd design entry (and the picorv32 entry if included) with a sentence
  naming `generate_irdrop_ref.sh <design>`, mirroring the CTS-variants
  paragraph.
- `docs/roadmap.md` §E8: a `- Progress 2026-11-23 (ticket
  \`e8-irdrop-ref\`): reference committed — ...` line naming the pinned
  convention and the measured numbers, same style as the E5 lines.

## 7. Verify

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

Both must pass (verify commands). No version bump — reference-only,
nothing Cargo-visible changed; state that in the commit per the
acceptance item.

## Done-when map (acceptance checklist)

| Acceptance item | Where handled |
|---|---|
| committed script, `check_power_grid` + `analyze_power_grid -voltage_file` on VPWR+VGND on `gcd.placed.def` in the pinned image, rerun byte-reproduces | `script.md`; step 3 |
| `gcd.irdrop.reference.json` — connectivity, total power, supply V, avg voltage, avg+worst drop/bounce, counts; never hand-written | `golden.md`; step B/D parsers |
| both conventions measured; mirrored convention pinned with delta | `conventions.md` |
| per-instance CSVs < 1 MB committed, else gitignored + size/SHA | `golden.md` |
| picorv32 included or deferred with reason | step 4 |
| `docs/results.md` entry — commands verbatim, vs P-IRD numbers, sizes, SHA prefixes, rerun count | step 5 |
| `cargo test`, `clippy -D warnings` pass; bump decision stated | step 7 |

# Implementation steps

One logical unit per commit; workspace version bump lands in the functional
commit (patch → 0.1.96). Commit messages: Conventional Commits + agent
trailer per `AGENTS.md` §5.

## Step 1 — measured semantics + fixture generation (container work first)

Write `tests/designs/microbench/vcd2saif.py` and
`tests/designs/microbench/generate_saif_ref.sh` (`fixture.md`), run them
inside `ghcr.io/librelane/librelane:3.0.16` via Colima, inspect `mb.saif`
plus `mb.saif.reference.json` against `mb.openroad.txt`, then commit:

```
test: add microbench SAIF fixture and OpenSTA-SAIF golden
- vcd2saif.py converts mb.vcd waveform to SAIF aggregates (T0/T1/TX/TC)
- generate_saif_ref.sh: container run emits mb.saif + mb.saif.reference.json
  (power_saif + power_vcd blocks, parsed from report_power -digits 6)
```

Confirm before commit: rerun the committed script end-to-end and `git diff`
is empty (AGENTS §7 rule 1).

## Step 2 — SAIF reader

`readers/saif.rs` per `api.md` + `readers/mod.rs` wiring. Parser unit tests
cover the `saif-format.md` grammar and the error/edge cases listed there.
Commit: `feat: add SAIF switching-activity reader`.

## Step 3 — wiring: resolver + diagnostics + CLI

`power.rs` diagnostics rename + two SAIF counters; `saif.rs::resolve`;
`main.rs` `--saif`/`--saif-scope` args and the merge into
`report.diagnostics`; print line source-neutral. Regenerate `mb.power.json`
in this commit. Unit/integration coverage per `api.md` (without the golden
assertion yet if the reference file landed in step 1 — the strict test comes
in step 4). Bump `[workspace.package] version` → 0.1.96 in this commit and
let `Cargo.lock` follow. Commit: `feat: power --saif input with scope diagnostics`.
(If steps 2 and 3 land as one commit, the single bump lives there —
the reader alone is dead code until wired, so folding them is fine.)

## Step 4 — STRICT golden test + docs

`test_saif_power_mb_matches_opensta` (`fixture.md`): measure actual
residuals, set `POWER_SAIF` to the tightest honest bounds, name any residual
> ~2% in `docs/results.md` before writing the bound. Fail-proof check
(corrupt TC → red → restore). `docs/results.md` dated entry per `probe.md`
P4; `docs/roadmap.md` N7 → SAIF struck (`when`-blocks and statistical
activity propagation stay open); `README.md`/`main.rs` help only where the
CLI surface is documented. Commit: `test: SAIF power golden at POWER_SAIF
tolerance` (+ `docs:` if separate).

## Verification gates

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
- `bash tests/designs/microbench/generate_saif_ref.sh` idempotent (rerun →
  clean `git status`)
- `grus-eda power --saif tests/designs/microbench/mb.saif` end-to-end CLI
  run recorded (stderr shows unmatched counts if ghost included)

## Out of scope (stays open)

- `when`-qualified `internal_power` blocks (`conditional_power_blocks`
  diagnostic already counts them).
- Matching OpenSTA's statistical activity propagation on large designs
  (gcd-class divergence, already documented for VCD).
- State-dependent leakage from `T0`/`T1` duty (Liberty `leakage_power` `when`
  conditions are not modeled; flat `leakage_power_w` stands).

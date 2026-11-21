# docs — `results.md` entry and `roadmap.md` N7 rewrite

## `docs/results.md` — new entry

```
### 2026-11-22 — N7 — AP4: user overrides + gcd re-comparison (v0.3.0)
```

Sections, in order:

1. **What landed** — `ActivityOrigin::User` (the measured `-pins` /
   `-input_ports` source; `input_ports` no longer folds into
   `annotated`), `ActivityOverrides::pins` (`Vec<(PinId,
   ActivitySetting)>`, call-site name resolution, driver-only like a
   dump annotation, `dead_user_entries` diagnostic), the final seed
   precedence table (constant → clock → global → pins → input_ports →
   dump → input → propagated/unknown), `Copy` dropped → minor bump.
   `-activity` vs `-density` call-site contract restated with `s_p40`
   as the pin.
2. **Golden** — `s_input_ports`/`s_p40` verbatim (now native `user`
   label); `s_input_ports` `origin_counts` `{clock 1, constant 4,
   input 1, propagated 6, unknown 4, user 2}`; the `pins`≡`input_ports`
   equivalence check; `dead_user_entries == 0` everywhere; group
   columns unchanged at the established bounds (the user/origin
   relabel moves no watt column — `s_input_ports` watts were already
   asserted).
3. **gcd re-comparison** — per `gcd.md`: command, inputs, provenance
   note, three-column table (OpenSTA / ours-then / ours-now),
   moved-toward vs still-diverging analysis, named residuals,
   origin-split + diagnostics output, the no-annotation secondary
   run.
4. **Residual notes** (the honest-divergence record for the closed
   sequence — restate so N7 stands alone):
   - per-pin vs per-net labels: `s_full` `y` stays
     `PER_PIN_RESIDUAL`; a non-driver pin on a seeded net reads
     `propagated` in OpenSTA while our net keeps the driver's label
     (`propagated` arm tolerance covers it — the convention
     difference, named);
   - `unknown` carries `prob_one: Some(0.0)` — expressible `when`
     literals at weight 0 (chosen, unmeasured — OpenSTA's own weight
     unmeasured);
   - `user` vs dump precedence chosen (unmeasured); `pins` vs
     `input_ports` order chosen (unmeasured); last-wins on duplicate
     `pins` (unmeasured);
   - OpenSTA's split counts `user` pins under `input N`; ours reports
     `user N` per net — recorded difference;
   - unchanged AP3 residuals still named: expression-form
     `next_state` (26 cells), `Sequential` origin never emitted,
     `prob_one: None` data nets taint the Q copy.
5. **Moved goldens** — `mba.activity.reference.json` byte-identical;
   `mb.power.json` regenerated, `git diff` empty; all other
   `*.reference.json` `git status`-clean.
6. **Fail-proof** — the three proofs from `golden.md`.
7. **Checks run** — `cargo test --workspace`, clippy `-D warnings`,
   `cargo test --release -p grus-core sta`, the gcd CLI runs (exact
   commands), the `mb.power.json` regen.

## `docs/roadmap.md` — N7 rewrite

Current text: "On `gcd` the totals diverge because OpenSTA propagates
activity statistically (BDD + `findSeqActivity`) rather than reading
per-net VCD counts; our exact per-net counts are reported instead, and
the residual is documented in `docs/results.md`."

Replace with the now-measured record:

- AP1–AP4 **done** (v0.2.14–v0.3.0): `resolve_activity` implements
  the measured OpenSTA statistical model — seed precedence (constant →
  clock → global → `user` `-pins`/`-input_ports` → VCD/SAIF pin
  annotation → input/undriven default), combinational local-marginal
  propagation (`P`, `d` via Shannon sensitization), sequential Q=D
  copy with `unknown` feedback cones, pin-namespaced annotation with
  dead-entry diagnostics.
- The gcd `report_power` residual is now *measured against the same
  annotation seeds* — the remaining delta (whatever the AP4 entry
  records: expected wire-RC switching residual + any M1-correlation
  gap) replaces the old "model not implemented" divergence; point at
  the 2026-11-22 results entry for the numbers.
- Keep the standing tolerances sentence (`mb.saif` ~0.4%, `mbw`
  `when` ~1e-7%) unchanged.
- Drop "AP4 scoped annotation precedence remains open"; the
  per-pin/per-net label residual and `unknown` `prob_one`/`when`
  notes move here as named limitations.

`docs/status.md` — untouched (it carries no power section).

## Files

- `docs/results.md` — the AP4 entry
- `docs/roadmap.md` — N7 rewrite

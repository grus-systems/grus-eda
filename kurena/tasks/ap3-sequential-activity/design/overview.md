# AP3 — sequential activity rules: overview

Third ticket of the AP1–AP4 sequence
(`kurena/tasks/activity-propagation/design/api.md` §Plan; measured
semantics: `docs/results.md` 2026-11-21 N7 probe, esp. P3; golden:
`tests/designs/microbench/mba.activity.reference.json`, 12 sessions ×
43 pins).

AP3 lands the measured **S1** verdict: a sequential output pin's net
copies the resolved `{P, density}` of the net on the cell's data pin —
origin `Propagated` (OpenSTA prints no `sequential` source) — and every
net the seeds + propagation still cannot resolve converts to `Unknown`
`{0, 0}`. After it, `EXPECTED_DIVERGENT` is empty and all 12 mba
sessions read verbatim.

The 13-pin AP2 residual is exactly:

- net `q` (`u_df/Q` + output port `qd`): Q=D copy of `a` —
  `3e6/0.55` (`s_ports`/`s_full`), `6e6/0.55` (`s_ports_a2x`),
  `3e6/0.40` (`s_xz`), `5e6/0.5` (`s_none`), `1e7/0.4` (`s_input`,
  `s_input_ports`), `2.5e6/0.5` (`s_p40`);
- the 11 `unknown`-cone pins on nets `n_q0`, `n_q0n`, `n_q1`, `n_c1d`
  (toggle flop `u_f0`+`u_i0`, counter bit `u_f1`+`u_x1`): `{0, 0}`
  `unknown` in every non-global session — OpenSTA does not resolve
  sequential feedback loops (no divide-by-2 even though `s_full`'s
  waveforms carry the exact `5e7`/`2.5e7` rates; those flat-name dump
  entries are dead anyway).

## What lands

1. **Data-pin identification on `LibCell`** (`readers/liberty.rs`,
   `lib_db.rs`). The `ff`/`latch` block parse stops at
   `is_sequential`/`is_latch` today and drops `next_state`/`data_in`.
   New: parse the block body, keep the two state-variable names and the
   data-pin name when `next_state`/`data_in` is a bare identifier.
   Complex forms (scan mux `"(D&!SCE) | (SCD&SCE)"`, data-enable
   `"(D&DE) | (IQ&!DE)"`, scan+enable) stay a named residual — the
   sky130 census is in `census.md` and goes into `docs/results.md`.
   `LibCell` layout changes → `CACHE_VERSION` bumps 8 → 9.
2. **Q = D copy** (`activity.rs`). A `seq_funcs` table parallel to
   `comb_funcs`: per net driven by a sequential/latch output pin, the
   cell's resolved data-net plus the output polarity (pin `function`
   matching state var = verbatim, matching var_N = `1−P`). Emit
   `Propagated` — the golden label; `ActivityOrigin::Sequential` stays
   declared-but-unemitted.
3. **Fixed point + `unknown` conversion** (`activity.rs`). The
   combinational Kahn pass generalizes to both producer kinds in one
   dependency graph — a resolved Q unblocks downstream combinational
   candidates in the same pass (Q→gate→D→Q chains resolve; feedback
   cycles never dequeue). After the pass, every net still `Default`
   becomes `Unknown {prob_one: Some(0.0), eff_toggles: 0.0}` — the
   measured semantics for sequential-cone leftovers; the same
   conversion also covers unparseable-function and unclassified-driver
   leftovers (unmeasured, recorded). `Unknown` propagates as taint
   exactly like `Default` (already in the eval rule).
4. **Golden coverage** (`power.rs` test). `EXPECTED_DIVERGENT` →
   `&[]`; the golden `unknown` source moves from the divergent arm to a
   strict `{d, P, label}` compare; all 12 sessions assert per-pin
   verbatim — which adds `s_input_ports` (new
   `ActivityOverrides::input_ports` user-annotation seed) and `s_p40`
   (new committed `mba_p40.sdc`, verbatim the period-40 SDC the
   generator writes to `/tmp`). Group columns for every session move
   from `eprintln` deltas to asserted bounds (measured first, recorded).
5. **Docs** (`docs/results.md`, `docs/roadmap.md`). AP3 entry: the
   13-pin residual closed, group-column residuals now, the sky130
   `next_state`/`data_in` census, the Q_N inversion rule, the `prob_one`
   choice on `unknown` nets, moved goldens enumerated, fail-proof
   record. Roadmap N7 → AP3 done.

## What does not land

- Complex `next_state`/`data_in` expressions — scan mux, data-enable
  mux, scan+enable terms in the expression (sdf*/edf*/sedf* cells, 26
  of 63 sequential cells — `census.md`). Their output nets stay
  `Default` → `Unknown`. Named residual.
- Async `clear`/`preset`/`clocked_on`/`enable` semantics — ignored by
  the copy rule (the measured `dfxtp` has none; the choice is
  recorded). `statetable` groups stay unparsed (6 latch cells).
- Per-pin annotation vs per-net precedence — `PER_PIN_RESIDUAL`
  (`s_full` `y`) stays AP4. `-pins` per-pin seeds, `-duty` on
  annotations, gcd `report_power` re-comparison — AP4.
- `ActivityOrigin::Sequential` — never emitted (OpenSTA prints
  `propagated` for the Q=D copy).
- `prob_one: None` data nets (x-only annotation) taint the Q copy like
  a combinational input — recorded, unmeasured.

## Decisions recorded

- **Unified producer worklist**, not a round-based interleave: seq-copy
  nodes join the combinational Kahn graph as single-dependency nodes.
  Dequeue-once semantics, one fixed point by construction, and the
  taint rule is shared. Round-based (`loop { comb; seq } until quiet`)
  is observationally equivalent — the worklist form avoids re-evaluating
  tainted candidates per round.
- **Eval-failure posture unchanged**: a comb eval `None` keeps `Default`
  at dequeue and converts to `Unknown` at the end — identical outcome
  to early-marking, since `Default`/`Unknown` taint identically.
- **`input_ports` beats a dump annotation** on the same port net
  (explicit user set wins — OpenSTA's `user` source vs `vcd`; no
  committed session mixes them, recorded). `-global` still beats it
  (measured p0b precedence), and `clock` beats both (`s_input_ports`'s
  `clk` reads `clock`).
- **`prob_one: Some(0.0)` on `Unknown`** — matches the golden's printed
  `0.000`; a `when` literal on an unknown net is then expressible at
  weight 0 rather than inexpressible (`conditional_power_blocks`).
  OpenSTA's own `when` weight on `unknown` nets is unmeasured —
  recorded.
- **Origin label for the copy is `Propagated`** — measured (every
  session's `u_df/Q`/`qd` read `propagated`).
- **Undriven ≠ port-driven for `input_ports`**: `s_input_ports`'s
  `n_float` keeps the `input` default `5e6/0.5` — the override applies
  only to nets driven by an io pin.

## Files

- `crates/grus-core/src/activity.rs` — `seq_funcs` table, unified
  propagation pass, `Default`→`Unknown` conversion, `input_ports` arm,
  doc-comment updates
- `crates/grus-core/src/readers/liberty.rs` — `ff`/`latch` body parse
  (`next_state`/`data_in` + state vars), `CACHE_VERSION` bump
- `crates/grus-core/src/lib_db.rs` — `LibCell` sequential fields
- `crates/grus-core/src/power.rs` — `LibCell` literal, mba test rework
- `tests/designs/microbench/mba_p40.sdc` — new (verbatim the
  generator's `/tmp/mba40.sdc`)
- `docs/results.md`, `docs/roadmap.md`, `Cargo.toml`, `Cargo.lock`
- `kurena/tasks/ap3-sequential-activity/design/` — this document set

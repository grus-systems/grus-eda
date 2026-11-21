# Goal

AP4 — the closing ticket of the activity-propagation
sequence (authoritative semantics:
`kurena/tasks/activity-propagation/design/api.md`; golden
`tests/designs/microbench/mba.activity.reference.json`).

AP1–AP3 already landed the skeleton, defaults, combinational
propagation, sequential rules, pin-namespace annotation, and
x/z (`s_xz` pinned verbatim). What remains is the `user`
override class, final precedence, and the real-design
re-comparison that proves the whole model end to end.

## What lands

1. **`user` activity overrides** — the measured
   `set_power_activity` surface our API lacks:
   - `-input_ports [all_inputs]` → port-scoped override,
     origin `user` (golden `s_input_ports`: `a`/`b` read
     `user`, everything else propagates — `input` default
     survives only on the undriven `u_flt/A`);
   - `-pins <list>` → per-pin override, also `user`;
   - `-activity a` is clock-normalized (`a × f_clk` —
     `s_p40` pins this: period 40 halves the resolved
     density) vs `-density d` absolute — keep the API's
     absolute-rate contract and put the normalization at
     the call site;
   - precedence: `user` sits above `input`-default and
     below `global`/`clock`/`constant` per the measured
     label order (api.md §2); `-global` after `-input`/
     `-input_ports` still wins on every non-clock
     non-constant pin (the committed side-probe recorded
     it).
   CLI exposure is optional; the API (`ActivityOverrides`
   or the shape it grew) must express them.

2. **Golden coverage** — extend the mba activity test:
   `s_input_ports` and `s_p40` per-pin `{d, P, source}`
   verbatim (they were pinned in the golden but only
   reached by the seed set AP4 adds); re-assert all 12
   sessions still match. Fail-proof: drop the `user`
   arm → red on `s_input_ports`; corrupt `s_p40` → red;
   restore.

3. **gcd re-comparison** — rerun `report_power` on the
   committed gcd fixture with the propagated model (no
   annotation file and/or the existing gcd annotation —
   whichever the design doc picks) against the OpenSTA
   numbers already in `docs/results.md`; update the N7
   residual table with the new deltas. State what the
   model buys us (which columns moved toward OpenSTA)
   and what still diverges — honest residuals, not
   marketing.

4. **Roadmap + docs**: N7 entry updated to reflect the
   closed sequence (AP1–AP4), the intentional-divergence
   record rewritten as the now-measured model; record any
   per-pin/per-net label residuals left (the annotated-
   net-instance-pins `propagated` convention, the
   `unknown` `prob_one`/`when` semantics note).

## Files

- `crates/grus-core/src/activity.rs` (user origin +
  precedence; likely small)
- `crates/grus-core/src/power.rs` (test extension; gcd
  test if the re-comparison is a test, else results.md
  only)
- `crates/grus-eda/src/main.rs` (only if CLI flags land)
- `docs/results.md`, `docs/roadmap.md`
- `kurena/tasks/ap4-activity-precedence/` (design docs)
- `Cargo.toml` / `Cargo.lock`

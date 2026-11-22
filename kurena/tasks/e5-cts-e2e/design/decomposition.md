# e5-cts-e2e — Decomposition and attribution

Every outside-bound number gets a named cause before the results entry
closes (AGENTS.md §7.5; the `e4-sta-e2e` decomposition precedent).
Four runs produce the rows; all print under
`test_<d>_cts_decomposition` (`--nocapture`).

## The four axes

| Axis | Left | Right | Isolates |
|---|---|---|---|
| `engine` | our `sta` + met2 placement RC on `<d>.cts.def` (the TritonCTS tree) | `cts_placement_rc` golden | the STA engine's residual on an *identical* netlist — should land in the `GCD_PLACEMENT`/`PICORV32_SPEF` engine band |
| `tree-shape` | our `sta` on our emitted CTS DEF | our `sta` on `<d>.cts.def` | what our tree's latency/skew map does to endpoint timing, engine held fixed |
| `total` | our `sta` on our emitted CTS DEF | `cts_placement_rc` golden | the gated criterion-3 diff |
| `reference-setup` | golden `clock_pins` spread + `latency_*` | `skew_setup_ns`/`skew_hold_ns` | how much of "skew" is measurement convention: `latency_max − latency_min` (36.209 ps on picorv32) vs the signed worst-pair `worst_clock_skew` (−34.165 ps); also step-A (in-CTS-process) vs step-C (clean process) agreement |

Each row prints per-field max diff and the worst-5 endpoints by
absolute diff (pin, ours, ref, diff) — the `print_scores` shape from
the e4 decomposition, reimplemented compactly since those helpers are
private to `analysis::tests`.

## Clock-pin rows

The same three STA runs also emit the per-pin `arr_rise_max`
comparison at the register clock pins:

- `engine`: our latencies on the ref DEF vs golden `clock_pins` —
  **the load-bearing row for the picorv32 skew question.** If our
  engine reproduces ~0.757–0.793 ns / 36 ps spread on TritonCTS's
  tree, the 70.6 ps spread is incontrovertibly tree-side; if it does
  not, part of the gap is engine and must be named that way.
- `tree-shape`: our latencies on ours-def vs ours on ref-def — the
  actual tree difference (which pins moved how much, worst-5 with the
  driving `clknet_*` net named).
- `reference-setup`: golden `clock_pins` spread vs `latency_*` spread
  vs `|skew_setup_ns|` — convention noise floor for criterion 1.

## Attribution rule

For any endpoint or clock pin outside its gate bound:

- `engine` diff large → STA-engine residual (compare with the
  recorded placement-RC band; a same-input engine drift is a real
  regression, not a CTS matter).
- `tree-shape` diff large → our tree's latency/skew field (buffer
  placement in free gaps, no trunk topology, different per-pin
  insertion delay — the legitimate freedom of criterion 4) or, on
  picorv32's 34.4 ps skew gap, a named tree-side cause: depth-balance
  equalizes *buffer count*, not per-pin RC — the residual hypothesis
  to confirm or refute with the per-pin table.
- `reference-setup` diff large → the golden's own surfaces disagree;
  criterion bounds were written against the latency surface, name it.
- `total` large with all three axes small → a new interaction:
  investigate, do not loosen.

The verdict language in `docs/status.md`/`docs/results.md` uses "met"
or "named residual: <axis> — <cause>" per criterion per design, never
an unexplained bound.

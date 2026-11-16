# Step 4 ticket 10 — latch endpoints and clock-gating checks (ibex E3 unblocker)

## Background

Ticket 09's SDC parser and latency handling are confirmed by the golden:
on the 3,700 endpoints both sides share, ibex zero-wire matches with **zero
failures** at the strict tolerance (worst diff ~8e-6 ns; WNS -8.7635 ns and
TNS -12,328.7 ns identical). The only gap is the endpoint count: our STA
counts **3,700**, the golden has **3,702**. The two missing endpoints are
both in ibex's clock gate (`core_clock_gate_i`, from `ibex.synth.v`):

- `_21829_` is `sky130_fd_sc_hd__dlxtn_1` (a negative-level latch): `.D(_00000_)`,
  `.GATE_N(clk_i)`, `.Q(\core_clock_gate_i.en_latch )`. The golden has an
  endpoint at **`_21829_/D`** with max_req 5.095 ns (= 4.0 + clock latency
  1.095), min_req 0.992889 ns, max_arr 5.455698 ns, max_slack 3.639301 ns.
- `_12469_` is `sky130_fd_sc_hd__and2_2`: `.A(clk_i)`,
  `.B(\core_clock_gate_i.en_latch )`, `.X(clk)`. The golden has an endpoint
  at **`_12469_/B`** — the enable input of the clock-gating AND cell — with
  max_req 9.095 ns (= 8.0 + 1.095), min_req 5.095 ns, max_arr 5.455698 ns,
  max_slack 3.639301 ns.

This is the same family as ticket 06 (async endpoints): our STA only
collects D pins of edge-triggered cells and primary outputs, and
`docs/status.md` already records "latches are skipped with a warning" as a
known limitation. Note the latch's Q feeds only the gating AND's enable, not
a data path, and the AND's output is the clock net — so this ticket is
endpoint-only: no data-propagation change through the latch is needed for
ibex (state that explicitly if you find otherwise).

## Files

- Modify: `crates/grus-core/src/sta/analysis.rs` (endpoint collection and
  the check model; test module only for the un-ignore)
- Modify: `crates/grus-core/src/sta/liberty.rs` or wherever the timing-arc
  model lives, only if the latch / gating arcs need new representation
- Do not touch: `crates/grus-core/src/sta/sdc.rs` (ticket 09's parser is
  done and confirmed)

## Work

1. **Latch D as an endpoint.** Read the Liberty timing arcs for the
   `dlxtn_1` cell (setup/hold on D relative to GATE_N) and add the latch D
   pin as an endpoint with the required times OpenSTA reports for it. The
   golden values above are the spec: derive the formulas from the arcs and
   the clock (period 8.0, latency 1.095), do not hard-code the golden
   numbers. Generalise to the other latch cells in the library
   (`dlxtp`, `dlclkp`, …) if their arcs have the same shape; if a cell's
   arcs need a different rule, implement only `dlxtn` and say so.
2. **Clock-gating check on the enable pin.** Detect the structural pattern
   OpenSTA checks: a combinational cell whose output is a clock net and one
   of whose inputs is the clock, and add a setup/hold check on the
   non-clock input (here `_12469_/B` against `clk_i`). Derive the required
   times from the cell's delay arcs and the clock (golden: max_req 9.095,
   min_req 5.095). If the detection rule is ambiguous for other cells in
   this design, scope it to the pattern that fires exactly here and
   document the rule.
3. **Un-ignore** the three ibex STA tests in this landing.

## Acceptance

- Endpoint count is 3,702 on ibex (all three modes) and unchanged on
  gcd/aes/jpeg/picorv32 (the full suite on Intel is the regression check —
  the maintainer runs it).
- ibex zero-wire passes the strict tolerance on all 3,702 endpoints.
- The two wire modes are measured and recorded; if they fail they are
  `#[ignore]`d by the maintainer with the measured tables (the aes/jpeg
  precedent) — do not widen any shared tolerance in this ticket.
- `cargo clippy --all-targets -- -D warnings` is clean (note the `--`
  before `-D warnings`).
- Final report: the formulas used for the latch required times and the
  gating check (with the Liberty arc names they came from), the detection
  rule, and any design where the count changed unexpectedly.

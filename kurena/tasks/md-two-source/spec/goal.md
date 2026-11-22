# Goal

Close the recorded coverage gap in `docs/path-delays.md` section 5 item 3
(and the skewed-clock part of item 1): today every `sta_exceptions`
endpoint has exactly one launch source, so the delay-domain merge's
delay-winner vs arrival-winner pairing (`delay_launch_max`) is only
exercised by a unit test, never by OpenSTA golden data.

Extend `tests/designs/sta_exceptions/exceptions.def` so that `u_ff1/D`
is reachable from TWO sequential launch sources whose launch-clock
arrivals differ under a propagated clock:

- Insert one `sky130_fd_sc_hd__mux2_1` (`u_mux`) ahead of `u_ff1/D`:
  `A0` <- `u_ff0/Q`, `A1` <- `u_ff2/Q`, `X` -> `u_ff1/D`. Give its
  select pin a new top-level input port `sel` with an input delay in
  `base.sdc` (and the propagated variant `base_prop.sdc` if the
  scenario generator needs a separate base).
- Add a second `sky130_fd_sc_hd__clkbuf_2` (`u_clkb2`) so that `u_ff2`'s
  `CLK` is driven through its own buffer: `clk -> u_clkb2/A -> c2 ->
  u_ff2/CLK`, while `u_ff0`/`u_ff1` stay on `c1`. Under
  `set_propagated_clock` the two clock nets then carry different
  latencies, so source `u_ff2` launches later than `u_ff0` — the
  precondition for arrival winner != delay winner at `u_ff1/D`.
- Keep the DEF self-contained (PLACED positions inside the existing
  die area, new `sel` PIN beside `d_in`, net list updated). Every cell
  name must resolve in `sky130_fd_sc_hd`.

Add new scenarios to `tests/designs/sta_exceptions/` and to the
`SCENARIOS` list in `generate_ref.sh`:

- `m2_max0`/`m2_max2` — `set_max_delay 1.0 -from [get_cells u_ff0]` /
  `-from [get_cells u_ff2]` `-to [get_cells u_ff1]` (single-source
  coverage at a shared endpoint).
- `m2_both` — both sources constrained in one command or two commands.
- `m2_min2` — `set_min_delay 0.5 -from [get_cells u_ff2] -to
  [get_cells u_ff1]` (min side with skewed launches).
- `pm2_prop` — `set_propagated_clock` + `set_max_delay 1.0 -from
  [get_cells u_ff0] -to [get_cells u_ff1]` — the case where the
  arrival winner and the delay winner can diverge (the reason this
  fixture exists).
- `pm2_min2` — propagated clock + `set_min_delay` from `u_ff2`
  (launch-vs-capture anchoring under clock skew, section 5 item 1).

Regenerate `exceptions.reference.json` by running the committed
`tests/designs/sta_exceptions/generate_ref.sh` as written (it
re-invokes itself inside `ghcr.io/librelane/librelane:3.0.16`; docker
is available). Reference values must come from that run only — never
hand-write or round them.

If `test_sta_exceptions_match_opensta` then shows a real divergence
between our delay-domain composition and OpenSTA on the new scenarios,
fix the composition in `crates/grus-core/src/sta/analysis.rs` (the
delay merge / launch pairing), not the test. If the divergence is a
fixture/model limit, record it in `docs/results.md` instead of
loosening tolerances silently.

Files touched: `tests/designs/sta_exceptions/exceptions.def`,
`tests/designs/sta_exceptions/*.sdc` (new scenarios, possibly
`base.sdc` for the `sel` input delay), `tests/designs/sta_exceptions/
generate_ref.sh` (SCENARIOS list),
`tests/designs/sta_exceptions/exceptions.reference.json`
(regenerated), `crates/grus-core/src/sta/analysis.rs` (only if a real
divergence needs a fix), `docs/results.md` (comparison entry),
`docs/path-delays.md` (mark the exercised items resolved).

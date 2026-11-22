# Goal

Close `docs/path-delays.md` section 5 item 3 (the diverging-winner
half): today every merged delay domain has the SAME source as both
arrival winner and delay winner, so the `delay_launch_max` pairing —
the launch-clock arrival that must travel with the delay winner — is
not discriminated by OpenSTA golden data. The current numbers
(`exceptions.reference.json`):

- `u_ff0` -> `u_mux/A0` (small pin cap): pure delay 0.717891 ns
  (`pm2_prop`)
- `u_ff2` -> `u_mux/A1` (larger pin cap): pure delay 0.719321 ns
  (`pm2_both` winner) AND earlier launch on `c2` (`u_clkb2` sees one
  load vs `u_clkb`'s two), so `u_ff2` wins both domains.

The fix is a two-line swap in `tests/designs/sta_exceptions/
exceptions.def`: swap the mux data pins so `u_ff0` drives the
higher-capacitance input and `u_ff2` the lower one:

- net `d0`: `( u_ff0 Q ) ( u_mux A1 )`  (was `A0`)
- net `d2`: `( u_ff2 Q ) ( u_mux A0 )`  (was `A1`)

After the swap `u_ff0` owns the larger pure data delay (~0.719 ns)
but launches later on `c1` (~8.5 ps behind `c2`, measured on
`pm2_both`), while `u_ff2` launches earlier with the smaller delay.
Since the skew (~8.5 ps) is much larger than the delay difference
(~1.4 ps), the arrival winner stays `u_ff2` while the delay-domain
winner becomes `u_ff0` — the divergent pairing this fixture exists
to discriminate. If the regenerated golden shows the swap did NOT
produce that split (e.g. the delay difference grew past the skew),
enlarge the `c1`/`c2` skew by adding a clock load (e.g. an extra
`clkbuf` output pin or a third flop on `c1`) — never by hand-editing
expected values.

Then regenerate `tests/designs/sta_exceptions/
exceptions.reference.json` by running the committed
`generate_ref.sh` as written (it re-invokes itself inside
`ghcr.io/librelane/librelane:3.0.16`; docker is available). The swap
shifts every scenario whose report touches `u_ff1/D`, so all 72
scenarios must be re-measured — never hand-write or round values.

Verify the divergence is real in the regenerated golden: compare
`pm2_prop` (u_ff0-only delay check) against `pm2_both` (merged) and
`m2_max0`/`m2_max2`, and confirm from the numbers that the delay
domain reports `u_ff0`'s candidate while `base_prop`'s `u_ff1/D`
max arrival still comes from `u_ff2`. If `test_sta_exceptions_
match_opensta` then fails, the pairing in
`crates/grus-core/src/sta/analysis.rs` is wrong — fix the
composition (`delay_launch_max` tracking / merge), not the test,
unless the cause is a fixture/model limit (then record it in
`docs/results.md` instead of loosening tolerances).

Also update the stale numeric literal in the `analysis.rs` test
comment the previous review flagged (`4.562147` -> whatever the
regenerated fixture makes it; it is already stale at 4.550057).

Files touched: `tests/designs/sta_exceptions/exceptions.def`,
`tests/designs/sta_exceptions/exceptions.reference.json`
(regenerated), `crates/grus-core/src/sta/analysis.rs` (only if the
divergence exposes a pairing bug, plus the stale test-comment
literal), `docs/results.md` (comparison entry), `docs/
path-delays.md` (section 5: mark the exercised part of item 3
resolved; CRPR credit and the propagated port-launch anchor stay
open), `Cargo.toml` (patch version bump in the same commit).

# Where the model assumes one clock — audit + our-side probes

This file is the "where we break" checklist. The audit below is from
reading HEAD (`v0.1.97`); the probe runs confirm the observable
behaviour and the exact messages. Line numbers are the planning-stage
read — re-verify before writing `docs/results.md`.

## 1. Parse-side (`crates/grus-core/src/sta/sdc.rs`)

| site | what it does today | two-clock symptom |
|---|---|---|
| `Sdc` fields (`sdc.rs:241-261`) | one `clock_name`/`clock_port`/`period_ns`, one `setup/hold_uncertainty_ns`, one `clock_latency_ns`/`clock_source_latency_ns`, one `propagated_clock`, one `clock_transition_ns` | no storage for a second domain at all |
| `create_clock` (`sdc.rs:841-870`) | second source-port clock hits `bail!("a second design clock is not supported")` at `:865` (guard added in `e921217`; before it the second clock overwrote name/port/period) | **parse bail** — the first observable break |
| `ClockRef` (`sdc.rs:159-163`) | two-valued enum `{Design, Virtual}` — a domain index cannot be expressed | `resolve_clock` maps every name into one of two buckets |
| `resolve_clock` (`sdc.rs:1679-1694`) | matches the single design name, else virtual list, else `bail!("unknown clock")` | a second domain's name bails wherever it is referenced (`-clock` on delays, `get_clocks` on latency/uncertainty/propagated) |
| `set_clock_uncertainty` (`sdc.rs:879-923`) | resolves every named clock but writes the single global fields; no `-from`/`-to` flags (they bail as unsupported options) | per-clock values overwrite one slot; inter-clock (`-from` `-to`) form refuses to parse |
| `set_clock_latency` (`sdc.rs:933-982`) | `[get_clocks {a b}]` bails `set_clock_latency takes one clock`; single name resolves to `ClockRef` and lands on the design or the shared virtual slot | multi-name form bails; second domain unnameable |
| `set_clock_transition` (`sdc.rs:1034-1055`) | iterates `all_clock_names`, applies only `ClockRef::Design`, counts virtuals ignored | a second domain would be counted ignored, not applied |
| `set_propagated_clock` (`sdc.rs:989-1030`) | per-name `resolve_clock`; `set` requires the design clock among names | second-domain propagation unnameable; `set_propagated_clock [all_clocks]` works only because virtuals are tolerated |
| `set_clock_latency` `-early`/`-late` | refused outright (`bail!("unsupported option")`) | asymmetric per-clock latency also unsupported — same bucket |
| `delay_rule` `-clock` (`sdc.rs:1646-1675`) | resolves to `ClockRef::{Design,Virtual}` | `set_input_delay -clock <clk2>` bails `unknown clock` |
| `all_clock_names` (`sdc.rs:1244-1252`) | virtuals + single design name | `all_clocks` cannot name a second domain |
| `finish()` (`sdc.rs:1696-1709`) | bails if `period_ns <= 0` (design clock required); every virtual clock's period must equal `period_ns` | per-domain virtual periods rejected |
| virtual clocks (`sdc.rs:483`) | list of `(name, period)` but they share `io_clock_latency_ns`/`io_clock_source_latency_ns` and one period check | multiple virtual domains collapse to one latency |
| `create_clock -waveform` | not in `CLOCK_VALUE_FLAGS` (`sdc.rs:1853`) — `positionals` bails `unsupported option '-waveform'` | non-50% duty unsupported (same-domain too) — record, not in scope |
| exception/delay selectors | `Value::Clocks` in `-from`/`-to` → `obj_sel` returns `None` → command counted in `ignored_commands` (`sdc.rs:1790-1801`) | `set_false_path -from [get_clocks cka]` is silently-ish skipped (loud in the skip summary) |

## 2. Eval-side (`crates/grus-core/src/sta/analysis.rs`)

These sites only become reachable if the parse guard is bypassed —
they are the "what breaks next" list, confirmed by the scratch-build
run (see `steps.md`).

| site | assumption | consequence of a second domain |
|---|---|---|
| clock-net marking (`~1289`) | forward walk rooted at `sdc.clock_port`'s net | `clk2`'s net is never marked from the root — but consumer-side marking (`register_clock_nets`, `~1300`) still marks it via the FFs' CLK pins, so `clock_net` ends up right by accident |
| `clk2` port exclusion (`~2353`, `~3078`, `~3141`, `~8372`; `PortSel::AllInputs{no_clocks}` in `sdc.rs`) | `sdc.clock_port` is the only clock source | `clk2` is treated as a **data input**: seeded as a launch source with transition/delays of 0 — bogus launches, wrong arrivals |
| `propagate_clock` seeding (`fn` at `:1839`; port seed `~1849-1880`) and `propagate_slew` (`:1671`) | one port seeds `lat`, `lat + period/2`, parity `[0,1]` | domain-2 clock pins stay `UNREACHED` → their `clk -> Q` launches produce nothing → domain-2 endpoints silently absent (or panic — measured by the scratch run) |
| `ideal_clock_parity` (`:1987-2029`) | parity walk from the single port | domain-2 pins keep `u8::MAX`; `edge_parity`/`launch_parity` fall back to `pol as u8` — wrong phase attribution |
| launch/capture pairing (`~4263-4475`) | `capture_extra = if c0 <= g { sdc.period_ns }` — one period, one parity lattice | cross-domain pairing (different periods, different phase grids) has no representation |
| uncertainty (`~3690`, `~4516`) | single `setup_uncertainty_ns`/`hold_uncertainty_ns` on every check | per-clock and inter-clock uncertainty unexpressible |
| output-port required (`~4516`) | `latency + sdc.period_ns - delay - unc` | wrong period/anchor for a `set_output_delay -clock ckb` port |
| latch borrow windows (`~2844`, `~2864-2866`, `~3636`, `~4151`) | `sdc.period_ns` | second-domain latch endpoints borrow against the wrong period |
| MCP multipliers (`~3648-3690`) | `n` is `sdc.period_ns`-based | cross-domain MCP needs the measured per-side period |
| CRPR tags (`~2576-2600`) | tag = `(clock net, edge)` — domain is implicit in the net | works per-net already, but the credit term needs launch/capture domain pairing to mean anything |
| virtual-clock I/O anchoring (`sdc.rs:345-355`, `input/output_clock_latency_ns`) | one `io_clock_*` latency for every `ClockRef::Virtual` | a `d_in` anchored to a second real clock has no anchor slot |

## 3. Our-side probe runs (record in `docs/results.md`)

Run from the repo root after the fixture lands; PDK paths as in
`generate_ref.sh` (`$PDK_ROOT`/`~/.ciel`, `sky130A/libs.ref/sky130_fd_sc_hd`).

| command | expected (record actual) |
|---|---|
| `grus-eda sta --lib <lib> --tlef <tlef> --lef <lef> --def tests/designs/sta_clocks/two_clock.def --sdc tests/designs/sta_clocks/t2_int.sdc` | SDC parse error: `a second design clock is not supported` |
| same with `t2_unc.sdc` | identical bail — the uncertainty commands never execute (parse order) |
| `parse_sdc` on `set_clock_latency 0.5 [get_clocks {core_clk vclk}]` | `set_clock_latency takes one clock` |
| `parse_sdc` on `set_clock_uncertainty -from [get_clocks a] -to [get_clocks b] 0.1` | `unsupported option '-from'` |
| `parse_sdc` on a virtual clock with period != design period | `finish()`: `virtual clock ... has period X, not the design period` |
| `parse_sdc` on `create_clock -waveform {0 0.5} -period 2 ...` | `unsupported option '-waveform'` |
| `sta --sdc` on `t2_int.sdc` variant whose exceptions use `[get_clocks cka]` selectors | command counted in `ignored_commands` (skip summary), no bail — the one "silent-ish" path; verify it is loud under `--strict` |
| scratch build: comment out the `create_clock` bail, run `t2_int.sdc` | second `create_clock` **overwrites** `clock_port`/`period_ns` (the pre-`e921217` behaviour the spec describes); domain A's clock pins unseeded → record what domain A's endpoints do (absent / unreached / panic) and whether `clk2`'s original net leaks into data seeding. Revert after the run; the edit is never committed |

The last row is the only code touch — a scratch experiment to document
the overwrite/partial-eval shape the guard prevents. Everything else
is observable on HEAD.

## 4. Output shape for `docs/results.md`

Two tables, dated `2026-11-19` or the run date:

1. **Break points** — SDC/command → observed behaviour (parse bail
   message + site / counted-ignored / partial-eval result) → the
   table in §3 filled in.
2. **Single-clock sites** — the §1/§2 audit condensed to the sites a
   reader needs to scope the implementation tickets, each with its
   `file:line`.

# Goal

First implementation ticket of the multi-clock sequence
(`docs/multi-clock.md` §3, measured in the `multi-clock-probe`
ticket): replace the single-clock `Sdc` model with a clock
table.

## Work (from the measured spec — follow `docs/multi-clock.md`)

1. `Sdc` keeps a clock table: per-domain name / port / period /
   latencies (network + source) / uncertainties (setup+hold) /
   transition / propagated flag. `ClockRef` becomes a domain
   index (the two-valued `{Design, Virtual}` enum goes away —
   virtual clocks become table rows too).
2. `create_clock` accepts N design clocks (remove the
   `a second design clock is not supported` guard); virtual
   clocks join the same table.
3. `set_clock_uncertainty` / `set_clock_latency` /
   `set_clock_transition` / `set_propagated_clock` target table
   rows; multi-name clock lists allowed
   (`[get_clocks {a b}]` no longer bails). Keep per-domain
   virtual periods instead of the single `finish()` check —
   relax the "virtual period must equal design period" rule to
   the measured OpenSTA behaviour.
4. **Eval stays single-domain**: `sta` on a >1-clock SDC must
   fail loudly (named diagnostic — not silently fold a second
   domain's FFs into domain 0, and not treat clk2 as a data
   input). Pick the narrowest gate: unseeded domains → error.
5. The silent wrong-target bug stays fixed: uncertainty on a
   virtual clock must land on that row, not the design fields
   (probe found this today — the table makes it structural).
6. Parse + model tests. **All existing goldens must be
   bit-identical** — single-clock behaviour unchanged.
7. Update the `multi_clock_bails` tests to the new accepted/
   still-bailing surface; keep tests for whatever still bails
   (`-from`/`-to` inter-clock uncertainty if left for
   `mc-xdomain`, `-waveform`, `-early`/`-late`).
8. Version bump; `docs/multi-clock.md` tick the mc-model box;
   `docs/results.md` entry.

## Files

- `crates/grus-core/src/sta/sdc.rs` (clock table + commands)
- `crates/grus-core/src/sta/analysis.rs` (single-domain gate;
  read table fields where the single fields were used)
- `crates/grus-eda/src/main.rs` (diagnostic if needed)
- `docs/multi-clock.md`
- `docs/results.md`
- `Cargo.toml` / `Cargo.lock`

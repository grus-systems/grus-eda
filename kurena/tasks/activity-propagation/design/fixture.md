# Fixture — `tests/designs/microbench/mba.*` (activity propagation)

Hand-written in-repo (like `mb.vcd`/`mbw.*`), not copied from any upstream
design. Every cell exists to isolate one propagation mechanism; the DEF below
is authoritative for connectivity (exact `+ PLACED` coordinates are free —
space the cells across the die like `mb.def`).

## `mba.def`

Header identical to `mb.def` (VERSION/DIVIDERCHAR/BUSBITCHARS/`DESIGN mba`,
`UNITS ... 1000`, `DIEAREA (0 0)(10000 10000)`, one `unithd` row, li1 tracks).

- `COMPONENTS 13`:
  - `u_and sky130_fd_sc_hd__and2_1` — `n_and = a & b`
  - `u_inv sky130_fd_sc_hd__inv_1` — `n_inv = !b`
  - `u_or  sky130_fd_sc_hd__or2_1` — `n_or = n_and | n_inv`
    (correlated reconvergent fanout: truth `(a&b)|!b = a|!b`, local-marginal
    propagation disagrees with the joint truth — the P2 discriminator)
  - `u_xor sky130_fd_sc_hd__xor2_1` — `y = a ^ b` (every input edge toggles
    the output under either input state — the density-rule discriminator)
  - `u_df  sky130_fd_sc_hd__dfxtp_1` — `D=a`, `CLK=clk`, `Q=q` (plain
    register: "does Q follow D's rate, clk rate, or a default?")
  - `u_f0  sky130_fd_sc_hd__dfxtp_1` + `u_i0 sky130_fd_sc_hd__inv_1` —
    `u_f0/D = n_q0n`, `u_i0/A = n_q0`, `u_i0/Y = n_q0n` (toggling flop;
    exact rate = 1 toggle/period = clk-rate/2)
  - `u_f1  sky130_fd_sc_hd__dfxtp_1` + `u_x1 sky130_fd_sc_hd__xor2_1` —
    `u_f1/D = n_c1d`, `u_x1/A = n_q0`, `u_x1/B = n_q1`, `u_x1/X = n_c1d`
    (2-bit counter; exact Q1 rate = clk-rate/4; tests whether the seq model
    captures feedback-derived divide rates)
  - `u_c sky130_fd_sc_hd__conb_1` — `HI -> n_hi`, `LO -> n_lo`
    (`function : "1"`/`"0"` output pins — the constant-net case)
  - `u_bh`, `u_bl sky130_fd_sc_hd__buf_1` — `n_hi -> n_bh`, `n_lo -> n_bl`
    (propagation of a constant through a real gate)
  - `u_flt sky130_fd_sc_hd__buf_1` — `A` on `n_float` (undriven sink net),
    `X` on `n_flt` (driven, no load) — the undriven/x-input case
- `PINS 10`: inputs `clk`, `a`, `b`; outputs `y`, `yc`, `qd`, `q0`, `q1`,
  `yh`, `yl` — same `+ PORT`/layer/placed shape as `mb.def` pins.
- `NETS 18`:
  - `clk (PIN clk)(u_df CLK)(u_f0 CLK)(u_f1 CLK)`
  - `a   (PIN a)(u_and A)(u_xor A)(u_df D)`
  - `b   (PIN b)(u_and B)(u_xor B)(u_inv A)`
  - `n_and (u_and X)(u_or A)`
  - `n_inv (u_inv Y)(u_or B)`
  - `n_or  (u_or X)(PIN yc)`
  - `y     (u_xor X)(PIN y)`
  - `q     (u_df Q)(PIN qd)`
  - `n_q0  (u_f0 Q)(PIN q0)(u_i0 A)(u_x1 A)`
  - `n_q0n (u_i0 Y)(u_f0 D)`
  - `n_q1  (u_f1 Q)(PIN q1)(u_x1 B)`
  - `n_c1d (u_x1 X)(u_f1 D)`
  - `n_hi  (u_c HI)(u_bh A)`
  - `n_lo  (u_c LO)(u_bl A)`
  - `n_bh  (u_bh X)(PIN yh)`
  - `n_bl  (u_bl X)(PIN yl)`
  - `n_float (u_flt A)` — no driver on purpose; record any OpenSTA warning
    verbatim, it is part of the P5 evidence
  - `n_flt   (u_flt X)` — driven, no load

## `mba.sdc`

Same shape as `mb.sdc`:

```
create_clock -name clk -period 20.0 [get_ports clk]
set_input_delay 1.0 -clock clk [all_inputs -no_clocks]
set_output_delay 1.0 -clock clk [all_outputs]
set_input_transition 0.15 [all_inputs]
set_load 0.05 [all_outputs]
```

## VCD stimulus set (authored; probe.md says which session reads which)

Shared skeleton mirroring `mb.vcd`: `$timescale 1ns`, scopes `tb`/`dut`,
dut-scope `$var wire 1` per *net* name (mb evidence: a net-level `$var`
annotates every pin on the net — no per-cell pin scopes needed), `$dumpvars`
initials for **every** declared signal, `#2000` end. Window `[10, 2000]`
(clk toggles every 10 ns from `#10` → 100 rises + 100 falls).

- `mba_ports.vcd` — **input ports only**: `clk`, `a`, `b`.
  `a`: rises 200/1000/1600, falls 600/1400/1900 → 3R+3F, T1=1100 → P≈0.55.
  `b`: rises 200/1000/1600, falls 900/1550/1950 → 3R+3F, T1=1600 → P=0.80.
  Asymmetric, non-degenerate duties on both inputs.
- `mba_ports_a2x.vcd` — same but `a` carries 6R+6F (double the edge density,
  same duty) — the sequential-rate sweep: does `q`/`n_c1d`-path activity
  follow `a`'s density, a fixed default, or clk?
- `mba_full.vcd` — **all 18 nets** with function-consistent waveforms
  (zero-delay evaluation of the a/b stimulus through the cone; `q` = `a`
  delayed one posedge; `n_q0` toggles each posedge — 100 toggles;
  `n_q1` every other — 50; `n_hi`≡1, `n_lo`≡0, `n_bh`/`n_bl` copies;
  `n_float` and `n_flt` carry `x` initials and stay `x`). The exact-activity
  control: the propagated-vs-exact delta per net is itself recorded.
- `mba_partial.vcd` — `clk`, `a`, `b`, plus `n_and` annotated with a
  **deliberately inconsistent** duty (1R+1F, high [300,1900] → P≈0.80 vs the
  a&b truth ≈0.44): if downstream `n_or` moves with the annotation, OpenSTA
  honors internal annotation over re-derivation; if not, it re-derives.
  Contingency variant if the first is ambiguous: `mba_partial2.vcd` — only
  internal nets (`n_and`, `n_or`) annotated, ports unannotated → do
  unannotated *inputs* get defaults while internals hold annotation?
- `mba_xz.vcd` — `clk`, `b`, and `a` with an x-run (`0->x` at 300, `x->1`
  at 500 — two half-weighted edges) and otherwise the `mba_ports` waveform.

## `generate_activity_ref.sh` (committed; emits the golden)

Self-invoking container script, same skeleton as `generate_when_ref.sh`
(repo `/work`, PDK `/pdk`, `openroad -no_init -no_splash -exit`, Colima
fallback via `mise exec ... docker`). Sections:

1. `mba_reads.tcl` — tlef + lef + `tt_025C_1v80` lib + `mba.def` + `mba.sdc`.
2. P0 discovery session (`probe.md` P0 — `info commands`, `catch`ed
   `set_power_activity` signature probes, per-pin accessor hunt) → console,
   recorded in `results.md`, not committed.
3. Sessions `s_none`, `s_global`, `s_input`, `s_ports`, `s_ports_a2x`,
   `s_full`, `s_partial`, `s_xz` — each is the reads preamble + the
   annotation step + `report_power -digits 6` + `report_activity_annotation`.
   `s_none`'s stdout is `grep -v`-filtered (`^\[INFO`, `^\[WARNING`, banner,
   blanks — the mbw filter) and committed as `mba.none.openroad.txt`,
   labelled filtered.
4. A python step (the `ROW` regex from `generate_when_ref.sh` §4, plus an
   annotation-split regex) parses every session's table + annotation counts
   into `mba.activity.reference.json` = `{ "<session>": {"annotation":
   {"vcd": N, ...}, "groups": {...}} }` — parsed, never typed.

## Test (`power.rs` `#[cfg(test)]`) — implement branch only

`test_activity_propagation_mba_matches_opensta`: `require()`s PDK + fixture,
runs `estimate_power` with the propagated-activity fill per api.md, compares
every group of the `s_ports` block (and `s_none` if the no-annotation CLI
mode lands) at the STRICT bounds measured in the results entry
(internal 2% / switching 5% / total 1% / leakage 25% + 1.5e-12 W floor as
starting bounds, tightened to the measured honest bound). Fail-proof:
disable the fill pass → test red; corrupt one golden block → red; restore.

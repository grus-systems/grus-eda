# mc-model — Implementation steps

One commit. Order chosen so each step keeps the tree compiling; the
test updates land with the code they pin. Run `cargo test -p grus-core
sdc` after steps 2–4, `cargo test --workspace` + clippy at the end.

## 1. `Clock`/`ClockId`/`Sdc` skeleton (sdc.rs)

- Add `pub struct Clock` (api.md §1) and `struct ClockId(usize)`;
  delete `enum ClockRef`.
- Replace `Sdc`'s single-clock fields with `pub clocks: Vec<Clock>`
  (keep `#[derive(Default)]`).
- Add the `impl Sdc` accessors per api.md §1: `design_clock`,
  `design_clock_count`, `clock`, `is_clock_port`, the field-forwarding
  methods (`clock_name`, `clock_port`, `period_ns`, …), `latency_ns`.
- `PortSel::matches` takes `sdc: &Sdc` instead of
  `clock_port: Option<&str>`; update `delay_at`, `last_value`,
  `output_load_pf`, `driving_cell` call sites (and the two analysis.rs
  selector sites from §4 of api.md in step 5).
- `DelayRule.clock: Option<ClockId>`; `delay_at` returns
  `Option<(f32,f32,Option<ClockId>)>`.
- Compile will fail at the command handlers — that is step 2–4.

## 2. `create_clock`, `resolve_clock`, `all_clock_names`, `finish`

- `create_clock` pushes rows (api.md §3); delete the
  `virtual_clocks` field on `Interp` and the
  `a second design clock is not supported` bail.
- `resolve_clock -> Result<ClockId>`: `c.name == name` only (the port
  fallback stays in `propagated_clock`).
- `all_clock_names` iterates `sdc.clocks` in file order.
- `finish`: ≥ 1 port-bearing row (`SDC defines no clock period`), every
  row `period_ns > 0`; delete the virtual-equals-design check.

## 3. Clock attribute commands

- `set_clock_latency`: collect `ClockId`s (no target → first
  port-bearing row, bail if none yet); write `latency_ns` /
  `source_latency_ns` per `-source`.
- `set_clock_uncertainty`: resolve each named clock and write the
  matched rows (both fields unless `-setup`/`-hold`).
- `set_clock_transition`: write `transition_ns` on each resolved row;
  remove the virtual-clock `ignored_options` counting.
- `propagated_clock`: per-row write with the port-name fallback and the
  virtual `set`-bail / `unset`-no-op preserved (api.md §3).
- `delay_rule` `-clock`: `Option<ClockId>`.

## 4. Single-domain accessors inside sdc.rs

- `latency_ns`, `design_clock_latency_ns`, `input_delay_ns`,
  `input_clock_latency_ns`, `output_delay_ns`,
  `output_clock_latency_ns`, `input_transition_ns`, `output_load_pf`,
  `net_loads`, `driving_cell`, `derate` — same signatures/semantics,
  internally reading the table per api.md §1.

## 5. analysis.rs mechanical reads + the gate

- Swap every `sdc.<field>` read to the accessor calls listed in
  api.md §4's table (all within `run_sta_detail`'s reach;
  `sdc.clock_port.as_deref() == Some(x)` → `sdc.is_clock_port(x)`;
  `ps.matches(…, clock)` → `ps.matches(…, self.sdc())`).
- Add the `design_clock_count() > 1` bail at the top of
  `run_sta_detail`.
- Test-only sites (~7218, ~8372) get the same accessor swaps.

## 6. Tests (see tests.md for the full matrix)

- Mechanical updates: field → accessor across the existing sdc.rs
  tests (`sdc.period_ns` → `sdc.period_ns()`, `sdc.clock_latency_ns` →
  `sdc.design_clock().unwrap().latency_ns`, ibex's
  `io_clock_latency_ns` → `sdc.clock("vclk_core_clock").unwrap().latency_ns`, …).
- Replace `rejects_a_second_design_clock` and
  `rejects_a_virtual_clock_with_another_period` with accept-tests that
  pin the rows.
- Rewrite `uncertainty_on_virtual_clock_writes_design_fields` as the
  row-targeting regression.
- Update `multi_clock_bails` (drop the now-accepted multi-name latency
  row; keep the still-bailing rows) and add the accepted-surface tests.
- New analysis test: `run_sta` on `two_clock.def` + `t2_int.sdc`
  errors.

## 7. Verify

- `cargo test --workspace` — all goldens byte-identical (they run
  unchanged; a diff in `tests/designs/**/*.reference.json` means a
  regression, not an update).
- `cargo clippy --workspace --all-targets -- -D warnings`.
- Prove the gate fails the right way (AGENTS.md §7 rule 7): comment
  out the bail → `run_sta` on the two-clock SDC produces numbers or a
  panic, the new test goes red; restore.
- CLI: `cargo run --release -- sta --lib <sky130 lib> --tlef <tlef>
  --lef <lef> --def tests/designs/sta_clocks/two_clock.def --sdc
  tests/designs/sta_clocks/t2_int.sdc` → exit 1, record the stderr
  verbatim.

## 8. Docs + version

- `Cargo.toml` `[workspace.package] version`: `0.1.98` → `0.1.99`
  (feature patch bump; `Cargo.lock` regenerates).
- `docs/multi-clock.md`: status line → mc-model landed; §3 item 1 ticked
  done; §2 break-matrix rows updated (2nd `create_clock` accepted,
  multi-name latency accepted, virtual uncertainty lands on its row,
  virtual periods free-form, transition on virtual row stored; the
  single-clock-SDC-on-multi-clock-DEF fold remains → mc-eval).
- `docs/results.md`: new dated entry — commands run (CLI bail
  verbatim), the accepted/still-bailing parse surface, golden-identity
  statement, version bump. Follow the existing entry shape.

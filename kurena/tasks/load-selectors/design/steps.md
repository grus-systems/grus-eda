# load-selectors — Implementation and verification steps

## Step 0 — Record the failure, then probe

1. Add the `ld_*` SDC scenario files from `fixture.md` (fixture files may
   precede implementation).
2. Run the current binary/test path once against a net/pin load SDC and
   record the whole-file parse failure. Also record the
   `set_fanout_load` unsupported-command failure. Put the exact command
   and output in `docs/results.md`.
3. Start Colima if needed per `AGENTS.md`, then run the `probe.md`
   questions inside `ghcr.io/librelane/librelane:3.0.16`. Use a separate
   OpenSTA process per positive SDC case; keep rejected cases in
   `catch`.
4. Write the measured rules into the same dated `docs/results.md` entry:
   net add/replace/placement, pin domain and add/replace, missing-name
   behaviour, cone-selector acceptance, qualifier surface, and
   `set_fanout_load` outcome. Do not start the timing change until this
   exists.

## Step 1 — Parser and SDC model

1. Add `Value::Nets` plus `get_nets`/`get_net` dispatch.
2. Replace `loads: Vec<(PortSel, f32)>` with `Vec<LoadRule>` and keep
   `output_load_pf` filtering port rules with last-wins behaviour.
3. Add `LoadSel`/`LoadRule` and `load_rule`; dispatch `set_load` through
   it. Unsupported selectors and unmodelled qualifiers count
   `ignored_commands["set_load"]`; malformed commands still error.
4. Add `set_fanout_load` to the command table and count it per the
   measured effect.
5. Extend `remove_from_collection` for net collections and keep
   `obj_sel(Value::Nets) == None` for exceptions.
6. Add the parser unit tests listed in `api.md`.

## Step 2 — Resolution and timing application

1. Build `ResolvedLoads` at the start of `prepare_sta`, before pi models
   and wire delays. Resolve exact net names and `instance/pin` names
   through `DesignDb`; count misses in
   `SkipReport::unresolved_load_selectors`.
2. Apply the measured assignment/order semantics from Step 0. Keep the
   same resolved table visible to `sink_cap`, `net_load`, `build_pi`, and
   `wire_delays`.
3. Update `SkipReport::{nonzero,is_clean}` and `merge_skip_reports`.
   Preserve resolution-time diagnostics through both `eval_sta` and
   `run_sta_partitioned`.
4. If the measured net semantics cannot be represented under
   parasitics, count it there and document the restriction rather than
   using a different approximation in RC mode.
5. Add/extend `analysis.rs` diagnostics tests from `fixture.md` §4.

## Step 3 — Golden generation

1. Append the `ld_*` names to `SCENARIOS` in
   `tests/designs/sta_exceptions/generate_ref.sh`.
2. Run the committed script as written. It self-reexecutes inside the
   pinned LibreLane container and emits
   `tests/designs/sta_exceptions/exceptions.reference.json`.
3. Inspect the diff with a JSON-aware check:

   - all pre-existing scenario keys unchanged;
   - `ld_net` and at least one pin scenario move an endpoint arrival vs
     `base` outside `max(5 ps, 1%)`;
   - counted-selector scenarios have the measured reference outcome
     (baseline timing or `error`);
   - `ld_fo` records the measured `set_fanout_load` outcome.

4. If OpenSTA rejects an expected applied scenario, stop and re-probe;
   do not hand-edit the JSON or infer values.

## Step 4 — Verification

1. Focused tests first, then the pipeline commands:

   ```sh
   cargo test -p grus-core sta_exceptions
   cargo test -p grus-core sdc
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   ```

2. Deliberately break the applied load path once and confirm the
   `ld_net`/`ld_pin_sink` comparison fails; restore and rerun the focused
   test (AGENTS.md lesson 7).
3. Check `--strict` behaviour with a diagnostics-focused SDC: unresolved
   or unsupported load selectors must fail strict unless the probe proved
   the skipped annotation cannot affect the reported timing surface.
4. Run `git diff` on `exceptions.reference.json`; every value must come
   from the container script.

## Step 5 — Documentation and version

1. Finish the `docs/results.md` entry: current failure, probe matrix and
   measured rules, applied/counted scenario list, exact generator command,
   verification commands, and fail-proof result.
2. Update `docs/roadmap.md` N2: remove only the completed
   `get_pins`/`get_nets`/`all_fanouts`/`set_fanout_load` item; keep
   unrelated path-delay follow-ups.
3. Update `docs/e3-sta.md` SDC subset text and
   `docs/timing-exceptions.md` if the skip-summary surface or strictness
   policy changed.
4. Bump `[workspace.package] version` by patch in the same commit as the
   functional change; update `Cargo.lock` if needed.

## Exit criteria

- `set_load` accepts `get_ports`, `get_pins`, `get_nets`, and unsupported
  cone/typed selectors without aborting the SDC.
- At least one applied net/pin scenario matches OpenSTA at STRICT
  tolerance with a visibly moved arrival.
- Counted cases are visible in `skipped` JSON/text diagnostics and obey
  the measured `is_clean` policy.
- `set_fanout_load` parses and its measured timing outcome is recorded.
- `cargo test --workspace` and clippy pass.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_exceptions/`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/timing-exceptions.md`
- `docs/e3-sta.md`
- `Cargo.toml`
- `Cargo.lock`

# Goal

Close `docs/roadmap.md` N2's remaining item: `set_load` and
`set_fanout_load` currently only accept a port selector. A
`[get_nets]`/`[get_pins]`/`[all_fanouts]` object list degrades to
`Value::Opaque` inside `value_and_ports` and fails the whole SDC;
`set_fanout_load` is not in the command table at all and bails
`unsupported command`. A stock `write_sdc` file that emits net/pin
load annotations therefore cannot parse.

## Work

1. **Measure the current failure and the OpenSTA semantics first.**
   Confirm today's behaviour (whole-SDC parse error vs
   counted-skip) and record it. Then measure against OpenSTA in
   the LibreLane container, on a fixture net list:
   - `set_load <v> [get_nets <nets>]` — cap lands on the named
     nets (verify via a driver whose delay/slew moves).
   - `set_load <v> [get_pins <pins>]` — which net(s) does OpenSTA
     charge (the pin's attached net? sinks only? driver only)?
   - `set_load <v> [all_fanouts ...]` / `[all_fanins ...]` — what
     cone does it cover, which options does OpenSTA accept.
   - `set_fanout_load <n> [objects]` — same object-set questions,
     and whether the fanout count enters the delay model at all
     in OpenSTA (it may only affect `report_checks -fanout` style
     output).
   Record every measured rule in `docs/results.md`.

2. **Implement.** Where the wire/load model can honour the
   selection, resolve and apply it (`ObjSel`-style resolution,
   counted diagnostics for unresolvable specs). Where it cannot
   (e.g. `all_fanouts` cone queries the model does not track),
   accept the selector and count a named skip in
   `ignored_commands`/the skip summary rather than erroring —
   never silently drop. `set_fanout_load` joins the command table
   at least as a counted command even if it only counts.

3. **Fixture + golden.** Extend `tests/designs/sta_exceptions/`
   (or a small `sta_edge`-style dir if cleaner) with scenarios
   whose `set_load` on nets/pins visibly moves an arrival, plus
   the counted-skip cases. Regenerate the golden with the
   committed script inside the container; STRICT tolerance.

4. **Docs**: `docs/results.md` dated entry (failure mode before,
   measured rules, applied vs counted outcomes);
   `docs/roadmap.md` N2 remaining list updated;
   `docs/timing-exceptions.md` if the skip surface changes.
   Version bump (patch) in the same commit.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_exceptions/*.sdc`
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/timing-exceptions.md`
- `Cargo.toml`
- `Cargo.lock`

# Goal

Fourth ticket of the multi-clock sequence (`docs/multi-clock.md`
§3): the remaining unsupported SDC surface — clock-object
selectors in exceptions, `set_clock_groups`, and
`create_clock -waveform`.

## Work (measured in `docs/multi-clock.md` §1.6 and the probe)

1. **Clock selectors in exceptions**: `set_false_path` /
   `set_max_delay` / `set_min_delay` / `set_multicycle_path`
   with `-from`/`-to`/`through` `[get_clocks <name>]` currently
   hit `obj_sel → None` and land in `ignored_commands`.
   Implement the measured semantic: a clock selector matches
   endpoints (or launch segments) whose domain is that clock —
   `-to [get_clocks ckb]` covers `ckb`-capture endpoints,
   `-from [get_clocks cka]` covers `cka`-launch segments.
   Re-probe OpenSTA in the container for the exact matching
   rule before coding (which object the selector anchors —
   launch domain vs capture domain — for each of `-from`,
   `-to`, and on a delay check vs a false path), and record it.
2. **`set_clock_groups -asynchronous -group A -group B`**
   (`t2_grp` measured): all cross-domain checks disappear —
   the endpoint table shrank 8→5 including I/O-anchored cross
   checks — while same-domain and same-domain-anchored I/O are
   untouched. Parse (multi `-group`, `-asynchronous` at
   minimum; `-logically_exclusive`/`-physically_exclusive` may
   share the eval path or bail with a named error — probe and
   follow the measured rule) and apply as a group-pair
   exclusion table consulted at check time.
3. **`create_clock -waveform {rise fall}`**: non-50% duty on
   design clocks — edge lattice becomes the waveform's two
   instants repeating per period, so pairing/parity/borrow use
   the actual edges. Probe duty-cycle values (e.g.
   `{0 0.5}` on 2.0, a non-trivial `{0.3 1.7}`) and record.
4. Golden coverage on `sta_clocks`: `t2_grp` (8→5 endpoint
   shrink, verbatim), a selector scenario, and a `-waveform`
   scenario — strict tolerances, committed artifacts.
5. Record in `docs/results.md`; tick `mc-selectors` in
   `docs/multi-clock.md` §3; version bump.

## Files

- `crates/grus-core/src/sta/sdc.rs` (selector resolution,
  clock-groups parse, `-waveform` flag + lattice fields)
- `crates/grus-core/src/sta/analysis.rs` (group exclusion,
  waveform edges, selector coverage)
- `tests/designs/sta_clocks/` (new sdc + regen)
- `docs/multi-clock.md`, `docs/results.md`
- `Cargo.toml` / `Cargo.lock`

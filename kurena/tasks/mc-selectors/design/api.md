# mc-selectors — API and data model

Line numbers reference `analysis.rs`/`sdc.rs` at the worktree HEAD.

## 1. `sdc.rs`: clock selectors in exceptions

### `ObjSel::Clocks(Vec<usize>)`

New variant carrying resolved `Sdc::clocks` row indices (not names —
`resolve_clock` has already validated them at parse; `ClockId` is
private so plain `usize` rows, the convention
`InterClockUnc::from`/`to` already uses).

`obj_sel` currently takes `&Value` with no parser access, so it cannot
resolve names. Two options, pick the smaller diff:

- (a) `obj_sel(&Value, &Parser)` — resolve `Value::Clocks(names)` to
  rows inside; or
- (b) keep `obj_sel` pure on the value kinds it knows and handle
  `Value::Clocks` at the two call sites (`timing_exception`,
  `path_delay`) with a `clock_sel(names)` helper.

Either way: `-from`/`-to [get_clocks <name>...]` and `[all_clocks]`
produce `Some(ObjSel::Clocks(rows))` and the command is stored — it no
longer falls through to `ignored_commands`. Unknown names bail
`unknown clock` (the `resolve_clock` path; match it to what the probe
measures for an unknown clock inside an exception — `probe.md` §1).
`Value::Nets`/`Value::Opaque` still return `None` → ignored.

`-through` stays in `*_UNSUPPORTED_FLAGS` — out of scope.

### Parse-time semantics to record

- `[get_clocks {cka ckb}]` (multi-name) → `rows` is the union.
- `[all_clocks]` → every row.
- Bare `-from cka` stays `ObjSel::Names` unless the probe measures
  OpenSTA resolving bare clock names in exceptions; if it does,
  `Names` resolution gains a clock-name arm *in `ExceptionCtx`*
  (names resolve at STA time there), recorded in
  `docs/multi-clock.md`.

## 2. `analysis.rs`: selector resolution

`ExceptionCtx` gains two maps built once in `run_sta_partitioned`
(where `graph`, `prepared`, `port_seeds`, `output_ports`, `ep_kinds`
are all already in scope — around the `clk_launch` build, ~L3610):

```text
key_dom: HashMap<u32, usize>   // launch key -> clock row
ep_cap:  HashMap<u32, usize>   // endpoint pin -> capture row
```

- `key_dom`: for `graph.launches`/`latch_opens`, `key_dom[l.q] =
  clock_domain[l.clk]`; for `port_seeds`, `key_dom[s.pin] =
  sdc.input_clock_row(port_name)` — the **raw** resolved `-clock` row
  (a virtual row stays virtual), *not* `launch_domain`-folded: the
  selector asks "clocked by X", not "on X's grid".
- `ep_cap`: register checks and gating enables → `clock_domain[clk
  pin]`; output ports → `sdc.output_clock_row(port)`.

`resolve_from(ObjSel::Clocks(rows))` → `{k ∈ launch_keys : key_dom[k] ∈
rows}`. `resolve_delay_from` shares it — a clock `-from` is an ordinary
launch selector (covers the clocked launch), **not** a pin startpoint;
`from_pins` stays empty. `resolve_to(ObjSel::Clocks(rows))` → `{p ∈
endpoint_pins : ep_cap[p] ∈ rows}`. `u8::MAX`-domain entries simply
match nothing.

Coverage measured on `t2_sel*` scenarios:
`-from [get_clocks cka]` = `{u_fa0/Q, u_fa1/Q, u_fy/Q, d_in}`;
`-to [get_clocks ckb]` = `{u_fx/D, u_fb0/D, u_fb1/D, yb}` (pend probe
confirmation of the port side — `probe.md` §1).

## 3. `set_clock_groups`: parse + eval

### `sdc.rs`

```rust
pub enum ClockGroupKind { Asynchronous, LogicallyExclusive, PhysicallyExclusive }
pub struct ClockGroup {
    pub kind: ClockGroupKind,
    /// Each `-group`'s resolved clock rows, in command order.
    pub groups: Vec<Vec<usize>>,
    /// Index into the merged rule list where this command sat in the
    /// file: `sdc.exceptions.len() + sdc.path_delays.len()` at the
    /// point the command runs. Precedence vs explicit rules.
    pub order: usize,
}
pub clock_groups: Vec<ClockGroup>,   // on `Sdc`, in file order
```

Parser `set_clock_groups`: walk args; `-asynchronous` /
`-logically_exclusive` / `-physically_exclusive` are mutually
exclusive mode flags (missing mode → named bail or the measured
default — probe); each `-group` consumes the next `Value::Clocks` or
`Value::Str` (whitespace list) resolved via `resolve_clock`. `-name`
accepts a string (stored nowhere — OpenSTA uses it for reporting only;
count nothing). `-allow_paths` and any other dash flag: probe-driven —
either `bail!("unsupported option")` or accepted-and-counted per what
OpenSTA does (`probe.md` §2). Fewer than two `-group`s → the command
excludes nothing; store it anyway (it parses in OpenSTA — verify) or
bail, per probe.

### `analysis.rs` — desugar to false paths

In `run_sta_partitioned`, after `rules` is built from
`exceptions` + `path_delays`: for each `ClockGroup`, for each unordered
pair of its `groups` lists `(A, B)`, synthesize two
`ResolvedRule::Exception` entries:

```text
{ kind: FalsePath, from: keys_by_dom(A), to: eps_by_cap(B) }   // A→B cut
{ kind: FalsePath, from: keys_by_dom(B), to: eps_by_cap(A) }   // B→A cut
```

inserted at index `group.order` in `rules` (all of a command's pairs
together, pair order irrelevant). `keys_by_dom(rows)` = `key_dom` ∪
rows; `eps_by_cap(rows)` = `ep_cap` ∪ rows. This is exactly what the
exception machinery does with a hand-written
`set_false_path -from [get_clocks A] -to [get_clocks B]`, so §1–§2 land
first — groups reuse the selector coverage.

Everything downstream is unmodified: signature partitioning puts the
excluded-domain keys in their own classes, `drop` kills their
candidates at covered endpoints, `survived = false` → `fp_endpoints`
(the same declared-constraint accounting `set_false_path` uses —
`is_clean`-exempt, shown by `nonzero()`). `run_sta_detail`'s gate
becomes:

```rust
if !sdc.exceptions.is_empty() || !sdc.path_delays.is_empty() || !sdc.clock_groups.is_empty() {
    return run_sta_partitioned(&prepared, emit_warnings);
}
```

`xdomain_defer`/`collect_endpoints` need **no** group awareness:
excluded pairs never leave the compose pass; they evaluate in their
class run and are dropped like any false-path-covered candidate. For
`t2_grp` this reproduces the measured 8→5 shrink including the
`d_in`-anchored input check (`d_in ∈ keys_by_dom(cka)`, `u_fb0/D ∈
eps_by_cap(ckb)`).

Precedence vs `set_max_delay`/explicit `set_false_path` is the `order`
insertion — measured by `probe.md` §2 before finalizing whether the
synthesized rules insert at `order`, append, or prepend.

## 4. `create_clock -waveform`: parse + edge lattice

### `sdc.rs`

```rust
pub waveform_ns: Option<[f32; 2]>,   // on `Clock`; [rise instant, fall instant]
```

`-waveform` joins `CLOCK_VALUE_FLAGS`; `flag_arg` reads its
`Value::Str` (`{0.3 1.7}` arrives whitespace-joined). Parse with
`split_whitespace` + `parse_f32`; require **exactly two** instants
(probe may widen — `>2`/odd counts get a named bail
`create_clock -waveform takes two edge instants` until measured
otherwise). Same field on design and virtual rows. `finish` unchanged
(period > 0 only).

### `analysis.rs` — the offset accessor

```rust
impl Clock {
    /// Time within the period of the clock edge with index parity `p`
    /// (0 = rise, 1 = fall): the waveform instant, or `p · P/2` for the
    /// default 50% duty cycle.
    fn edge_offset(&self, p: usize) -> f32;
}
```

Every site currently assuming `p · P/2` moves to `edge_offset` (a `&Clock`
is in scope at all of them):

- `propagate_clock` port seed (~L1910): `lat + half_period` → `lat +
  clock.edge_offset(1)`; the RISE merge stays `lat` only when
  `edge_offset(0) == 0` — generalize to `lat + edge_offset(0)` for
  symmetry (identical when offset is 0).
- Ideal seeding (~L2595): `ideal_latency + edge_par * half_period` →
  `ideal_latency + row.edge_offset(edge_par)`.
- `setup_edge_pair`/`hold_edge_pair` (~L4784, ~L4816): signatures gain
  the two offset pairs — `launch0 = off_l[g]`, `phase_c = off_c[c0]`
  instead of `g·PL/2` / `c0·PC/2`. `grid_lcm` and the window logic are
  period-only — unchanged.
- Same-domain phase tests in `collect_endpoints`/`gating` (~L5162,
  ~L5265, ~L5413, ~L5455): `c0 <= g` → `cap_clk.edge_offset(c0) <=
  lclk_or_cap.edge_offset(g)`; `c0 > g` / `pe_s <= g` / `g < pe_s`
  likewise. On the uniform grid every comparison is equivalent to the
  parity one (both instants ordered `0 < P/2`), so existing SDCs are
  byte-identical.
- `xdomain_defer`'s `pair_ok` passes the rows' offsets through.

### Sites deliberately unchanged

- Latch `open`/`close` comparisons (`close <= open → += P`), borrow
  windows, `edge0_pol` decode, MCP `(ns−1)·P`/`−nh·Pc` shifts, pulse
  widths (`width = arr[close] − arr[open]`, wrap by `period`), delay-
  check edge-0 anchors (`clk_s.arrival_*`), `capture_extra` — all read
  seeded/propagated arrivals that already carry the instants, or
  multiply whole periods.
- Port seeds: `per_pol` arrivals anchor at `latency + in_delay`.
  Whether the waveform rise instant adds (`+ edge_offset(0)`) is a
  `probe.md` §3 measurement; if it does, add it where `per_pol` is
  built (~L2462).

## 5. Model-limit documentation

- Inter-clock `-from`/`-to` uncertainty and group/selector coverage
  all match on *clock rows*; `launch_domain` folding affects only
  dom-mask pairing, not row-keyed coverage — state the distinction in
  `docs/multi-clock.md`.
- `-waveform` support covers exactly the two-instant alternating
  rise/fall form; anything else is a named parse bail.

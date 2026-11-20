# mc-selectors — API and code deltas (revision)

All changes live in `crates/grus-core/src/sta/analysis.rs` inside
`ExceptionCtx` / `run_sta_partitioned`; `sdc.rs` is untouched (its
lookups are already name-only). Line numbers reference `c69cd99`.

## 1. `clock_row`: name-only + port-namespace resolution

`ExceptionCtx::clock_row` (~L3243) currently resolves a bare name to a
clock row when it matches `c.name` **or** `c.port`. The port arm is
unmeasured and wrong under every plausible reading of OpenSTA: it lets
`-from clk1` cut the `cka` launch domain.

```rust
fn clock_row(&self, name: &str) -> Option<usize> {
    self.sdc().clocks.iter().position(|c| c.name == name)
}
```

— the same lookup `Parser::resolve_clock`/`clock_rows` already apply
(`sdc.rs` ~L2056 records the measured rule). Update its doc comment and
the `key_dom`/`ep_cap` comments that cite "bare clock names" to say
*clock `-name`s*.

### Bare names resolve against the port namespace

`ObjSel::Names` resolution order (unchanged where measured):

1. `inst/pin` (contains `/`) → `pin_launches`/`pin_endpoints`/
   `delay_pin_spec`.
2. Port name → port object.
3. Cell name → `keys_by_cell`/`eps_by_cell`.
4. Clock `-name` → `keys_on_clocks`/`eps_on_clocks`.
5. Nothing → per-name `unresolved += 1`.

Step 2 currently checks `port_seeds` on `-from` and `output_ports` on
`-to` — seeded/output ports only. `t2_selport`/`t2_selpto` measure
whether an *unseeded input* port (`clk1`/`clk2`) resolves in OpenSTA
(`probe.md` §1 decision table). Under the expected silent-dead-rule
reading, step 2 gains a resolution arm: the name matches **any**
`design.io_pins` entry → resolved (`hit = true`), contributing keys
only through the existing seeded/output coverage checks. Direction does
not gate resolution — an input port is a resolved-but-dead object on
`-to` the same as on `-from`, matching OpenSTA's object lookup; the
probe's `t2_selpto` pins it. Iterate `design.io_pins` with
`cx.port_name`/`pin_dirs` at ctx build into a `port_by_name` map or
scan inline (a handful of ports — inline scan is fine).

If the probe instead shows a warning or a clock resolution, follow the
`probe.md` §1 table — do not code to the expected row.

### `unresolved` counts object-resolution failures, not empty coverage

OpenSTA's `no valid objects specified for -from` fires when a selector
resolves **zero objects**; a resolved object that covers nothing
(`clk1`, `[get_cells u_buf]`, `[get_pins u_fa0/D]`) is a silent dead
rule. Today the counting conflates the two: `resolve_sides` (~L3710)
and the delay-rule equivalents (~L3737, ~L3741) bump `unresolved` on an
empty *coverage* set.

Change each `resolve_*` to also return whether the selector resolved
any object, and bump only on `resolved_any == false`:

- `ObjSel::Clocks(rows)` → `resolved_any = !rows.is_empty()`
  (`t2_selunk`'s all-miss collection — keeps its +1).
- `ObjSel::Ports(sel)` → `resolved_any = sel` matches ≥1 `io_pins`
  entry (its actual direction). `[get_ports nope]` keeps the +1;
  `[get_ports clk1]` resolves dead.
- `ObjSel::Cells(sel)` → `resolved_any = ≥1` name in `cell_by_name`
  (per-name misses still counted inside `cell_sel_ids`).
- `ObjSel::Pins(specs)` → `resolved_any = ≥1` spec in `pin_by_path`
  (per-spec misses still counted inside `pin_*`).
- `ObjSel::Names(names)` → `resolved_any = ≥1` name resolved in steps
  1–4 (per-name misses still counted inside).

`resolve_from`/`resolve_to` return `(HashSet<u32>, bool)`;
`resolve_delay_from` returns `(HashSet<u32>, HashSet<u32>, bool)`.
`resolve_sides` and the delay-rule resolution bump `unresolved` on
`!resolved_any` instead of `is_empty()`.

Arithmetic check against the pinned count: `exc_diag`'s
`unresolved_exception_selectors == 4` is preserved — each all-miss
`get_cells` side counts `per-name miss (+1)` + `zero objects (+1)` = 2,
as before. A *resolved-but-dead* selector loses its old +1 — that is
the measured correction, and no committed golden compares the count.

## 2. `dead_ends`: gate the fanout traversal on delay rules

The traversal (~L4364–4393) builds `dead_ends: HashMap<u32, Vec<u32>>`
for every launch key unconditionally, but the leaf pass only fires on
`ResolvedRule::Delay` entries in a class signature. Restructure:

- Extract the DFS body (net-sink + `graph.outgoing` walk, leaf = pin
  with neither) into `dead_end_leaves(design, graph, PinId) -> Vec<u32>`
  — the same walk, parameterized on the start key.
- Inside `for (ci, (sigma, keys)) in classes.iter().enumerate()`
  (~L4394), compute first:

  ```rust
  let has_delay_rule =
      sigma.iter().any(|&ri| matches!(rules[ri as usize], ResolvedRule::Delay(_)));
  ```

  and skip the leaf block entirely when false. When true, iterate the
  class's keys, call `dead_end_leaves` per key, and process leaves
  inline — the `dead_ends` map disappears (each key lives in exactly
  one class, so nothing is recomputed).

This is behaviour-preserving: a leaf row can only be produced under a
delay rule, so SDCs without `set_max_delay`/`set_min_delay` (every
group/false-path-only scenario) never traverse. `t2_selmd`/`t2_mdx`
keep their `internal path endpoint` rows — gate them in the golden
comparison as before.

## 3. Stale comments

`hold_edge_pair` is min-gap since `t2_wave`; every comment still
describing the hold pair as *coincident* is stale:

- ~L5494–5498 (the flagged one): rewrite as the same-domain realign on
  waveform instants — `+P` when `cap_clk.edge_offset(c0) >
  cap_clk.edge_offset(g)` — plus the foreign-bit min-gap pairing.
- ~L4285: "A cross-domain hold pair lives on the coincident grid" →
  min-gap grid (the `d_min = −nh·Pc` context is unchanged).
- ~L5099–5101 (`xdomain_defer` doc): "pair coincident edges on the
  check/anchor grid" → min-gap on the check/anchor grid (the gating
  `hold_next` exception stays).
- ~L5336: "min-gap on setup, coincident on hold" → min-gap on both.

Audit the `coincident` hits (~L4961 `launch_domain`, ~L5031 setup
pairing) — those are correct: same-period grids do pair coincident
edges, and setup's strictly-after rule excludes coincidence. Do not
touch them.

## 4. What does not change

- `ObjSel::Clocks` resolution at parse (`clock_rows`, all-miss → empty
  rows → dead rule + one count under the §1 rule).
- `key_dom`/`ep_cap` construction and the `set_clock_groups` desugar —
  raw declared rows, lone-group semantics, `-allow_paths` no-store.
- `Clock::edge_offset`, `waveform_ns`, and every consumer site.
- `run_sta_detail`'s partition gate.

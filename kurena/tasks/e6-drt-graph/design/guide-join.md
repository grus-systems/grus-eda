# e6-drt-graph — Guide intake and net join

Reads the committed `tests/designs/<d>/<d>.guide` (the OpenROAD
`global_route -guide_file` reference from ticket 1) into per-net
`NetGuide`s joined to `DesignDb` nets.

## Parse + resolve (all existing)

```text
fs::read_to_string(<d>.guide)
  -> congestion::guide::parse_guides(&text, &tech)      // (name, boxes) in file order
  -> congestion::guide::resolve_guide_nets(&db, &parsed) // (NetId, boxes), file order
```

- **Extra nets**: `resolve_guide_nets` already `bail!`s with the
  offending name when a guide block names no design net — the spec's
  "extra" case, an error not a skip. Nothing new needed.
- **Missing nets**: nets in `net_names` with no guide block. Per the
  OpenROAD emitter's own rules a block is absent when the net has
  < 2 member pins (measured: picorv32's 67 port-only nets) or is
  non-signal. Both classes are *named* — a signal net missing from
  the guide is a named gap, never a silent skip.

## Signal-net classification

`net_names` carries every `NETS` record — including `+ USE
GROUND`/`+ USE POWER` nets (`VGND`, `VPWR` on gcd — verified). No
net-level use flag exists in `DesignDb`, so classify by member pins:

- instance pin (`pin_cells[p] == Some`): signal iff
  `pin_macro_pins[p]` is `None` (unmapped — conservative) or
  `!tech.macro_pins[mp].is_power`;
- io port (`pin_cells[p] == None`): signal iff `p ∈ db.io_pins`
  (`io_pins` is sorted — I5 — binary search; `+ SPECIAL` ports were
  already excluded at read).

A net is signal iff ≥1 member pin is signal. `VGND`/`VPWR` fail the
test on gcd: the ports are `+ SPECIAL` and the `*` wildcard members
are `is_power` macro pins. Clock nets pass (their members are
ordinary signal pins) — `+ USE CLOCK` nets are routed like signal
nets and belong in the gap check.

## Join output

```rust
pub struct GuideJoin {
    /// Guide blocks resolved to nets, in guide-file order.
    pub nets: Vec<(NetId, Vec<GuideBox>)>,
    /// ≥2-pin signal nets with no block — the named gap.
    pub missing_signal: Vec<NetId>,
    /// Absent for a known reason: <2 member pins or non-signal.
    /// Named in the census print, not a failure.
    pub absent_expected: Vec<NetId>,
}

pub fn join_guides(db: &DesignDb, tech: &TechDb, parsed: &[(String, Vec<GuideBox>)])
    -> anyhow::Result<GuideJoin>;
```

`missing_signal` and `absent_expected` iterate `net_names` order —
deterministic. A net present in the guide with an *empty* block
(`name ( )`) is not missing: it yields an empty region, and its
member pins land in `region_escape` instead.

Measured on gcd: `missing_signal = []`, `absent_expected = [VGND,
VPWR]` plus zero single-pin nets (gcd's NETS section has none with
<2 members; picorv32's ~67 port-only nets land here).

## Files

- `crates/grus-core/src/drt/graph.rs`

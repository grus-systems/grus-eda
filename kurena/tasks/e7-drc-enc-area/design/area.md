# e7-drc-enc-area — `drc::union` + `drc::area`

The area family: 6 minimum-area rules (`li.6`, `met1.6`–`met5.4`) and
2 minimum-hole-area rules (`met1.7`, `met2.7`). Both measure merged
per-layer polygons, so this ticket adds `drc::union` — component and
sweep machinery shared by the two checks. Bounds come from LEF
`AREA` / `MINENCLOSEDAREA` (`lef-rules.md`); `rule_name` strings are
verbatim templates.

## `drc::union` — polygon substrate

Two structure levels, both over `LayerShapes`:

```rust
/// Member-source filter: rects that are real metal on the plane.
/// `MacroObs` is not a member (blockage abstract, not fabricated
/// metal) but is consulted separately for hole discards.
pub fn is_member(src: ShapeSrc) -> bool {
    matches!(src, SignalWire | SpecialWire | Via | IoPort | MacroPin)
}

/// Connected components of member rects on one layer —
/// Magic's merged plane polygons.
pub struct LayerComponents {
    /// rects index -> component id
    pub comp_of: Vec<u32>,
    /// component id -> member `rects` indices
    pub members: Vec<Vec<u32>>,
}
pub fn components(ls: &LayerShapes) -> LayerComponents;

/// Union area of a rect set, DBU² — one x-sweep.
pub fn union_area(rects: &[RectDbu]) -> i64;

/// One enclosed void: the free-space region plus its totals.
pub struct Hole {
    /// Free rects of the component (for OBS adjacency tests).
    pub rects: Vec<RectDbu>,
    pub bbox: RectDbu,
    pub area_dbu2: i64,
}
/// Free-space components inside `window` that never touch its
/// boundary — enclosed by the `solid` union. `solid` is every member
/// rect overlapping the window (any component, any net).
pub fn enclosed_voids(solid: &[RectDbu], window: RectDbu) -> Vec<Hole>;
```

**Components.** `ls.for_each_pair_within(1)` — the same closed-touch
sweep spacing's same-layer pass uses (touch or overlap, corner
contact counts) — merged via union-find **regardless of net**: Magic
extracts polygons from the merged paint, not from node labels. A
two-net touch is a short elsewhere, not something the area check
splits hairs over. `MacroObs` rects are never members.

**Union area.** Sort member `xl`/`xh` into slab events; maintain the
active interval set; per slab, merge y-intervals and add
`slab_w × Σ merged_len` (i64). Called per component — member sets are
small and x-disjoint in aggregate, so total work is ~O(n log n) per
layer even with a die-spanning rail component.

**Enclosed voids.** Same x-sweep inside `window` (solid rects clipped
to it): per slab the free intervals are the metal-union complement
within `[window.yl, window.yh]`. Free rects merge into components via
union-find across adjacent slabs on **positive y-overlap** —
edge-connected only; a void reaching outside through a point-pinched
channel counts as enclosed (corner contact does not open it). A
component with no free rect touching any `window` edge is a hole.

The boundary test is what separates exterior notches from holes: a
C-shaped polygon's gap reaches the window edge → exterior; a donut's
void never does → hole. Voids are computed per component window
(equivalent to a global pass — a true hole is enclosed by a single
connected polygon, so it lives inside exactly one component's bbox)
which keeps free-rect counts bounded by component size rather than
layer size.

## `drc::area` — the checks

```rust
pub struct AreaRule {
    pub rule: &'static str,
    pub rule_name: &'static str,
    pub layer: &'static str,   // LEF routing-layer name
}

pub const AREA_RULES: &[AreaRule] = &[
    // li.6, met1.6, met2.6, met3.6, met4.4a, met5.4 — see lef-rules.md
];

pub const HOLE_RULES: &[AreaRule] = &[
    // met1.7, met2.7
];

/// A sub-bound merged polygon (min-area) or enclosed void (holes).
pub struct Violation {
    pub rule: &'static str,
    pub rule_name: &'static str,
    pub layer: DrcLayer,
    /// Component bbox (min-area) or void bbox (holes).
    pub region: RectDbu,
    pub area_dbu2: i64,
    pub bound_dbu2: i64,
    /// First net-tagged member's net, when any — diagnostics.
    pub net: Option<SymbolId>,
}

pub struct RuleReport { /* rule, layer, bound_dbu2, candidates, violations */ }

pub fn check_area_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport>;
pub fn check_area(design: &DrcDesign, tech: &TechDb) -> Vec<Violation>;
pub fn check_hole_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport>;
pub fn check_holes(design: &DrcDesign, tech: &TechDb) -> Vec<Violation>;
```

### Min-area evaluation

Per rule: resolve layer → `area_dbu2` bound (0 = undeclared, skip).
`components(ls)`; a component is **evaluated** (`candidates += 1`)
iff it holds >= 1 member of `SignalWire | SpecialWire | Via | IoPort`
— drawn metal. `MacroPin`-only components are skipped: pin abstracts
understate the real GDS, so flagging them can only over-report
(recorded, same class as width's finding filter). A evaluated
component with `union_area < bound` (strict `<`, matching width) is
one violation — `region` = component bbox, `net` = first `Some(net)`
member.

### Hole evaluation

Per rule: resolve layer → bound = `min_enclosed_area_dbu2`, else the
deck-pinned `140000` fallback (`lef-rules.md`). For each component
with a drawn member:

1. `window` = component bbox; `solid` = member rects of **any**
   component/net overlapping the window via `iter_overlapping` — a
   foreign wire inside a donut occupies void space.
2. `enclosed_voids(solid, window)` → candidate voids.
3. **OBS discard** — a void is dropped when any of its free rects
   closed-intersects (touch counts) a `MacroObs` rect on the layer.
   Real content hides behind the obs abstract; keeping the void can
   only over-report. Discarding the whole void rather than subtracting
   the obs rect is the conservative direction.
4. Remaining voids with `area < bound` flag (strict `<` — the name's
   `>` reads exact-bound as clean).

## Semantics notes vs Magic

- **Net-agnostic merge** mirrors the plane: two same-layer touching
  rects merge whether or not their nets match (recorded — a mismatch
  is a short, not an area rule input).
- **Point-pinched voids are holes.** Free space merges on positive
  edge overlap only; a channel closed to a point does not connect.
  Magic's degenerate-channel behavior is unverifiable from LEF+DEF —
  recorded, and exercised by a fixture.
- **OBS never fills or encloses** — excluded from `solid` members;
  used only to discard candidate voids.
- **Corner-connected metals merge** (closed-touch), consistent with
  the spacing island model: two rects meeting at a point form one
  polygon, which can wall a hole.

## Files

- `crates/grus-core/src/drc/union.rs`
- `crates/grus-core/src/drc/area.rs`
- `crates/grus-core/src/drc.rs` (module decls)
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`

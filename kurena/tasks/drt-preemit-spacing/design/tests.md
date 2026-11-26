# Tests — `crates/grus-core/tests/drt.rs`

Two new tests in the emit section (after `test_emit_determinism`),
plus the `main.rs` argv assert. Both reuse `emit_leg("gcd",
&MazeConfig::default())` — the established ~seconds emit leg.

## `test_preemit_gcd` — clean case + shape-set equivalence

```rust
let (tech, db, _route_db, _vias, _rep) = emit_leg("gcd", &MazeConfig::default());
let pre = preemit_check(&db, &tech).expect("preemit_check");
```

Asserts, in order:

1. `findings_total(&pre) == 0` — clean gcd (mirrors
   `test_emit_selfdrc_gcd`'s bar).
2. Non-vacuity: `candidates > 0` summed over `Family::Spacing` rules
   (and the four checked families as in `emit_gate`) — a check that
   saw nothing proves nothing.
3. Determinism: `preemit_check(&db, &tech)` twice → `assert_eq!(pre, pre2)`.
4. **Equivalence (the load-bearing one)**: `write_routed_def` →
   `self_drc(&out)` → `assert_eq!(pre, drc_report)` — the in-memory
   report equals the emit→parse→assemble report verbatim. This is the
   "same rectangles" acceptance claim, tested per AGENTS §7.2 rather
   than asserted in prose. Any future emit/parse drift breaks this
   test, not a sentence in docs.
5. IoPort presence in the assembled set: scan `assemble(&db)` for
   `src == ShapeSrc::IoPort` and assert non-empty — pins the
   "including foreign pin shapes" coverage claim to a shape gcd
   actually has.

## `test_preemit_foreign_pin` — violation fixture (fail-proof arm)

Deterministic inject, the `test_emit_fail_proof` arm-A idiom, targeted
at a foreign **io-port** rect — the checkable `Pin` class occupancy
soft-priced:

```rust
let (tech, mut db, _route_db, _vias, _rep) = emit_leg("gcd", &MazeConfig::default());
let d = assemble(&db, &tech).expect("assemble");
// first IoPort shape on a routing layer whose bound > 1
let (lid, pin_rect, port_net) = d.routing.iter().enumerate()
    .flat_map(|(l, ls)| ls.rects.iter().map(move |s| (l, s)))
    .find_map(|(l, s)| (s.src == ShapeSrc::IoPort && s.net.is_some())
        .then(|| (LayerId(l as u32), s.rect, s.net.unwrap())))
    .expect("gcd emit has io-port shapes (met3 ports)");
let bound = tech.routing_layers[lid.0 as usize].min_spacing_dbu();
// inject a SignalWire of a different net at gap 1 < bound, y-aligned
let b = (0..db.num_nets())
    .find(|&n| db.net_names[n] != port_net)
    .expect("a foreign net exists");
let w = RectDbu::new(pin_rect.xh + 1, pin_rect.yl,
                     pin_rect.xh + 1 + bound, pin_rect.yh);
db.net_geoms[b].wire_rects.push((lid, w));
```

Then `preemit_check(&db, &tech)` and assert:

- `findings_total >= 1`;
- at least one `Family::Spacing` finding `f` with
  `f.bboxes.contains(&pin_rect)` **and** `f.nets` containing the port
  net's resolved name — proves the foreign pin is a finding endpoint,
  not just present geometry;
- print the finding via `print_gate`-style lines for the results log.

The injected pair satisfies the spacing conditions by construction:
`gx=1`, `gy=0` → different islands, both checkable, `1² < bound²`.
Extra findings caused elsewhere by the injected wire are fine (the
assert is `≥1` plus the pin-rect membership, not an exact count).

Red-proof protocol (AGENTS §7.7): temporarily gate the IoPort shape
out of `assemble` (or `checkable` out of the spacing filter) → the
test goes red → restore. Record the run in `docs/results.md`.

## `main.rs` argv test

`--preemit-check` added to the `drt` argv fixture; `assert!(args.preemit_check)`.

## What is intentionally not tested

- picorv32: `emit_leg` ≈ 135 min release — the gcd pair covers the
  mechanism; note the omission in `results.md`.
- CLI end-to-end exit code: report-only by design this ticket; the
  exit-code fixture belongs to `drt-selfdrc-exit`.
- `MacroPin`-endpoint findings: the checker deliberately cannot
  produce them (semantics.md) — asserting them would be asserting a
  change to `checkable`.

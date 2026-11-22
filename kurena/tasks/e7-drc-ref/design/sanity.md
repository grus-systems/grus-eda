# e7-drc-ref — Sanity control (broken DEF)

The control that proves the reference surface *detects* violations
rather than only reporting 0 (the AGENTS.md §7.7 "prove a test can
fail" discipline, applied to the reference itself). One scripted,
named mutation; one extra Magic run; nonzero errors + named rules
required.

## Mutation: `add_spacing_probe_wire`

Why this shape: to produce a **different-net spacing** violation by
construction, deterministic against any neighbor analysis. Alternatives
rejected:

- **Shift an existing wire coordinate** — a spacing violation only if
  some different-net shape happens to sit within range; unpredictable
  without a geometry pass, and the shifted wire can also just misalign
  (yielding no error or an unrelated rule). Not self-contained.
- **Shift a placed instance** — gcd's placement is not fully dense; a
  sub-min-spacing shift may land inside the row gap and produce no
  violation at all.
- **Sub-min-width wire/rect** — deterministic, but exercises `width`
  rather than the `spacing` family the spec names. Kept as the
  documented fallback if `add_spacing_probe_wire` ever fails to fire.

Algorithm (python3 heredoc, step D):

1. Parse the input DEF `NETS` section. Collect each net's routing
   statements (`+ ROUTED <layer> ( ... ) ...`, `NEW <layer> ( ... )`).
   Skip nets whose statements carry `NONDEFAULTRULE` (non-default
   widths break the offset math).
2. Choose `net_a`: the first signal net carrying a **horizontal**
   `met2` routed segment `(x1 y) (x2 y)`; record its centerline `y` and
   `x1`. A vertical-first DEF flips the mutation to the x axis (same
   math, transposed). Fallback layer order `met2 → met1 → met3`; no
   routable segment found → abort (the fixture assumption failed
   loudly).
3. Read `LAYER <layer>` `WIDTH` and minimum `SPACING` from the tech
   LEF (`/pdk/.../sky130_fd_sc_hd__nom.tlef`) — constants are parsed,
   not hardcoded. sky130 met2: width 0.14 µm, min spacing 0.14 µm
   (expected; the parse pins it).
4. Choose `net_b`: the first other signal net. Append to `net_b`'s
   statement list (before its terminating `;`) one disjoint wire path
   — `NEW` is the same grammar `detailed_route` emits (a bare
   continuation keyword, not a `+` statement), so every
   DEF reader that accepts the input accepts the mutation:

   ```
      NEW met2 ( x1  y+W+gap ) ( x1+len  y+W+gap )
   ```

   where `W = WIDTH` (both wires default width; edges sit
   `W/2 + gap + W/2` apart centerline-to-centerline, i.e. `gap` edge
   distance), `gap = 0.03 µm` (30 DBU — under 0.14 µm min spacing,
   nonzero so it reads as spacing, not a short), `len = 0.5 µm`. The
   new wire runs parallel to `net_a`'s, 30 nm edge-to-edge → exactly
   one violation class: `met2 spacing`.

5. Write `runs/drc/<d>.drc.broken.def` (byte-identical to the input
   except the one inserted statement — diff-able evidence); print the
   `DRC-MUTATION` summary line with every parameter.

Contingency: if Magic's DEF reader rejects or ignores the `NEW`
statement (transcript shows the shape unread), substitute `+ RECT
met2 ( llx lly ) ( urx ury )` with the same edge gap — record which
statement form was used in `mutation.name`/`docs/results.md`.

## What must be recorded

`sanity_control` block in the JSON (`golden.md`): mutation name + all
parameters (nets, layer, gap, segment endpoints, parsed layer
constants), broken
DEF path + sha, `magic_total`, `magic_per_rule`, verbatim
`violated_rule_names`, and `violated_rules_in_checked_deck`.

Assertions in step E (all loud):

- `magic_total > 0` on the broken DEF; empty `violated_rule_names`
  aborts.
- The violated names are recorded verbatim — the expected family is
  "<layer> spacing", but whatever Magic names is data. If the names
  surprise (e.g. a different rule family fires first), that is
  recorded in `docs/results.md`, not smoothed.
- `violated_rules_in_checked_deck` cross-checks the names against the
  tech-file extraction — `false` means the taxonomy extraction missed
  a rule Magic reports; flag it.

## Why two Magic processes

The broken run is a separate `magic` invocation (`runs/drc` cwd,
`magic_broken.log`) — clean state, same convention as the irdrop
three-process precedent; the clean run's 0-count and the broken run's
nonzero count come from identical invocation so the delta attributes
to the mutation alone.

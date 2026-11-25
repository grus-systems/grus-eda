#!/usr/bin/env python3
"""Unrouted-census recount for a committed DRT ours-golden (E6).

Joins `tests/designs/<d>/<d>.drt.ours.reference.json` to the committed
`tests/designs/<d>/<d>.guide` and recounts the unrouted decomposition
that docs/results.md quotes. Committed inputs only — the routed DEF and
`runs/` artifacts are gitignored and are never inputs here.

Semantics (what each golden field actually is):

- `ours_router.unrouted` — final unrouted net names (U).
- `cli_lines` `  unrouted <net>` — the same set, emit-side.
- `cli_lines` `  stub_outside_cover <net> <inst>/<pin>` — one record per
  pin that *ever* had no legal paint, accumulated across all rip-up
  iterations (a recorded pin may re-attach or end `NoPath`; it is not
  the final `Unattachable` class).
- `unrouted_kinds` — per-pin *final* failure counts (`UnroutedNet.failed`);
  a net may carry pins of both kinds and no per-net kind is recorded.
- `unrouted_blockers` — aggregate per-pin blocker attribution.

Guide-union census: union-find over each net's guide boxes; two boxes
union when |Δlayer| <= 1 and their rects strictly 2D-overlap (the
`guide_union_connected` classifier in crates/grus-core/tests/drt.rs).
Layer index order: li1=0, met1=1, met2=2, met3=3, met4=4, met5=5.
Classes: `met2_connected` (<=met2 union connected), `met3_bridge`
(<=met2 disconnected, full union connected), `disjoint` (full union
disconnected), `guide_absent` (net missing from the guide).

Usage: recount_unrouted.py <design>   # e.g. picorv32, gcd
"""

import json
import re
import sys
from pathlib import Path

DESIGNS = Path(__file__).resolve().parent

LAYER_INDEX = {"li1": 0, "met1": 1, "met2": 2, "met3": 3, "met4": 4, "met5": 5}
MET2 = 2


def fail(msg):
    print(f"recount_unrouted: FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


def parse_guide(path):
    """name -> [(xlo, ylo, xhi, yhi, layer_index), ...]; names verbatim."""
    nets = {}
    name, boxes = None, None
    with open(path) as f:
        for lineno, raw in enumerate(f, 1):
            line = raw.strip()
            if not line:
                continue
            if name is None:
                if line == "(":
                    fail(f"{path}:{lineno}: box group without a net name")
                name = line
            elif boxes is None:
                if line != "(":
                    fail(f"{path}:{lineno}: expected `(` after net name, got `{line}`")
                boxes = []
            else:
                if line == ")":
                    nets[name] = boxes
                    name, boxes = None, None
                    continue
                fields = line.split()
                if len(fields) != 5:
                    fail(f"{path}:{lineno}: box line must be `xlo ylo xhi yhi layer`")
                try:
                    box = tuple(int(v) for v in fields[:4])
                except ValueError:
                    fail(f"{path}:{lineno}: bad rect `{line}`")
                layer = LAYER_INDEX.get(fields[4])
                if layer is None:
                    fail(f"{path}:{lineno}: unknown layer `{fields[4]}`")
                boxes.append(box + (layer,))
    if name is not None:
        fail(f"{path}: unterminated guide entry `{name}`")
    return nets


def union_connected(boxes, max_layer):
    """Single component under |Δlayer|<=1 + strict 2D overlap."""
    kept = [b for b in boxes if b[4] <= max_layer]
    if not kept:
        return False
    parent = list(range(len(kept)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for a in range(len(kept)):
        ba = kept[a]
        for b in range(a + 1, len(kept)):
            bb = kept[b]
            if abs(ba[4] - bb[4]) > 1:
                continue
            if ba[0] < bb[2] and ba[2] > bb[0] and ba[1] < bb[3] and ba[3] > bb[1]:
                ra, rb = find(a), find(b)
                if ra != rb:
                    parent[ra] = rb
    return len({find(i) for i in range(len(kept))}) == 1


def census(names, guide):
    """Per-class counts over a net-name set; returns (counts, bridged)."""
    counts = {"met3_bridge": 0, "met2_connected": 0, "disjoint": 0, "guide_absent": 0, "met3_plus": 0}
    bridged = []
    for n in names:
        boxes = guide.get(n)
        if boxes is None:
            counts["guide_absent"] += 1
            continue
        if any(b[4] > MET2 for b in boxes):
            counts["met3_plus"] += 1
        if union_connected(boxes, MET2):
            counts["met2_connected"] += 1
        elif union_connected(boxes, max(LAYER_INDEX.values())):
            counts["met3_bridge"] += 1
            bridged.append(n)
        else:
            counts["disjoint"] += 1
    if counts["met3_bridge"] + counts["met2_connected"] + counts["disjoint"] + counts[
        "guide_absent"
    ] != len(names):
        fail(f"census does not sum: {counts} over {len(names)} nets")
    return counts, bridged


def print_census(label, names, guide):
    counts, bridged = census(names, guide)
    print(
        f"{label} ({len(names)} nets): met3+={counts['met3_plus']} "
        f"met3_bridge={counts['met3_bridge']} met2_connected={counts['met2_connected']} "
        f"disjoint={counts['disjoint']} guide_absent={counts['guide_absent']}"
    )
    if bridged:
        print(f"  met3_bridge nets: {', '.join(bridged)}")
    return counts


def main():
    if len(sys.argv) != 2:
        fail("usage: recount_unrouted.py <design>")
    d = sys.argv[1]
    golden_path = DESIGNS / d / f"{d}.drt.ours.reference.json"
    guide_path = DESIGNS / d / f"{d}.guide"
    for p in (golden_path, guide_path):
        if not p.is_file():
            fail(f"missing input {p}")

    golden = json.loads(golden_path.read_text())
    or_ = golden["ours_router"]
    unrouted = or_["unrouted"]
    kinds = or_["unrouted_kinds"]
    blockers = or_["unrouted_blockers"]
    cli = or_["cli_lines"]

    u = set(unrouted)
    if len(u) != len(unrouted):
        fail(f"`unrouted` field has duplicate names ({len(unrouted)} entries, {len(u)} distinct)")
    if len(u) != or_["unrouted_count"]:
        fail(f"unrouted set {len(u)} != unrouted_count {or_['unrouted_count']}")

    cli_unrouted = set()
    stub_nets = set()
    stub_pins = set()
    stub_lines = 0
    for line in cli:
        parts = line.split()
        if parts[0] == "unrouted" and len(parts) == 2:
            cli_unrouted.add(parts[1])
        elif parts[0] == "stub_outside_cover":
            stub_lines += 1
            stub_nets.add(parts[1])
            stub_pins.add((parts[1], parts[2]))
    if cli_unrouted != u:
        fail(
            f"cli_lines `unrouted` set ({len(cli_unrouted)}) != `unrouted` field ({len(u)}): "
            f"only-cli={sorted(cli_unrouted - u)[:5]} only-field={sorted(u - cli_unrouted)[:5]}"
        )

    unatt = kinds["Unattachable"]
    nopath = kinds["NoPath"]
    if stub_lines < unatt:
        fail(f"stub_outside_cover lines {stub_lines} < Unattachable {unatt}")

    s_in_u = u & stub_nets
    s_not_u = stub_nets - u
    u_not_s = u - stub_nets

    blk_total = sum(blockers.values()) if blockers else 0
    named = sorted(n for n in u if not re.fullmatch(r"_\d+_", n))

    print(f"== {d} unrouted census recount ==")
    print(f"unrouted={len(u)} unrouted_kinds NoPath={nopath} Unattachable={unatt}")
    print(
        f"stub_outside_cover lines={stub_lines} distinct_pins={len(stub_pins)} "
        f"distinct_nets={len(stub_nets)}"
    )
    print(f"stub∩unrouted={len(s_in_u)} stub∖unrouted={len(s_not_u)} unrouted∖stub={len(u_not_s)}")
    print(f"unrouted_blockers total={blk_total} {blockers}")
    print(f"named unrouted nets (not `_N_`)={len(named)}")
    print(
        f"kind-split bounds: <= {min(unatt, len(s_in_u))} nets carry a final Unattachable pin "
        f"(⊆ stub∩unrouted); >= {len(u) - unatt} unrouted nets all-NoPath "
        f"(incl. all {len(u_not_s)} unrouted∖stub); "
        f">= {len(s_in_u) - unatt} stub-carriers NoPath-only"
    )

    guide = parse_guide(guide_path)
    print_census("unrouted∖stub", sorted(u_not_s), guide)
    print_census("stub∩unrouted", sorted(s_in_u), guide)
    print_census("unrouted", sorted(u), guide)


if __name__ == "__main__":
    main()

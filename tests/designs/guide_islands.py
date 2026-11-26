#!/usr/bin/env python3
"""Dead-component census for guide SPEFs (E4 topology axis).

Counts, per net, the parasitic components a driver's BFS cannot reach —
the "dead islands" OpenROAD's guide SPEFs carry and ours must not carry
more of. Inputs: any SPEF with `*CONN`/`*RES` sections — our emitted
`<d>.guide.ours.spef`, the reference `<d>.guide.spef`, or the OpenROAD
read-back `runs/guide-check/<d>.ours.guide.spef`.

Definitions (the numbers are only meaningful under exactly these rules):

- Union-find over the node names in each net's `*RES`; one edge per
  non-self-loop resistor (`a != b`). Self-loop resistors (`n n r`) are
  counted separately by R value — a stub on layer `L` records the LEF
  `RESISTANCE` of the cut above it (sky130: `0.0093` = stub on `li1`/mcon,
  `0.0045` = `met1`/via1, `0.00341` = `met2` or `met3` (via2/via3 share
  the value), `0.00038` = `met4`/via4).
- `*I`/`*P` names from `*CONN` are nodes. The driver root is the first
  `*I` with direction `O`, else the first `*P` with direction `I` (input
  port driving the net), else the first conn name that appears in a
  non-self-loop `*RES`.
- A dead component is a union-find component not containing the driver
  root. Report per design: nets with >= 1 dead component, total dead
  components, components split by size (`1` node vs `>= 2` = the
  multi-node detour-island class), and the dead `*P` list (`*P` names
  inside a dead component, per net).

Usage: guide_islands.py <spef> [<spef> ...]
"""

import sys


def parse_spef(path):
    """Return [(net, conns, res)] — conns: [(kind, name, dir)],
    res: [(a, b, r)]. Node names compared verbatim (escapes kept)."""
    nets = []
    net = conns = res = None
    section = None
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line.startswith("*D_NET "):
                net = line.split()[1]
                conns, res = [], []
                nets.append((net, conns, res))
                section = None
            elif line == "*CONN":
                section = "conn"
            elif line == "*RES":
                section = "res"
            elif line == "*CAP" or line == "*END":
                section = None
            elif section == "conn":
                parts = line.split()
                if len(parts) >= 3 and parts[0] in ("*I", "*P"):
                    conns.append((parts[0], parts[1], parts[2]))
            elif section == "res":
                parts = line.split()
                if len(parts) == 4:
                    res.append((parts[1], parts[2], float(parts[3])))
    return nets


class UnionFind:
    def __init__(self):
        self.parent = {}

    def find(self, x):
        p = self.parent.setdefault(x, x)
        while p != self.parent[p]:
            self.parent[p] = self.parent[self.parent[p]]
            p = self.parent[p]
        return p

    def union(self, a, b):
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.parent[rb] = ra


def census(path):
    nets = parse_spef(path)
    nets_dead = 0
    total_dead = 0
    dead_1 = 0  # single-node components
    dead_multi = 0  # >= 2 nodes — the detour-island class
    nets_multi = 0  # nets carrying >= 1 multi-node dead component
    dead_ports = []  # (net, port) — *P names inside dead components
    self_loops = {}  # R value -> count

    for name, conns, res in nets:
        uf = UnionFind()
        for a, b, r in res:
            if a == b:
                self_loops[r] = self_loops.get(r, 0) + 1
            else:
                uf.union(a, b)
        conn_names = {n for _, n, _ in conns}

        root = None
        for kind, n, d in conns:
            if kind == "*I" and d == "O":
                root = n
                break
        if root is None:
            for kind, n, d in conns:
                if kind == "*P" and d == "I":
                    root = n
                    break
        if root is None:
            for a, b, _ in res:
                if a != b and (a in conn_names or b in conn_names):
                    root = a if a in conn_names else b
                    break
        # No root (no conn in a resistor): the largest component is the
        # live one — mirrors the BFS seeding a driver pin that never
        # appears in *RES.
        comps = {}
        for a, b, r in res:
            for n in (a, b):
                comps.setdefault(uf.find(n), set()).add(n)
        for n in conn_names:
            comps.setdefault(uf.find(n), set()).add(n)
        root_comp = uf.find(root) if root is not None else None
        if root_comp is None and comps:
            root_comp = max(comps, key=lambda c: len(comps[c]))

        dead = [c for rc, c in comps.items() if rc != root_comp]
        if dead:
            nets_dead += 1
        total_dead += len(dead)
        multi = [c for c in dead if len(c) >= 2]
        dead_1 += sum(1 for c in dead if len(c) == 1)
        dead_multi += len(multi)
        if multi:
            nets_multi += 1
        for c in dead:
            for kind, n, _ in conns:
                if kind == "*P" and n in c:
                    dead_ports.append((name, n))

    print(f"== {path}")
    print(
        f"nets={len(nets)} nets_with_dead={nets_dead} dead_components={total_dead} "
        f"(1-node={dead_1} multi-node={dead_multi}, nets_with_multi={nets_multi})"
    )
    for r in sorted(self_loops):
        print(f"self-loops R={r}: {self_loops[r]}")
    print(f"self-loops total: {sum(self_loops.values())}")
    print(f"dead *P: {len(dead_ports)}")
    for net, port in dead_ports:
        print(f"  {net} {port}")
    print()


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    for path in sys.argv[1:]:
        census(path)
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Per-net `*D_NET` total-capacitance extraction and diffing for guide SPEFs.

Used by `generate_ref.sh` step 5d (extract) and by the direction-(b)
comparisons in docs/results.md (diff). Names are compared verbatim —
SPEF escapes are kept, matching the raw DEF spelling `guide_net_name`
emits.

Usage:
  guide_caps.py extract <spef> <out.txt>   # name<TAB>cap_pf, SPEF order
  guide_caps.py diff <caps_a> <caps_b>     # per-net table + stats to stdout
"""

import sys

TOL_REL = 0.05
TOL_ABS_PF = 1e-3  # 1 fF in pF


def read_caps(path):
    rows = []
    with open(path) as f:
        for line in f:
            parts = line.split("\t")
            if len(parts) == 2:
                rows.append((parts[0], float(parts[1])))
    return rows


def extract(spef_path, out_path):
    rows = []
    with open(spef_path) as f:
        for line in f:
            if line.startswith("*D_NET "):
                _, name, cap = line.split()
                rows.append((name, cap))
    with open(out_path, "w") as f:
        for name, cap in rows:
            f.write(f"{name}\t{cap}\n")
    print(f"extract: {len(rows)} nets from {spef_path} -> {out_path}")


def diff(path_a, path_b):
    a = read_caps(path_a)
    b = read_caps(path_b)
    map_b = dict(b)
    names_a = {n for n, _ in a}
    names_b = {n for n, _ in b}
    both = names_a & names_b
    if not both:
        print("diff: name sets are disjoint — wrong inputs?", file=sys.stderr)
        return 1

    print(f"{'net':40s} {'a_pf':>12s} {'b_pf':>12s} {'rel':>9s} {'in_tol':>6s}")
    rows = []
    for name, cap_a in a:
        if name not in map_b:
            continue
        cap_b = map_b[name]
        delta = abs(cap_a - cap_b)
        rel = delta / cap_b if cap_b else 0.0
        tol = max(TOL_REL * cap_b, TOL_ABS_PF)
        rows.append((name, cap_a, cap_b, delta, rel, delta <= tol))
    worst = sorted(rows, key=lambda r: r[3], reverse=True)
    for name, cap_a, cap_b, delta, rel, ok in worst:
        print(
            f"{name:40s} {cap_a:12.6f} {cap_b:12.6f} {rel * 100:8.2f}% {'yes' if ok else 'no':>6s}"
        )

    rels = sorted(r[4] for r in rows)
    n = len(rows)
    inside = sum(1 for r in rows if r[5])
    only_a = sorted(names_a - names_b)
    only_b = sorted(names_b - names_a)
    print()
    print(f"nets compared: {n} (only-in-a: {len(only_a)}, only-in-b: {len(only_b)})")
    print(
        f"within max(5%, 1 fF): {inside}/{n} = {inside / n * 100:.1f}%"
        if n
        else "within tol: n/a"
    )
    if n:
        med = rels[n // 2] if n % 2 else (rels[n // 2 - 1] + rels[n // 2]) / 2
        p90 = rels[int(0.9 * (n - 1))]
        print(f"rel diff: median={med * 100:.2f}% p90={p90 * 100:.2f}% max={rels[-1] * 100:.2f}%")
        print("worst 5:")
        for name, cap_a, cap_b, delta, rel, ok in worst[:5]:
            print(f"  {name}: a={cap_a:.6f} b={cap_b:.6f} rel={rel * 100:.2f}%")
    if only_a:
        print(f"only in a ({path_a}): {', '.join(only_a[:20])}")
    if only_b:
        print(f"only in b ({path_b}): {', '.join(only_b[:20])}")
    return 0


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "extract":
        extract(sys.argv[2], sys.argv[3])
        return 0
    if len(sys.argv) == 4 and sys.argv[1] == "diff":
        return diff(sys.argv[2], sys.argv[3])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())

# e8-solve — Electrical and solver model

## Boundary conditions

`PdnMesh::sources` are voltage-source contact nodes. They are Dirichlet
boundary nodes, not unknowns:

```text
V_s = source_voltage_v     for every s in mesh.sources
```

`source_voltage_v` is input data. The CLI reads it from
`nets.<NET>.conventions.embedded.supply_voltage_v` in the committed
reference JSON; the value is never hard-coded. Source-node voltages are
assigned directly to `node_voltages_v`, so `== source_voltage_v` is an
exact check, including `0.0` for VGND.

## Current source magnitude and sign

`cell_power_w[c]` is the N7 per-instance total for `CellId(c)`, in watts.
`current_scale_v` is the positive rail-to-rail voltage used to convert
power to current. For the committed designs it is
`abs(VPWR_supply - VGND_supply) = 1.8`; the CLI falls back to
`PowerReport.voltage_v` only when the reference does not contain a POWER
and a GROUND rail to form the span.

For each net, count how many `mesh.sinks` belong to each cell. A cell's
current is split evenly across its terminals on that net:

```text
I_pin = cell_power_w[cell] / (current_scale_v * sink_count[cell])
```

Every `I_pin` must be finite and `>= 0`; the input length must be
`db.num_cells()`. This is checked at the public boundary. The same
positive magnitude drives both rails; only the RHS sign changes:

- `PdnUse::Power`: current leaves the rail through the instance,
  `b[node] -= I_pin`, producing `V <= V_source`.
- `PdnUse::Ground`: current returns into the ground mesh,
  `b[node] += I_pin`, producing `V >= V_source`.

A sink that lands on a source node still counts in `total_current_a`;
because the node is clamped, its load is supplied directly by the source
and does not perturb free nodes.

## Reduced conductance system

Let `F` be the non-source mesh nodes in ascending mesh-node order and
`S` the source nodes. For each resistor `(a, b, r_ohms)` convert to
conductance in f64:

```text
g = 1.0 / f64(r_ohms)
```

Stamp it into the reduced matrix `A = G_ff` and RHS `b`:

| endpoints | matrix stamp | RHS stamp |
|---|---|---|
| `a ∈ F`, `b ∈ F` | `A[a,a] += g; A[b,b] += g; A[a,b] -= g; A[b,a] -= g` | none |
| `a ∈ F`, `b ∈ S` | `A[a,a] += g` | `b[a] += g * V_s` |
| `a ∈ S`, `b ∈ F` | `A[b,b] += g` | `b[b] += g * V_s` |
| both in `S` | none | none |

Then apply the signed sink terms above. Parallel resistors naturally sum
because identical `(row,col)` entries are accumulated in a `BTreeMap`
before conversion to CSC. `a == b` is a no-op (ticket 3 already drops
and counts self-loops).

The system solved is therefore:

```text
G_ff * V_F = G_fs * V_s + I_signed
```

with `I_signed < 0` for POWER loads and `I_signed > 0` for GROUND loads.

## SPD property

Before assembly, `solve_mesh` calls `mesh.validate()` and rejects a
non-empty violation list. A good mesh has:

- finite, positive resistances;
- at least one source;
- every node reachable from a source through resistors.

`A` is the principal submatrix of a weighted graph Laplacian after
grounding source nodes. Symmetry comes from stamping both off-diagonal
entries; strict diagonal dominance for every source-connected component
follows from the eliminated `g` terms. A component with no clamped node
would be semidefinite, but `validate()`'s `UnreachableNode` check rejects
it first. Cholesky failure is still surfaced as an error rather than
silently retried.

## Solver backend

Use `nalgebra-sparse 0.12` only:

```rust
nalgebra_sparse::CooMatrix<f64>
nalgebra_sparse::CscMatrix<f64>
nalgebra_sparse::factorization::CscCholesky
nalgebra_sparse::na::DMatrix
```

Assembly stores unique `(row, col)` entries in `BTreeMap`, pushes them
into `CooMatrix`, converts to `CscMatrix`, factors with
`CscCholesky::factor`, and solves a single-column `DMatrix`.

Rationale for the chosen backend:

- pure-Rust Apache-2.0, no system SuiteSparse dependency;
- direct solve on an SPD matrix, no CG tolerance/preconditioner tuning;
- deterministic for a fixed input;
- 244 and ~5.5k free nodes are small enough for this ticket's CPU-only
  scope; runtime and peak RSS are measured on picorv32;
- rejects `sprs-ldl` (LGPL), `nalgebra-sparse-linalg` CG (another crate
  plus an iterative tolerance contract), and a hand-rolled CG (same
  tolerance question without an independent direct check).

The upstream `CscCholesky` docs note that it performs no fill-reducing
ordering. That is accepted here and recorded because the mesh is small
and mostly chain/grid-like; if measured fill becomes the dominant cost,
the implementation can swap only the backend while retaining this
assembly/API. The E8 commit must state the method actually used.

## Outputs and scalar semantics

`node_voltages_v` is dense in mesh-node order. Source entries are
assigned exactly; free entries come from the solve. `sink_voltages_v`
parallels `mesh.sinks`.

All scalar voltages use the sink row set, not every mesh node, matching
PDNSim's per-instance map:

```text
deviation_i = V_source - V_i      POWER
deviation_i = V_i - V_source      GROUND

average_voltage_v   = mean(V_i)
worstcase_voltage_v = min(V_i)    POWER
                    = max(V_i)    GROUND
average_drop_v      = mean(deviation_i)    (positive magnitude)
worst_drop_v        = max(deviation_i)     (positive magnitude)
drop_percent        = 100 * worst_drop_v / current_scale_v
```

The names keep the golden's `drop` vocabulary even though `use_kind ==
Ground` reports bounce. `drop_semantics` records `drop` or `bounce`.
Empty sink sets report voltage `V_source`, zero drops, and zero current.

Currents and powers recorded:

```text
total_current_a     = sum(I_pin over every sink)
injected_power_w    = total_current_a * current_scale_v
uninjected_power_w  = sum(cell_power_w) - injected_power_w
```

On the committed designs `uninjected_power_w` should be zero because all
DEF components attach to both rails; nonzero is diagnostic, not hidden.
The explicit-current API has no `cell_power_w` context, so it reports
`uninjected_power_w = 0`; the cell-power path computes the real value.

## Residual

The reported residual is on the reduced free-node system:

```text
residual_inf     = max_i |(A*x - b)_i|          [A]
residual_rel_inf = residual_inf /
                   (||A||_inf * ||x||_inf + ||b||_inf)
```

The absolute value is the acceptance's `||GV - I||∞`; the normalized
value is the interpretability check for the ≥6-significant-digit
requirement. Implementation computes both after solving from the same
accumulated `A`/`b`, not from a second symbolic path.

## Numeric precision

Mesh resistances remain `f32` at input but conductance, RHS, voltages,
and summaries are `f64`. This is deliberate: drops are ~1e-4 V on a
1.8 V clamp, and a six-significant-digit voltage requirement is too
close to `f32` roundoff. Source values stay bit-exact because they are
assigned, not solved.

## Error surface

The public solve returns an error on:

- missing/unsupported `PdnUse` (`Power`/`Ground` only);
- `mesh.net != net.name` where a `PdnNet` is passed;
- non-empty `mesh.validate()`;
- power vector length not equal to `db.num_cells()`;
- non-finite or negative power/current;
- non-finite `source_voltage_v` or `current_scale_v <= 0`;
- sink pin with no owning cell;
- CSC/factorization failure.

These are input/system-boundary checks; no defensive checks are added
inside stamping loops.

## Files

- `crates/grus-core/src/pdn.rs`
- `crates/grus-core/src/pdn/solve.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-core/Cargo.toml`
- `Cargo.toml`
- `Cargo.lock`

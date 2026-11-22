# e8-solve — Sparse DC solve of the PDN mesh

E8 ticket 4 (after `e8-mesh-rc`). Solves `G·V = I` on each PDN mesh and
emits the per-instance voltage map in the reference CSV format.

## Scope

1. Current injection: per-instance power from our N7 `power` engine
   (the `grus-eda power` per-instance path — same Liberty model +
   activity inputs PDNSim uses internally; `convention_mirrored:
   embedded` means the reference's own estimator is the comparison
   point). `I_pin = P_inst / V_supply` at each sink node; VGND
   symmetric (current drawn from the ground net inverts the bounce).
2. Voltage-source nodes clamped to supply (`1.80 V` VPWR / `0 V` VGND —
   read the supply from the golden's `supply_voltage_v` so the number
   is never hand-typed; for VGND the reference's reported supply is
   whatever PDNSim reported — mirror it).
3. Sparse linear solve. Pick the lightest crate that fits (e.g.
   `nalgebra-sparse` CG/Cholesky, or a hand-rolled CG if the SPD
   structure is trivially exploitable) — new dependency is allowed per
   repo rules; justify the pick in the commit message. Stamped
   conductance matrix must be SPD-positive by construction (sources
   grounded/clamped); report solver residual `‖GV−I‖∞`.
4. Emit `*.irdrop.voltage.ours.csv` per net in the reference column
   order (`Instance,Terminal,Layer,X location,Y location,Voltage`)
   with the same row set as the golden CSVs (one row per attached
   instance terminal — sink order deterministic).
5. Scalar summary per net: worst drop/bounce, average drop/bounce,
   total current, solver iterations/residual — printed and stored for
   the e2e ticket's comparison.

## Non-goals

- No IR-drop-vs-STA timing feedback loop (voltage-scaled delays are
  beyond E8's done-when).
- No transistor-level grid modeling — wires are lumped resistors at
  node-center granularity, matching ticket-3's mesh.

## Files

- `crates/grus-core/src/pdn/solve.rs` or `src/pdn.rs` (G-assembly,
  solve, CSV emit)
- `crates/grus-eda` CLI surface only if the spec's own subcommand is
  cleanest (`irdrop` or `pdn` flag — implementer's call, keep it
  consistent with existing CLI style)
- `Cargo.toml` (solver dep) + `docs/results.md`

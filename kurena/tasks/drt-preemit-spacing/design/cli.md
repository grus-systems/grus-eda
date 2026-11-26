# CLI wiring — `--preemit-check`

## Flag

```rust
// DrtArgs, next to `self_drc`
#[arg(
    long,
    help = "Assemble the post-`apply_routes` shape set and run the \
            50-rule subset checker before the routed DEF is written"
)]
preemit_check: bool,
```

Opt-in, independent of `--self-drc`. `check_drt_ours.sh` does not pass
it → committed `cli_lines` goldens stay byte-identical; wiring it into
the e2e script (keep-list + golden regen) is a follow-up, not this
ticket.

## Compute position vs print position

**Compute between `apply_routes` and `write_routed_def`** — the
contract is that detection precedes the write:

```rust
let report = emit::apply_routes(&mut design_db, tech_db, &graphs, &route_db, &vias)?;
let preemit = args
    .preemit_check
    .then(|| emit::preemit_check(&design_db, tech_db))
    .transpose()?;
emit::write_routed_def(&design_db, tech_db, &args.def, &args.out_def)?;
```

**Print in the findings section, immediately before the `if
args.self_drc` block** — output stays grouped: emit census → reference
joins → `preemit_check` findings → `self_drc` findings → `wrote`.
Output order does not claim detection order; code order does.

## Shared printer

Extract the existing per-rule loop out of the `self_drc` block into a
file-local helper so both blocks print through the same code — "same
rule names/format" is then literal, not parallel:

```rust
fn print_subset_findings(tag: &str, subset: &SubsetReport) {
    let findings_total: usize = subset.rules.iter().map(|o| o.findings.len()).sum();
    println!("{tag} findings_total={findings_total}");
    for o in &subset.rules {
        if o.findings.is_empty() { continue; }
        println!("  rule={} family={} layer={} findings={}",
                 o.rule, drc_family_name(o.family),
                 o.layer.as_deref().unwrap_or("-"), o.findings.len());
        for f in &o.findings {
            println!("    {}", drc_finding_line(o.rule, f));
        }
    }
}
```

The `self_drc` block is re-pointed at it (its `scope:` line stays —
byte-identical self-drc output, verified by the parse test / a manual
diff).

## Output sample

```
preemit_check findings_total=1
  rule=met3.2 family=spacing layer=met3 findings=1
    met3.2 [0,84360,600,84960] x [601,84360,901,84960] gap=(1,0) bound=300 nets=clk,_00423_
preemit_check scope: report-only — 50-rule lef_expressible subset on the
in-memory emit shape set (same input --self-drc re-parses); findings do
not gate the write or the exit code until drt-selfdrc-exit lands
```

Clean run prints `preemit_check findings_total=0` + the scope line —
mirroring `self_drc`'s always-two-line shape (total + scope).

## Report-only + the exit-code gap

Findings never suppress `write_routed_def` and never change the exit
code in this ticket — `drt-selfdrc-exit` owns the findings→exit
contract (it will need one decision covering both `self_drc` and
`preemit_check` reports; coordinate there, not here). The scope line
states the gap in every run; `docs/results.md` repeats it.

## Interaction matrix

| flags | behavior |
|---|---|
| neither | unchanged — byte-identical output |
| `--preemit-check` | in-memory check pre-write; findings in findings section; write proceeds; exit 0 |
| `--self-drc` | unchanged — re-parse + `run_all` post-write |
| both | both blocks print (`preemit_check` first); the two reports should be identical — any delta is a writer/parse divergence to investigate |

## Arg-parse test

Extend the existing `drt argv parses` unit test: add `--preemit-check`
to the argv and `assert!(args.preemit_check)`.

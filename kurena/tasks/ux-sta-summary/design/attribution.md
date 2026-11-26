# ux-sta-summary — Domain attribution & histogram semantics

The rules below are the ones `docs/e9-reporting.md` records (the spec
asks for the multi-`-clock` port rule and the bucket bounds to be
documented). They are stated here as invariants of the *endpoint row*,
which is the granularity both new blocks share.

## 1. What a domain is

A domain is one `Sdc::clocks` row — one `create_clock` command, in file
order. Virtual rows (`port: None`) count: only port checks can anchor on
them, but they are still `create_clock` domains and get a row. Two rows
sharing a `-name` are two domains — grouping is by row index, never by
name.

`StaPrepared.clock_domain[pin]` is the per-pin capture identity the
check machinery already computes (the `Sdc::clocks` row whose port seed
reached the pin through the clock network, `u8::MAX` when none did).
`collect_endpoints` reads it as `cap` for register checks and `en_dom`
/`clock_domain[g.clk_pin]` for clock-gate enables; the port loop reads
`out_row` per surviving anchored `set_output_delay` entry.

## 2. Attribution rule

**Each endpoint is attributed to the `Sdc::clocks` row of its *reported*
check — the candidate `EndpointReport.max_slack_ns`/`min_slack_ns`
shows — preferring the max side.**

Mechanically: `Worst.dom` is stamped at construction; `endpoint_report`
picks `max`'s `dom` when a max candidate exists, else `min`'s. Per
endpoint kind:

| endpoint | `dom` |
|---|---|
| register/async check | `cap` = `clock_domain[check.clk]` — always `!= u8::MAX` for reported endpoints (`u8::MAX` skips `NoClock` first) |
| clock-gate enable | `clock_domain[g.clk_pin]` |
| output port | `out_row` of the anchored `set_output_delay` entry that produced the candidate — i.e. the **last-producing** anchor in eval order (min-corner entries then max-corner entries, file order each): the same last-group-wins rule the `EndpointReport`/`worst_*` comment documents (`t2_clkcor2`), applied per candidate |
| `set_max_delay`/`set_min_delay`-rewritten check | the anchor of the check it replaced — `..w` spreads inherit it; full literals set the in-scope `cap_row`/`out_row` |
| `-from get_pins` pin-segment check | same — keeps the replaced check's anchor (`..w`) |
| internal leaf (`dead_end_leaves`, `req = N`) | `u8::MAX` — no clock anything |
| unclocked capture (`t2_u3` family) | `u8::MAX` — falls out of `clock_domain` naturally |

Because `dom` rides the candidate, the partitioned compose needs no
lookup: the worst-of-class `Worst` that wins `composed[ep]` carries the
anchor that produced it, exact per class. The compose-time delay/MC
rewrites inherit via `..w` or take `cap_row`/`out_row`.

### Multi-`-clock` ports — the stated rule

A port can anchor on several `-clock`s producing one check per path
group; OpenSTA's endpoint table reports one row (the last group's) while
aggregates span them all. Our endpoint is likewise one row and lands in
**one** domain — the anchor of its reported (last-producing,
max-preferred) candidate. Its `worst_*` slacks, which fold every anchor's
check, travel with it into that domain's wns/tns/violation folds: an
approximation stated here and in `docs/e9-reporting.md`, not a hidden
one. Σ `endpoints` across domain rows still equals `endpoints.len()`.

### Unattributed (`clock: null`)

An endpoint whose *reported* candidate carries `dom == u8::MAX`:
internal delay-check leaves and unclocked captures. Skipped endpoints
(`skipped_endpoints`) never reach `endpoints` and are not counted
anywhere in these blocks — including `set_clock_groups`-deferred paths,
which are skips, not endpoints. The row is emitted only when at least
one endpoint is unattributed.

## 3. Domain folds

Identical to the globals, over the row's subset:

```text
wns_{max,min}        = min over members of worst_{max,min}_slack_ns   (∞ → null when empty)
tns_{max,min}        = Σ min(0, worst_{max,min}_slack_ns)
{setup,hold}_violations = count of worst_{max,min}_slack_ns < 0
```

Partition algebra the test asserts: `min(domains.wns_*)` = global wns,
`Σ tns` = global tns, `Σ violations` = global counts, `Σ endpoints` =
`endpoints.len()`.

## 4. Histogram

Bucket key: **`worst_max_slack_ns`** — the value that drives
`wns_max_ns`/`setup_violations`, so `negative.count ==
setup_violations` holds by construction. `worst_*` (not the serialized
`max_slack_ns`) is deliberate: on a multi-anchor port the two differ and
the aggregates follow `worst_*`; the doc notes JSON consumers see the
`max_slack_ns` approximation.

Fraction `s / P` where `P` = `domain_period_ns` (the *capture* clock's
period — the lattice the required time was computed on; a cross-domain
check is judged against its own capture period, not the launch's):

| bucket | bound | serialized name |
|---|---|---|
| negative | `s < 0` | `negative` |
| p0_to_20 | `0 <= s < 0.2·P` | `p0_to_20` |
| p20_to_50 | `0.2·P <= s <= 0.5·P` | `p20_to_50` |
| over_50 | `s > 0.5·P` | `over_50` |
| no_period | `domain_period_ns` is `None`, **or** `worst_max_slack_ns` is `None` (min-only endpoint — nothing to fraction) | `no_period` |

All five buckets always present, in the order above; `Σ count ==
endpoints.len()`. `f32` arithmetic throughout — `0.2 * P`/`0.5 * P`
computed in `f32` like the rest of the engine.

## 5. `merge_corner_reports` (`--lib-hold`)

The merged report's `domain_row` per endpoint:

- `setup.max_slack_ns.is_some()` → setup's `(domain_row,
  domain_period_ns)`. The merged max side is setup's reported check and
  its anchor.
- else → `h.domain_row.or(e.domain_row)` with the matching
  `domain_period_ns` — the merged min side is the hold run's reported
  check; setup's attribution is the fallback when hold's is
  unattributed.

Then `summarize_domains` runs on the merged endpoints with the clock
names of `setup.domains` (row order is the same SDC order — the merge
needs no `Sdc`). Corner-divergent multi-anchor attribution is the only
approximation: setup's row wins when its max check exists.

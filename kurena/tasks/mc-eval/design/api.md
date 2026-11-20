# mc-eval — Data model & pairing design

All changes live in `crates/grus-core/src/sta/analysis.rs` except the two
new `Sdc` accessors in `sdc.rs`. Single-domain behaviour is byte-identical
by construction: with one design row every tag is `0`, every mask is
`{0}`, and every `cap`/`anchor` read is the design row.

## 1. Domain identity

**A domain tag is a row index into `Sdc::clocks`** (`u8`). Design rows
(`port.is_some()`) are the only rows that can own a clock pin — they are
what `create_clock [get_ports ...]` reaches. `u8::MAX` = untagged.

```rust
/// Tag of the clock whose lattice a launch or capture lives on.
/// - design row r           -> r
/// - virtual row r          -> first design row with the same period_ns
///                             (edge pairing is identical) — the ibex
///                             `vclk_core_clock` case, arithmetically
///                             equal to today's first-row read
/// - virtual row, no match  -> r itself (never equals a capture tag, so
///                             any check it reaches defers)
fn launch_domain_tag(sdc: &Sdc, row: usize) -> u8;
```

`prepare_sta` bails loudly when `sdc.clocks.len() > 8`
(`sta: {n} clock rows; domain tags cover at most 8`) — the `dom` masks
below are `u8` bitsets, one bit per row.

## 2. Domain propagation — `clock_domain: Vec<u8>`

`ideal_clock_parity` becomes a `(parity, domain)` walk:

- Seed **every design row's** port pin: `par[port] = [0, 1]`,
  `dom[port] = row`.
- Same levelized walk as today; `dom` follows the same "first written
  wins per polarity" policy as `par` (ties keep the incumbent — a
  genuinely contested clock net is pathological and outside the
  committed fixtures; note in code comment).
- Return `(par: Vec<[u8;2]>, dom: Vec<u8>)`. Called unconditionally —
  cheap, and the domain map is mode-independent.

`prepare_sta` calls `ideal_clock_parity` unconditionally (today it
runs only in the ideal branch) and derives the parity oracle so the
current semantics are preserved exactly:

```rust
let (ideal_par, clock_domain) = ideal_clock_parity(cx, lv);
// any_propagated = any design row with `propagated` set.
let clock_parity = if any_propagated {
    // per pin/pol: propagated parity where it arrived, else ideal —
    // needed only in mixed mode; parity reads are gated on `reached`,
    // so the fallback is unobservable on the all-propagated path.
    per-pin merge of clk_par (<= 1 wins) over ideal_par
} else {
    ideal_par   // exact current value, ideal + pulse-oracle included
};
```

`StaPrepared` gains `clock_domain: Vec<u8>`; `clock_parity` becomes a
plain `Vec<[u8;2]>` (always computed — drop the `Option`).

## 3. `PinTimingState` domain masks

```rust
pub struct PinTimingState {
    // ... existing fields ...
    /// Bitmask of launch-domain tags tying the max arrival, per polarity.
    dom_max: [u8; 2],
    /// Same for min.
    dom_min: [u8; 2],
}
```

Same discipline as `par_*`/`crpr_*`: seed contributes its bit;
winner-replace sets the mask to the winner's; a tie unions the bits.
`merge()` gains two args (`dom_max`, `dom_min` after `crpr_min`);
`merge_all` unions them. `PinTimingState` fields stay crate-private —
no serde or external visibility.

Seed contributions in `eval_sta`/`prepare_sta`:

- `PortSeed { dom: u8 }` — `launch_domain_tag(sdc.input_clock_row(name))`;
  contributes `1 << dom` to `dom_*` when its launch survives the
  `allowed` filter, `0` when filtered out (same as the par contribution
  today).
- `LaunchSeed { dom: u8 }` — `clock_domain[l.clk]`; a launch whose tag
  is `u8::MAX` is **not emitted at all** (dead launch, same outcome as
  an unreached clk pin today). This matters beyond tidiness: a stray
  data arrival can reach an undeclared domain's CLK pin — e.g. a
  one-clock SDC on `two_clock.def` still seeds `clk2` as a data input —
  and without the gate that arrival would launch `u_fb0`'s Q and leak
  into `u_fy/D`. With the gate the whole undeclared cone dies upstream.
- Ideal-mode clock-pin seeds contribute `1 << dom` alongside the edge
  parity bit; propagated `clk_state` carries dom bits seeded at each
  design port inside `propagate_clock` (harmless — nothing downstream
  reads `state[clk].dom_*`; capture identity comes from `clock_domain`).

## 4. The same-domain rule and the deferral

One predicate decides both arrival evaluation and capture-side params:

```rust
/// Evaluated iff every launch tying this extreme is the capture
/// domain — a foreign bit, or an empty mask on a reached extreme, is a
/// pair mc-xdomain must form; evaluate nothing.
fn same_domain(dom_mask: u8, capture_tag: u8) -> bool {
    dom_mask == 1u8 << capture_tag
}
```

Applied in `collect_endpoints` per `(pol, corner)`:

- `capture_tag = clock_domain[check.clk]`; `cap = &sdc.clocks[tag]`.
- For each side (`max`/`min` × pol): evaluate only when
  `same_domain(s.dom_max[pol], tag)` / `same_domain(s.dom_min[pol], tag)`.
  Otherwise mark the side deferred.
- **Whole-endpoint rule**: if any side is deferred the endpoint is not
  reported (both `max` and `min` are needed for `endpoint_report`) →
  `skipped_endpoints.push(SkippedEndpoint { pin, reason: CrossDomain,
  related: "launch cka, capture ckb" })` and
  `skips.cross_domain_endpoints += 1`.
- The predicate applies only to sides that resolve to an **ordinary
  clock check**. A side covered by a `set_max_delay`/`set_min_delay`
  rule takes the delay-check path instead — its req anchor is the
  capture domain's edge-0 (§1.5), no edge pairing needed, so it
  evaluates per-domain even on a cross-domain endpoint. `t2_md`/`t2_mdl`
  defer anyway: `set_max_delay` covers only the max side and the hold
  side still can't be paired. A cross endpoint with *both* sides
  covered would report — correct, and not reachable in the committed
  fixture. `set_multicycle_path` does not exempt a side — it shifts an
  ordinary check's req, pairing still required.

`SkipReport`: new field `cross_domain_endpoints: usize`; included in
`nonzero()`, `endpoints_skipped()`, `is_clean()` (returns false — strict
runs fail loudly on unhandled domains), and `merge_skip_reports`.
New `SkipReason::CrossDomain`. The `related` string names the launch
domain set minus capture, e.g. `launch cka -> capture ckb` — used
verbatim in `docs/results.md`.

`run_sta_partitioned`'s skip pass re-derives the reason from the merged
state (per-run skips are discarded there): after the existing
unreached/`endpoint_skip_reason` checks, if any extreme's dom mask has a
bit ≠ `capture_tag` (or the mask is empty on a reached extreme) →
`CrossDomain`. Same for gating-enable and output-port endpoints (cap tag
= anchor fold, §6). A `dom` mask bit beyond the capture tag inside an
`allowed`-filtered class can't produce false positives: filtered seeds
contribute `0`, so a mask ≠ `{cap}` genuinely means a cross launch tied
an extreme.

## 5. Clock seeding — both walks per domain

`propagate_clock` seeds **every design row** (mode-independent — the
ideal/propagated distinction is applied at consumer seeding, and the
pulse-width oracle needs propagated arrivals on ideal domains too):

```rust
for (row, clock) in sdc.clocks.iter().enumerate() {
    let Some(port_name) = clock.port.as_deref() else { continue };
    let Some(port) = find_port_pin(port_name) else { continue };
    let lat = clock.source_latency_ns;
    let half = 0.5 * clock.period_ns;
    // seed rise at lat, fall at lat + half, par [0,1], dom bit = row,
    // slew = sdc.input_transition_ns(port_name)
}
```

The `if sdc.propagated_clock() / else` seeding block in `prepare_sta`
collapses into one consumer loop (the `clock_state` pulse oracle
still runs `propagate_clock` when `!pulse_width_checks.is_empty()`,
under any mode — same as today):

```rust
for pin in consumers {
    let dom = clock_domain[pin];
    if dom == u8::MAX { continue }                 // no declared domain
    let row = &sdc.clocks[dom];
    if row.propagated {
        let s = clk_state[pin];
        if s.reached(RISE) || s.reached(FALL) { base_state[pin] = s; }
        // unreached stays unreached -> NoClock downstream
    } else {
        // ideal seed: lat = row.latency_ns + row.source_latency_ns,
        // half = 0.5 * row.period_ns, slew = row.transition_ns
    }
}
```

`build_graph`'s `clock_net` marking seeds the clock-walk stack from
**every** design row's port net (not `sdc.clock_port()`).

`cell_driven_clock_nets` unifies into one `prepare_sta` loop (after
domain tags + optional propagation), replacing both current sites —
the `build_graph` count under `!propagated_clock()` (~1399) and the
unreached-net count in the propagated branch (~2443). Rule: count each
marked cell-driven clock net unless its driver pin is served by a
propagated domain — `count = !(dom_tag(driver) is a propagated row &&
clk_state[driver].reached)`.

- All-ideal: `dom` is never a propagated row → counts all — same total
  as the `build_graph` site today.
- All-propagated: the domain walk's reachability is a superset of
  propagation's (structural, no delay gating), so `MAX` ⇒ unreached;
  the rule reduces to "count unreached marked nets" — same total as
  the current prepare-side loop.
- Mixed: ideal-domain cell-driven nets count (not served by
  propagation); propagated-domain unreached nets count — the correct
  per-domain semantics the global flag couldn't express.

## 6. Per-domain reads — site table

`sdc.rs` additions (thin, public):

```rust
/// Resolved `set_input_delay -clock` row; absent/None -> first design row.
pub fn input_clock_row(&self, port: &str) -> usize;
/// Same for `set_output_delay`.
pub fn output_clock_row(&self, port: &str) -> usize;
```

Every `sdc.period_ns()`/`setup_uncertainty_ns()`/`hold_uncertainty_ns()`/
`propagated_clock()`/`clock_source_latency_ns()`/`design_clock_latency_ns()`
read in eval is replaced by a `cap`/anchor-row read. `cap` is always the
capture side; latency stays *inside* arrivals on the launch side
(measured §1.4). Sites:

| site | now | becomes |
|---|---|---|
| `propagate_clock` seeds | `sdc.clock_port()` + design row | per design row (§5) |
| `ideal_clock_parity` seed | single port | every design row's port |
| ideal consumer seeds | `design_clock_latency_ns()` + design row period/transition | `clocks[dom]` row |
| check req (`collect_endpoints`) | `period_ns`, `setup/hold_uncertainty_ns`, `propagated_clock()` | `cap.*` where `cap = clocks[clock_domain[clk]]` |
| `capture_extra` / latch `close` wrap | `period_ns` | `cap.period_ns` |
| latch borrow (`eval_sta`) | `period_ns`, `setup_uncertainty_ns` | `clocks[clock_domain[info.enable]].*` (the latch's own domain); `MAX` → skip — a stray data arrival can set `en.reached`, never index `clocks` on a `MAX` tag |
| opening-edge req (latch D check) | `period_ns`, `setup_uncertainty_ns` | `cap.*` |
| delay checks (`run_sta_partitioned`) | `period_ns`, `setup/hold_uncertainty_ns`, `propagated_clock()` | `cap.*` on `check.clk`'s tag; output-port `-to` checks use `clocks[output_clock_row(port)]` |
| MCP `d_max`/`d_min` | `period_ns` | `cap.period_ns` (Check: `clock_domain[clk]`; OutputPort: anchor row) |
| output-port req | `output_clock_latency_ns`, `period_ns`, uncertainties | `clocks[output_clock_row(port)].*` — the anchor row's own values (virtual rows included; `launch_domain_tag` only for the pairing test) |
| pulse width | `period_ns` | `clocks[clock_domain[check.pin]].period_ns` |
| unreached gates (`check.latch`, gating enables) | `sdc.propagated_clock()` | `cap.propagated`; `dom == u8::MAX` → `NoClock` before any `cap` read |
| `credit_for` | `sdc.propagated_clock()` | `cap.propagated` |
| `credit_for`/`crpr_credit` | unchanged — same-domain pairs only by construction | — |

`run_sta_detail`: the `design_clock_count() > 1` bail block is deleted;
`sta_bails_on_a_two_clock_sdc` is replaced (tests.md).

## 7. What changes for the recorded hazard

`docs/multi-clock.md` records the fold this ticket must not repeat: a
clock pin an undeclared domain feeds was previously launched on domain
0's lattice. Two structural fixes keep that impossible:

- `is_clock_port` (already row-wise since `mc-model`) keeps `clk2` out
  of data-input seeding — the port is never a data launch.
- `clock_domain` pins every clock-net vertex to the row that reaches
  it. Under a one-domain SDC on `two_clock.def`, `cb`-side pins carry
  `u8::MAX`: no ideal/propagated seed → D checks and the gate skip
  `NoClock` naming the pin; launches whose clk pin is `MAX` are never
  emitted (§3), so `u_fy/D` and `yb` get no arrival at all. `clk2` is
  still a data input — it is genuinely undeclared — but its stray
  arrival on `cb`-side CLK pins no longer folds anything onto domain
  0: `cap` is always `clock_domain[check.clk]`, and a `u8::MAX` tag
  short-circuits before any row read.

## 8. Signatures touched

- `PinTimingState::merge(&mut self, dst_pol, arrival, delay, slew, par,
  crpr_tag, par_min, crpr_min, dom_max, dom_min)` — `src.dom_*` threaded
  at every `merge`/`merge_all` site in `propagate_clock`, `eval_sta`,
  `prepare_sta` port seeds, `eval_launch` pairs (`LaunchPair` merges
  contribute `1 << seed.dom`).
- `ideal_clock_parity(...) -> (Vec<[u8;2]>, Vec<u8>)`.
- `propagate_clock(...)` — signature unchanged; internals seed per row.
- `collect_endpoints(..., clock_domain: &[u8], ...)` — new param.
- `eval_pulse_width_checks` — reads `clock_domain` via prepared.
- `LaunchSeed`, `PortSeed`, `StaPrepared` field additions (§3).
- `SkipReport::cross_domain_endpoints`, `SkipReason::CrossDomain`,
  `merge_skip_reports`, `nonzero`/`endpoints_skipped`/`is_clean`.

No changes to `sta.rs` output types, `main.rs`, `power.rs`, `Context`,
or the `EndpointPolarities`/`resolve_*` machinery — deferral is upstream
of `record_endpoint`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`

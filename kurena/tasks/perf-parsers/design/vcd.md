# perf-parsers — `readers/vcd.rs`

`parse_vcd` (lines 167–321) walks 4.6M body lines on gcd.vcd (18.8 MB,
~4.5M scalar `0!`/`1"` changes, ~31k `b…` vector writes, ~600 `x`).
Per scalar change the current code does ~3 heap allocations:
`activity.entry(id.to_string())` (line 556), `decl.name.clone()` +
`signals.entry(name.to_string())` inside `record_edge` (565, 512–524);
per vector line additionally a `Vec<u8>` (588–599) and a `format!` per
bit (624). All of it is removable without touching semantics.

## V1 — `activity` access without `id.to_string()` per line

`apply_change` (556) and `apply_vector_change` (604):
`activity.entry(id.to_string()).or_default()` allocates and hashes
every line. Declared ids already hold an `IdActivity` (inserted in
`parse_var_decl`, 417–420), so `get_mut(id)` hits essentially always;
keep the `or_insert_with` fallback to preserve `or_default` +
`if act.last.is_empty() { *act = with_bits(..) }` behavior bit-for-bit
(including the `act.last.len() != decl.nbits` re-init at 605–607).

## V2 — `record_edge` without per-edge `String`s (512–524)

Signature `(name: &str, …)` already borrows; the cost is
`signals.entry(name.to_string())` per real edge. `get_mut(name)` fast
path, `entry(name.to_string())` only on first sight — identical
entries. `apply_change` then passes `&decl.name` instead of
`decl.name.clone()` (565): **zero** allocs on the scalar hot path.

## V3 — Precomputed per-bit names for the vector path (624)

`format!("{}[{}]", decl.name, bit_idx)` allocates per bit per change.
`SignalDecl` gains `bit_names: Vec<String>` built once in
`parse_var_decl` — scalar: `[name]`; vector: `name[left∓pos]` strings
with the **same** asc/desc index rule used at 258–266 and 296–304 —
and `apply_vector_change` passes `&decl.bit_names[pos]`. The closing
passes (251–277, 279–309) keep their own `format!`s (cold) or reuse
`bit_names`; the produced `signals`/`pin_signals` keys must be
byte-identical — same expansion rule, same order.

## V4 — No `Vec<u8>` per vector line (588–599)

`bits.len() < nbits` pads on the left (pad char from `bits[0]`:
`x`/`z`/`0`); `> nbits` returns early. Iterate positions directly:

```rust
if bits.len() > decl.nbits { return; }
let pad_len = decl.nbits - bits.len();
// pos < pad_len → pad byte; else bits.as_bytes()[pos - pad_len]
```

Same pad semantics and same early-return; the
`chars[0]-is-leftmost` index rule (610–616) is unchanged.

## V5 — xxh3 on the hot maps, public field types unchanged

- Private maps → `FastMap` (`Xxh3DefaultBuilder` alias as in
  `def.md` §D3): `decls` and `activity` (`VcdHeaders`, 155–163) are
  probed once per change line on 1–3-byte ids — the largest single
  win after de-allocating.
- `signals`: built hot but is a **public field** of `VcdActivity`
  (`pub signals: HashMap<String, SignalActivity>`) — `saif.rs`
  constructs the struct literally with a default-hasher map
  (`saif.rs` ~449–553). Keep the declared type: accumulate into a
  `FastMap<String, SignalActivity>` during the body scan, then
  `signals: fast.into_iter().collect()` once at the end (~1.5k
  entries — free). Same for `pin_signals` only if it turns hot in
  profile (it is a closing-pass map — leave default).

## V6 — Byte-level dispatch (479–502)

`parse_scalar_change` via `chars().next()`/`chars.as_str()` and
`id.contains(char::is_whitespace)` → byte checks: value byte in
`b"01xXzZ"`, remainder non-empty and free of ASCII whitespace.
`parse_vector_change` `find(char::is_whitespace)` → byte scan.
Identical accept/reject on VCD's ASCII alphabet; non-ASCII bytes were
never valid value/id content and still fail the same way.

`#`-timestamps keep `parse::<u64>` (cold relative to changes);
`line.trim()`/`text.lines()` stay (`&str` slicing, no alloc).

## Cold paths untouched

`parse_timescale`, `pending`/`handle_header_item`, `parse_var_decl`
(apart from building `bit_names`), `split_name_range`, the two
closing passes, `pin_signals` emit. Header cost is ~declarations, not
lines.

## Error-path checklist

VCD rejects are not line-numbered but stay byte-identical in text:
missing/bad `$timescale`, malformed `$var`, empty `$var` name. Body
semantics preserved: unknown ids ignored, `r<real>`/malformed lines
ignored, scalar-on-vector-id and vector-on-scalar-id dropped,
short-vector pad rule, `$`-commands in body skipped.

## Expected

~479–860 → ~150–300 ms `power.activity.load` (×2–3): removes ~3 allocs
and 2 SipHash lookups on every one of ~4.5M lines. Residuals: file
read + UTF-8 validate (~few ms), the `FastMap→HashMap` conversion and
closing passes, `dur`/`sa` per-bit Vec traffic (structure, not
scanning).

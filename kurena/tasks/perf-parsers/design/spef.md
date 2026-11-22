# perf-parsers — `readers/spef.rs`

`parse_spef` (lines 55–218) walks `text.lines()`; each line is split into
a fresh `Vec<&str>` (line 89) and each `*CAP`/`*RES` entry resolves one to
two node names through `resolve` (lines 72–86). On jpeg.routed.spef
(~730k entry lines, 36.9 MB) that is ~0.7M `Vec` allocations plus 2–3
`String` allocations per resolved name.

## S1 — Drop the per-line token `Vec` (line 89)

`let tokens: Vec<&str> = line.split_whitespace().collect()` → drive the
match arms off an iterator or a reused scratch buffer. Every arm only
needs indexed access to ≤4 tokens plus a length check; keep the exact
`tokens.len()` predicates — they are the field-count error paths:

- `*C_UNIT`/`*R_UNIT`/`*T_UNIT`: `len() < 3` → "needs a value and a unit".
- `*D_NET`: `len() < 3` → "malformed *D_NET".
- NAME_MAP entry: `len() < 2` → "has no name".
- `Section::Cap`: `len() == 3 | == 4` else "*CAP entry must have 3 or 4
  fields"; `Section::Res`: `== 4` else "*RES entry must have 4 fields".

Cheapest faithful shape: fill a `&mut [&str; 5]` (or a reused
`Vec<&str>` scratch cleared per line) from `split_ascii_whitespace`,
take `n = filled` as the length, and index `t[0..n]` exactly as the
current `tokens[..]` — the `match first` dispatch and all error bails
keep their shape. A line with >4 tokens still counts correctly (fill
past the array only to count, or take `n > 4` from the iterator).

## S2 — `resolve` without per-lookup `String`s (lines 72–86)

Two allocations per mapped token today: `format!("*{index}")` for the map
probe, then `name.clone()` / `format!("{name}:{s}")`.

- Rekey `name_map` `HashMap<String, String>` (line 65, insert at 148) to
  `HashMap<u32, String>` keyed by the `*N` index digits — same digits are
  already validated by `is_name_map_index` (lines 41–45). On resolve,
  `*12:3` splits at `:`; the index part parses to `u32` only when it is
  all digits — a non-numeric `*x` token must fall through to the
  unescape path **exactly as today** (today's `map.get` simply misses).
  Keep `is_name_map_index`'s semantics for the *section* test
  (`*PORTS`/`*I` are not indices).
- `unescape_spef_name` (`String`, line 24, also used by
  `sta/parasitics.rs` — signature stays) gets a private `Cow` sibling:
  `Cow::Borrowed` when the token has no `\`. `resolve` returns
  `Cow<str>`; the storage sites (`net.caps`/`net.resistors`/the
  `current` tuple) call `.into_owned()` — one allocation per stored
  name, down from 2–3. A `*N:suffix` hit still builds one `String`
  (`format!` or `name + ":" + s` into a scratch then owned — identical
  result).

## S3 — `split_whitespace` → `split_ascii_whitespace`

Whitespace in IEEE 1481 is ASCII space/tab; `split_ascii_whitespace`
skips the Unicode table per byte. Deliberate note: inside a *token*,
non-ASCII whitespace now stays part of the word instead of splitting —
on such input the line fails later (numeric parse or field count) at
the same line number, never silently. All committed SPEF and every
error test are ASCII. Record the note in `docs/results.md`.

## S4 — `f32` fields via `fast-float`

`tokens[2].parse::<f32>()` per `*D_NET`/`*CAP`/`*RES` line (~0.7M calls
on jpeg). Add `fast-float` (mature, permissively licensed; justify in
the commit message per AGENTS §1): `fast_float::parse::<f32, _>(tok)`.
`f32::from_str` and `fast_float` agree on every SPEF literal shape
(sign, digits, `.`, `e±`); both accept `inf`/`nan` spellings, so no
acceptance change on anything the format or tests produce. The
`unit_scale` parse (line 29) can share it — cold anyway.

## S5 — Hot maps on `xxh3`

`xxhash-rust` is already a `grus-core` dep (`xxh3::Xxh3DefaultBuilder`
implements `BuildHasher`); alias per file:
`type FastMap<K, V> = HashMap<K, V, Xxh3DefaultBuilder>;`

- `owner: HashMap<&str, usize>` (lines 188–199): every resistor endpoint
  and cap first-node hashed up to 3× — the largest map in the file.
- `name_map` (S2) — small; same alias for free.
- `spef.nets` (lines 213–216): the field is `pub nets: HashMap<String,
  SpefNet>` with default hasher; keep the public type — build it as
  `FastMap` internally only if useful, or just leave `nets.insert` as
  is (one insert per net, ~8k nets — cold).

`ended`/`halves` (lines 200–208): one `String` clone per coupling cap
second node is inherent to storing owned names; leave.

## S6 — Error-path checklist (all preserved verbatim)

Line numbering: `for (line_no, line) in text.lines().enumerate()` and
`at()` (line 93) are untouched. Preserved rejects: `*X_UNIT` arity +
bad value + unsupported unit; `*T_UNIT != 1 NS`; malformed `*D_NET`;
NAME_MAP entry without a name; `*CAP`/`*RES` field counts; `*D_NET`
without `*END`; number-parse failures with `SPEF line N` context; the
every-truncation test (lines 365–374).

## Expected

297 → ~100–150 ms jpeg `sta.spef.read` (×2–3): the Vec + String churn
and SipHash lookups were most of the 118 MB/s ceiling; float parse is
the residual candidate if the first pass under-delivers — re-measure
with `sample` before considering more.

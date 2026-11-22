# perf-parsers — `readers/def.rs`

`parse_def_bytes` (lines 571–861) lexes byte-level already; the costs
are `Vec`/`String` churn per statement and per `( comp pin )` group, and
SipHash name maps on the NETS connection path. jpeg.placed.def is
9.8 MB (61,569 components, ~125k `( comp pin )` groups across 46,568
nets); `cong.load` and `sta.load.def` both wrap this function.

## D1 — No `Vec` per group/statement

- **NETS `( comp pin )` groups** (lines 760–797): today each group goes
  through `read_group` → a fresh `Vec<&str>` per connection (~125k on
  jpeg). Replace with an inline `read_conn_group` that consumes exactly
  `Word(comp) Word(pin) RParen` and reproduces **both** existing error
  shapes verbatim:
  - non-`Word` where a name is expected, or `Semi`/`LParen`/`Eof` where
    `RParen` is expected → `bail!("DEF line {}: unexpected {other:?}
    inside `( ... )`")` — identical message to `read_group` (line
    179–183).
  - a third `Word` inside the parens (`( a b c )`) → `bail!("DEF line
    {line}: connection of net {net_name} must be `( component pin )`")`
    — identical to the `[comp, pin]` pattern fail (line 763–766). The
    bail terminates the parse either way, so not consuming the rest of
    the group is equivalent.
- **Statement buffers in counted sections**: `for_each_record`
  (lines 259–293) calls `read_statement` (line 157) per `-` record.
  Add `read_statement_into(lexer, &mut Vec<&'a str>)` (same loop, fills
  a caller buffer) and reuse one scratch `Vec` for the whole
  COMPONENTS/PINS/SPECIALNETS section; `entry(&words, line)` signature
  is unchanged. Top-level one-shot statements (ROW/TRACKS/DIEAREA/…)
  keep `read_statement` — cold.
- `read_group` itself stays for the `in_attributes` arm (line 758) —
  cold path (routed-wire groups inside net `+` attributes).

## D2 — `Cow` unescape on the name paths

`unescape_def_name` (line 96) allocates a `String` even when the name
has no `\`. Add `fn unescape_def_name_cow(&str) -> Cow<str>` sharing the
same loop (`Cow::Borrowed` fast path); keep the `pub fn` returning
`String` (pub API, one-line wrapper is fine). Uses:

- `add_component` inst name (line 306) — still owned for the map
  insert, but `db.strings.intern`/`cell_name_to_id.insert` see the Cow.
- `add_pin` pin name (line 411).
- NETS: `net_name` (line 739 — owned anyway), and per connection
  `pname` (769) / `iname` (776): today these allocate **only to feed a
  `get`** — `io_pin_name_to_id.get(pname.as_ref())` /
  `cell_name_to_id.get(iname.as_ref())` borrow as `&str`, so the common
  escape-free case costs zero allocs per connection.
- Error messages that interpolate `{pname}`/`{iname}` format the Cow —
  identical text.

## D3 — xxh3 on the name→id maps

`type FastMap<K, V> = HashMap<K, V, xxhash_rust::xxh3::Xxh3DefaultBuilder>`
(existing dep): `cell_name_to_id` (line 582) and `io_pin_name_to_id`
(583). Both are probed once per connection; keys are already-owned or
`&str` — `FastMap<String, CellId>` etc. `db.strings.intern` keeps
SipHash (interner is out of file scope; named residual).

## D4 — `split_attributes` without the `Vec` (lines 202–220)

Called once per COMPONENTS/PINS record. Re-express as a small iterator
(`AttrIter` yielding `Result<(&str, &[&str])>`) over the word slice —
same split rule (args run to the next `+`), same two bails: "expected
`+` before `{first}`" and "`+` without an attribute keyword". Call sites
in `add_component`/`add_pin` iterate instead of collecting.

## D5 — SPECIALNETS coordinate probes (lines 511–514)

`words[i].parse::<i32>().is_ok()` is evaluated up to 4× and then
`number(...)` parses the same token again on success. Bind
`Option<i32>` once per word (`"*" => None`, else `parse`) and reuse —
identical accept/reject.

## What is deliberately not changed

- `DefLexer::next_token` (lines 42–93): already byte-level, comment/escape
  handling stays; an optional 256-entry delimiter lookup is allowed only
  if a profile says the word loop is the residual.
- `number::<T>` (line 187): `FromStr` int parse on `&str` — no
  allocation to remove; a hand-rolled parser must reproduce
  `i32::from_str` exactly (`[+-]?[0-9]+`, overflow → err) — keep std.
- Per-cell `lib_pin` linear scan in `add_component` (lines 372–383) and
  per-connection `cell_pins` pin-name match (784–788): O(few) each —
  named residual, not this ticket.
- `File::read_to_end` + `from_utf8` (lines 864–868): ~ms on 9.8 MB —
  residual (mmap precedent noted in `overview.md`).

## Error-path checklist

`lexer.line()` accounting is untouched (comments/`\n` counting in the
lexer is unchanged). Preserved rejects: truncated statement "not
terminated by `;`"; section `END` mismatches; count mismatches
(`{declared} records but {found} were read`); unknown section/attribute
bails; `( component pin )` shape (D1); ROW/TRACKS/GCELLGRID shapes;
`PIN`/component-resolution `with_context` messages; missing
`END DESIGN`; the every-truncation test (~line 1741).

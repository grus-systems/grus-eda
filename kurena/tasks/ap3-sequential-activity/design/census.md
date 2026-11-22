# census — sky130 `ff`/`latch` `next_state`/`data_in` coverage

Offline scan of
`$PDK_ROOT/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib`
(`PDK_ROOT` env var, default `~/.ciel` — same fallback as `measure.sh`)
(2026-11-22, **cell-level** `ff`/`latch` groups only — 8-space indent;
24 `test_cell` groups carry their own `ff` copies (`next_state : "D"`,
functional-mode view) and are excluded here exactly as the parser
skips them whole). This table is the data behind the `docs/results.md`
census the acceptance item requires.

## `ff` blocks — 45 cells

| `next_state` | cells | form | covered |
|---|---|---|---|
| `"D"` | 19 | bare pin name | **yes** — `dfbbn`, `dfbbp`, `dfrbp`, `dfrtn`, `dfrtp`, `dfsbp`, `dfstp`, `dfxbp`, `dfxtp` (incl. `clear`/`preset`-carrying `dfr*`, negative-edge `*tn`, both-output `*bp`) |
| `"(D&!SCE) | (SCD&SCE)"` | 19 | scan mux | no — `sdfbbn/p`, `sdfrbp`, `sdfrtn/p`, `sdfsbp`, `sdfstp`, `sdfxbp`, `sdfxtp` |
| `"(D&DE&!SCE) | (IQ&!DE&!SCE) | (SCD&SCE)"` | 5 | scan + enable mux | no — `sedfxbp`, `sedfxtp` |
| `"(D&DE) | (IQ&!DE)"` | 2 | data-enable mux | no — `edfxbp`, `edfxtp` |

Note: `dfrtp`/`dfrbp`-style cells carry `clear : "!RESET_B"` *alongside*
a bare `"D"` — they parse as covered; the async pin is ignored by the
copy rule (unmeasured — recorded).

## `latch` blocks — 18 cells

All 18 carry `data_in : "D"` — **all covered** (`dlrbn/p`, `dlrtn/p`,
`dlxbn/p`, `dlxtn/p`, `lpflow_inputisolatch`). `enable` is `!GATE_N`
(10), `GATE` (7), `SLEEP_B` (1, retention latch); 6 cells also carry
`statetable` groups (still unparsed). The Q=D copy applies to latches
by the same code path — unmeasured for latches specifically, recorded.

## Output polarity census

Every sequential output pin's `function` is exactly one of the state
vars: `Q → "IQ"`, `Q_N → "IQ_N"`. Cells with both outputs: `dfbbn`,
`dfbbp`, `dfrbp`, `dfsbp`, `dfxbp`, `dlrbn`, `dlrbp`, `dlxbn`, `dlxbp`,
plus the residual `sdf*bp`/`sedf*bp`/`edf*bp`. No third form exists in
the lib — an unmatched function is unclassifiable, not a silent guess.

## Coverage summary

- `next_state`/`data_in` bare-name coverage: **37 / 63** sequential
  cells (19/45 ff + 18/18 latch).
- Residual (expression `next_state`): **26 / 63** — all `sdf*`, `edf*`,
  `sedf*` scan/enable-mux cells. Their output nets resolve `Default` →
  `unknown {0,0}` — an honest placeholder, not a measured copy of the
  mux logic (OpenSTA's choice for a scan-mux D is unmeasured on the
  committed fixtures; mba uses only `dfxtp`).
- The parser's `is_sequential`/`is_latch` flags see the same 63 cells
  (cell-level groups only — `test_cell` is skipped whole).

## Reproduce

```bash
python3 - <<'EOF'
import os
import re
from collections import Counter
pdk = os.environ.get('PDK_ROOT', os.path.expanduser('~/.ciel'))
lib = open(os.path.join(pdk, 'sky130A/libs.ref/sky130_fd_sc_hd/lib/'
                        'sky130_fd_sc_hd__tt_025C_1v80.lib')).read()
cells = re.findall(r'(?ms)^    cell \("([^"]+)"\) \{(.*?)\n    \}\n', lib)
ff, la = Counter(), Counter()
for name, body in cells:
    for m in re.finditer(
            r'(?ms)^        ff \("[^"]*","[^"]*"\) \{(.*?)\n        \}', body):
        ns = re.search(r'next_state\s*:\s*"([^"]*)"', m.group(1))
        ff[ns.group(1) if ns else '<none>'] += 1
    for m in re.finditer(
            r'(?ms)^        latch \("[^"]*","[^"]*"\) \{(.*?)\n        \}', body):
        di = re.search(r'data_in\s*:\s*"([^"]*)"', m.group(1))
        la[di.group(1) if di else '<none>'] += 1
print(ff.most_common(), la.most_common())
EOF
```

(The `^        ` indent anchors are what exclude `test_cell` bodies —
the script reads only the committed PDK file.)

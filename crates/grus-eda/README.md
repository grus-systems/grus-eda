# grus-eda

Fast physical-design research toolkit for the open SkyWater sky130 PDK, in one
Rust binary.

```
cargo install grus-eda
```

Answers architectural-exploration questions — timing, placement density,
routing congestion, power, IR drop — against the established open-source tools
(OpenSTA, OpenROAD, Magic, PDNSim) as the sign-off reference at standard file
boundaries (Liberty, LEF, DEF, SPEF, SDC). Includes research implementations of
global routing, clock-tree synthesis, detailed routing, and a LEF-expressible
DRC subset; residuals are reported rather than hidden.

Every stage counts as done only when its numbers agree with the reference tool
on the same public test designs; the verification log is the repository's
`docs/results.md`. Research prototype: sign-off, GDS output, and LVS are out
of scope.

- Repository: <https://github.com/grus-systems/grus-eda>
- Homepage: <https://eda.grus.systems>

License: Apache-2.0

//! Detailed-routing substrate: `tracks` merges the LEF `TRACKS` and
//! DEF `TRACKS` grids into per-layer, per-axis DBU coordinate lists and
//! names preferred-axis gaps; `access` enumerates the candidate
//! attachment points of every signal-net member pin on that grid.

pub mod access;
pub mod tracks;

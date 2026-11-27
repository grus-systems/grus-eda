//! Detailed-routing substrate: `tracks` merges the LEF `TRACKS` and
//! DEF `TRACKS` grids into per-layer, per-axis DBU coordinate lists and
//! names preferred-axis gaps; `access` enumerates the candidate
//! attachment points of every signal-net member pin on that grid;
//! `graph` turns per-net guide boxes into the guide-bounded
//! track-crossing graph the maze search walks; `via` enumerates the
//! adjacent-pair candidate defs and names each pair's default pick;
//! `maze` routes each guided net inside its region by sequential tree
//! extension under interval occupancy, and names whatever it cannot
//! connect; `emit` turns committed routes into routed `NETS` DEF
//! geometry the rest of the toolchain already reads.

pub mod access;
pub mod emit;
pub mod graph;
pub mod maze;
pub(crate) mod pool;
pub mod tracks;
pub mod via;

pub use maze::{
    AreaResidual, Collision, FailKind, IterStat, MazeConfig, NetRoute, OccMode, RouteDb,
    RouteProgress, UnroutedNet, ViaPlace, WireSeg,
};

//! Detailed-routing substrate: `tracks` merges the LEF `TRACKS` and
//! DEF `TRACKS` grids into per-layer, per-axis DBU coordinate lists and
//! names preferred-axis gaps; `access` enumerates the candidate
//! attachment points of every signal-net member pin on that grid;
//! `graph` turns per-net guide boxes into the guide-bounded
//! track-crossing graph the maze search walks; `via` enumerates the
//! adjacent-pair candidate defs and names each pair's default pick;
//! `maze` routes each guided net inside its region by sequential tree
//! extension under interval occupancy, and names whatever it cannot
//! connect.

pub mod access;
pub mod graph;
pub mod maze;
pub mod tracks;
pub mod via;

pub use maze::{
    Collision, FailKind, MazeConfig, NetRoute, OccMode, RouteDb, UnroutedNet, ViaPlace, WireSeg,
};

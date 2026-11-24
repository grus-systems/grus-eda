//! Routed DEF emission: `RouteDb` → `db.net_geoms` → a canonical
//! `NETS` section spliced into the placed-DEF template. The emitted
//! grammar is exactly what `parse_net_wire` reads back — regular-net
//! wires carry no width token (width is a `SPECIALNETS`-only
//! construct; a bare integer after the layer parses as a point
//! coordinate), a standalone via record is `NEW <lower> ( x y )
//! <via_name>` naming the `ViaTable` pair default, and a non-square-cap
//! rect (a self-spacing fill) is `<layer> ( x y ) RECT ( 0 0 w h )`
//! — deltas from the point, which the parser reads back into
//! `wire_rects` unchanged. See `docs/e6-drt.md` §8 for the model and
//! the claim boundary.

use crate::congestion::guide::guide_net_name;
use crate::design_db::{DesignDb, NetGeom, PdnVia};
use crate::drt::graph::RouteGraphs;
use crate::drt::maze::{RouteDb, ViaPlace, WireSeg};
use crate::drt::via::ViaTable;
use crate::interner::{LayerId, NetId};
use crate::readers::def::escape_def_name;
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

/// What `apply_routes` did — printed by the CLI and asserted in tests.
#[derive(Clone, Debug, Default)]
pub struct EmitReport {
    /// Nets whose `NetRoute` was applied (`route_db.nets.len()`).
    pub routed: usize,
    /// DEF spellings of guided nets in `route_db.unrouted`.
    pub unrouted: Vec<String>,
    /// Σ |x2−x1| + |y2−y1| over all emitted `WireSeg`s, DBU.
    /// Centerline length — matches `routed_def_parse`'s per-segment
    /// accounting, so the delta vs `total_wirelength_um` is honest.
    pub wire_length_dbu: u64,
    /// Cut-layer name → placed-via count, grouped by
    /// `pairs[vp.lower].cut` resolved via `tech.strings` — joins
    /// `vias_per_cut_layer` in `<d>.drt.reference.json`.
    pub vias_per_cut: BTreeMap<String, u64>,
}

/// A `WireSeg` expanded to the square-cap rect `parse_net_wire`
/// derives for a two-point record — the same rect `commit_route`
/// rasterizes into occupancy, so the emitted DEF describes exactly
/// the shapes the router priced.
fn seg_rect(w: &WireSeg, tech: &TechDb) -> RectDbu {
    let w2 = tech.routing_layers[w.layer.0 as usize].width_dbu / 2;
    RectDbu::new(
        w.x1.min(w.x2) - w2,
        w.y1.min(w.y2) - w2,
        w.x1.max(w.x2) + w2,
        w.y1.max(w.y2) + w2,
    )
}

/// Convert `RouteDb` results into `db.net_geoms` entries.
///
/// Every `NetRoute` in `route_db.nets` becomes a `NetGeom` of
/// `± w/2` wire rects plus `PdnVia`s built from
/// `pairs[vp.lower].default` — the emitter never reselects a via
/// (the placed shape must be the one the router priced). Every
/// `route_db.unrouted` net is reset to an empty `NetGeom` (conns-only
/// record — never stale geometry). `db.net_geoms` is grown to
/// `num_nets` first — `parse_def` fills it on the DEF-input path but
/// the field is not guaranteed populated otherwise — and only the
/// routed/unrouted entries are overwritten, so unguided nets keep
/// their geometry verbatim.
///
/// A guided net absent from both lists means graphs/route_db
/// disagree — bail. Unguided nets keep whatever geometry they had
/// (a placed template carries none; a routed template's records
/// pass through as foreign geometry).
///
/// Errors (bail, not silent): a `ViaPlace` whose pair has
/// `default == None` (a via-table gap reached emit — the router
/// skips gap transitions, so this is a bug), or `vp.lower !=
/// cand.lower` (table consistency).
pub fn apply_routes(
    db: &mut DesignDb,
    tech: &TechDb,
    graphs: &RouteGraphs,
    route_db: &RouteDb,
    via_table: &ViaTable,
) -> Result<EmitReport> {
    if db.net_geoms.len() < db.num_nets() {
        db.net_geoms.resize_with(db.num_nets(), NetGeom::default);
    }

    let mut report = EmitReport {
        routed: route_db.nets.len(),
        ..Default::default()
    };
    let mut routed: BTreeSet<u32> = BTreeSet::new();
    for nr in &route_db.nets {
        routed.insert(nr.net.0);
        let mut geom = NetGeom::default();
        for w in &nr.wires {
            report.wire_length_dbu += (w.x2 - w.x1 + w.y2 - w.y1) as u64;
            geom.wire_rects.push((w.layer, seg_rect(w, tech)));
        }
        // Self-spacing fills are committed paint — a `wire_rects`
        // entry each (they emit as `RECT` path records, not 2-point
        // wires, so `wire_length_dbu` stays centerline-honest).
        for (lid, r) in &nr.fills {
            geom.wire_rects.push((*lid, *r));
        }
        for vp in &nr.vias {
            let via = place_via(db, tech, via_table, vp, nr.net)?;
            let cut_name = via_table.pairs[vp.lower.0 as usize]
                .cut
                .map(|c| tech.strings.resolve(c).to_string())
                .unwrap_or_else(|| db.strings.resolve(via.cut).to_string());
            *report.vias_per_cut.entry(cut_name).or_default() += 1;
            geom.vias.push(via);
        }
        db.net_geoms[nr.net.0 as usize] = geom;
    }

    let mut unrouted_set: BTreeSet<u32> = BTreeSet::new();
    for u in &route_db.unrouted {
        unrouted_set.insert(u.net.0);
        db.net_geoms[u.net.0 as usize] = NetGeom::default();
        report.unrouted.push(guide_net_name(db, u.net));
    }

    for g in &graphs.nets {
        if !routed.contains(&g.net.0) && !unrouted_set.contains(&g.net.0) {
            bail!(
                "guided net {} is absent from both route_db.nets and route_db.unrouted — graphs/route_db disagree",
                guide_net_name(db, g.net)
            );
        }
    }
    Ok(report)
}

/// The `PdnVia` a `ViaPlace` commits: the pair default's name and
/// geometry verbatim at `(vp.x, vp.y)` — emit reselects nothing.
/// Also counts the placement into `vias_per_cut` at the caller.
fn place_via(
    db: &DesignDb,
    tech: &TechDb,
    via_table: &ViaTable,
    vp: &ViaPlace,
    net: NetId,
) -> Result<PdnVia> {
    let k = vp.lower.0 as usize;
    let pair = via_table.pairs.get(k).with_context(|| {
        format!(
            "net {} via at ({}, {}) names lower layer {} — outside the adjacent-pair table",
            guide_net_name(db, net),
            vp.x,
            vp.y,
            vp.lower.0
        )
    })?;
    let Some(di) = pair.default else {
        let lname = |l: LayerId| {
            tech.strings
                .resolve(tech.routing_layers[l.0 as usize].name)
        };
        bail!(
            "net {} via at ({}, {}) on pair {}–{}, which has no default candidate — a ViaGap reached emit",
            guide_net_name(db, net),
            vp.x,
            vp.y,
            lname(pair.lower),
            lname(pair.upper),
        );
    };
    let cand = &via_table.candidates[pair.candidates[di as usize] as usize];
    if cand.lower != vp.lower {
        bail!(
            "net {} via at ({}, {}): pair default {} has lower {} != {}",
            guide_net_name(db, net),
            vp.x,
            vp.y,
            db.strings.resolve(cand.name),
            cand.lower.0,
            vp.lower.0,
        );
    }
    Ok(PdnVia {
        name: cand.name,
        lower: cand.lower,
        upper: cand.upper,
        cut: cand.cut,
        cut_count: cand.cut_count,
        geom: cand.geom.clone(),
        x_dbu: vp.x,
        y_dbu: vp.y,
    })
}

/// Byte offset where `section` (`"NETS "`, `"END NETS"`) starts at a
/// line boundary — a plain `find` would match `SPECIALNETS` for
/// `"NETS "`.
fn find_section(template: &str, section: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(pos) = template[from..].find(section) {
        let idx = from + pos;
        if idx == 0 || template.as_bytes()[idx - 1] == b'\n' {
            return Some(idx);
        }
        from = idx + 1;
    }
    None
}

/// Recovers the two-point centerline a `wire_rect` expands from, or
/// `None` when the rect is not square-cap-expressible (foreign
/// geometry — a `RECT` stub or flush cap from a routed template).
/// `w2` is `routing_layers[l].width_dbu / 2`, the parse side's `±`
/// factor.
fn centerline(lid: LayerId, r: &RectDbu, tech: &TechDb) -> Option<(i32, i32, i32, i32)> {
    let w2 = tech.routing_layers[lid.0 as usize].width_dbu / 2;
    if r.yl + w2 == r.yh - w2 && r.xl + w2 <= r.xh - w2 {
        // Horizontal wire or point: (xl+w2, y) – (xh−w2, y).
        Some((r.xl + w2, r.yl + w2, r.xh - w2, r.yh - w2))
    } else if r.xl + w2 == r.xh - w2 && r.yl + w2 <= r.yh - w2 {
        // Vertical wire: (x, yl+w2) – (x, yh−w2).
        Some((r.xl + w2, r.yl + w2, r.xh - w2, r.yh - w2))
    } else {
        None
    }
}

/// The `NETS` section text for `db` — `NETS <n> ;` plus one canonical
/// record per net in db order: name (`guide_net_name`), conns in
/// `net_pin_ids` order (`( comp pin )` / `( PIN name )`), then
/// geometry — exactly one `+ ROUTED` on the first statement, `NEW`
/// on the rest — and `+ USE SIGNAL ;`. Errors name the offending
/// net, matching the writer's no-silent-distortion contract.
fn nets_section(db: &DesignDb, tech: &TechDb) -> Result<String> {
    let layer_name = |l: LayerId| {
        db.strings
            .resolve(tech.routing_layers[l.0 as usize].name)
            .to_string()
    };
    let mut out = String::with_capacity(1 << 20);
    out.push_str(&format!("NETS {} ;\n", db.num_nets()));
    for n in 0..db.num_nets() {
        let net = NetId(n as u32);
        let net_name = guide_net_name(db, net);
        let mut rec = format!("    - {net_name}");
        for &pin in db.net_pins(net) {
            let p = pin.0 as usize;
            match db.pin_cells[p] {
                Some(cell) => rec.push_str(&format!(
                    " ( {} {} )",
                    escape_def_name(db.strings.resolve(db.cell_names[cell.0 as usize])),
                    escape_def_name(db.strings.resolve(db.pin_names[p])),
                )),
                None => rec.push_str(&format!(
                    " ( PIN {} )",
                    escape_def_name(db.strings.resolve(db.pin_names[p])),
                )),
            }
        }
        let mut stmts: Vec<String> = Vec::new();
        if let Some(geom) = db.net_geom(net) {
            for &(lid, ref rect) in &geom.wire_rects {
                match centerline(lid, rect, tech) {
                    Some((x1, y1, x2, y2)) => {
                        stmts.push(format!("{} ( {x1} {y1} ) ( {x2} {y2} )", layer_name(lid)));
                    }
                    // Not square-cap-expressible → a `RECT` record:
                    // deltas from the point, which `parse_net_wire`
                    // reads back as the identical `wire_rect` —
                    // self-spacing fills and foreign `RECT` stubs from
                    // a routed template both round-trip byte-stable.
                    None => stmts.push(format!(
                        "{} ( {} {} ) RECT ( 0 0 {} {} )",
                        layer_name(lid),
                        rect.xl,
                        rect.yl,
                        rect.xh - rect.xl,
                        rect.yh - rect.yl,
                    )),
                }
            }
            for via in &geom.vias {
                stmts.push(format!(
                    "{} ( {} {} ) {}",
                    layer_name(via.lower),
                    via.x_dbu,
                    via.y_dbu,
                    escape_def_name(db.strings.resolve(via.name)),
                ));
            }
        }
        for (i, stmt) in stmts.iter().enumerate() {
            let kw = if i == 0 { "+ ROUTED" } else { "NEW" };
            rec.push_str(&format!("\n    {kw} {stmt}"));
        }
        rec.push_str("\n    + USE SIGNAL ;\n");
        out.push_str(&rec);
    }
    out.push_str("END NETS\n");
    Ok(out)
}

/// Splices a canonical `NETS` section into `template_def`, writing
/// every byte outside `NETS n ;` … `END NETS` verbatim — the same
/// mechanism `write_def` uses for `COMPONENTS`. Errors when the
/// template has no `NETS` section or a `wire_rect` is not
/// square-cap-expressible.
pub fn write_routed_def(
    db: &DesignDb,
    tech: &TechDb,
    template_def: &Path,
    out_def: &Path,
) -> Result<()> {
    let template = std::fs::read_to_string(template_def)
        .with_context(|| format!("Failed to open template DEF at {template_def:?}"))?;

    let nets_start = find_section(&template, "NETS ").context("NETS section not found in template DEF")?;
    let nets_end = find_section(&template[nets_start..], "END NETS")
        .context("END NETS not found in template DEF")?
        + nets_start;
    let after_end = match template[nets_end..].find('\n') {
        Some(pos) => nets_end + pos + 1,
        None => template.len(),
    };

    let mut out = String::with_capacity(template.len() + (1 << 20));
    out.push_str(&template[..nets_start]);
    out.push_str(&nets_section(db, tech)?);
    out.push_str(&template[after_end..]);

    let mut file = std::fs::File::create(out_def)
        .with_context(|| format!("Failed to create routed DEF at {out_def:?}"))?;
    file.write_all(out.as_bytes())?;
    Ok(())
}

use crate::lib_db::{NldmTable, TableAxisVar};

/// Returns the interval index and the fractional position `u` inside it. Outside the axis the
/// first or last interval is extended linearly: OpenSTA never clamps a slew or a capacitance.
#[inline]
fn find_interval_unclamped(axis: &[f32], val: f32) -> (usize, f32) {
    let n = axis.len();
    if n <= 1 {
        return (0, 0.0);
    }
    let fraction = |i: usize| {
        let denom = axis[i + 1] - axis[i];
        if denom.abs() > 1e-12 { (val - axis[i]) / denom } else { 0.0 }
    };
    if val <= axis[0] {
        return (0, fraction(0));
    }
    if val >= axis[n - 1] {
        return (n - 2, fraction(n - 2));
    }
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if axis[mid] <= val {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo, fraction(lo))
}

pub fn eval_nldm_delay_or_slew(table: &NldmTable, in_slew_ns: f32, out_cap_pf: f32) -> f32 {
    let (x, y) = match (table.var1, table.var2) {
        (TableAxisVar::TotalOutputNetCapacitance, TableAxisVar::InputNetTransition) => {
            (out_cap_pf, in_slew_ns)
        }
        (TableAxisVar::InputNetTransition, _) => (in_slew_ns, out_cap_pf),
        (TableAxisVar::TotalOutputNetCapacitance, _) => (out_cap_pf, 0.0),
        _ => (in_slew_ns, out_cap_pf),
    };
    eval_2d(table, x, y)
}

pub fn eval_nldm_constraint(table: &NldmTable, constrained_slew_ns: f32, related_slew_ns: f32) -> f32 {
    let (x, y) = match (table.var1, table.var2) {
        (TableAxisVar::RelatedPinTransition, TableAxisVar::ConstrainedPinTransition) => {
            (related_slew_ns, constrained_slew_ns)
        }
        _ => (constrained_slew_ns, related_slew_ns),
    };
    eval_2d(table, x, y)
}

#[inline]
pub fn eval_2d(table: &NldmTable, x: f32, y: f32) -> f32 {
    let m = table.index_1.len();
    let n2 = table.index_2.len();
    if m <= 1 && n2 <= 1 {
        return table.values[0];
    }
    let (i, u) = find_interval_unclamped(&table.index_1, x);
    let i1 = (i + 1).min(m - 1);
    if n2 <= 1 {
        return (1.0 - u) * table.values[i] + u * table.values[i1];
    }
    let (j, v) = find_interval_unclamped(&table.index_2, y);
    let j1 = (j + 1).min(n2 - 1);
    if m <= 1 {
        return (1.0 - v) * table.values[j] + v * table.values[j1];
    }
    let t00 = table.values[i * n2 + j];
    let t10 = table.values[i1 * n2 + j];
    let t01 = table.values[i * n2 + j1];
    let t11 = table.values[i1 * n2 + j1];
    (1.0 - u) * (1.0 - v) * t00 + u * (1.0 - v) * t10 + (1.0 - u) * v * t01 + u * v * t11
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> NldmTable {
        // f(x, y) = 2x + 3y + 1 on a 2x2 grid, so interpolation and extrapolation are exact.
        NldmTable {
            var1: TableAxisVar::InputNetTransition,
            var2: TableAxisVar::TotalOutputNetCapacitance,
            index_1: vec![1.0, 2.0],
            index_2: vec![10.0, 20.0],
            values: vec![33.0, 63.0, 35.0, 65.0],
        }
    }

    #[test]
    fn interpolates_inside_the_grid() {
        assert!((eval_2d(&table(), 1.5, 15.0) - 49.0).abs() < 1e-4);
    }

    #[test]
    fn extrapolates_linearly_below_and_above() {
        let t = table();
        assert!((eval_2d(&t, 0.0, 10.0) - 31.0).abs() < 1e-4);
        assert!((eval_2d(&t, 4.0, 30.0) - 99.0).abs() < 1e-3);
    }

    #[test]
    fn swapped_axes_are_normalized() {
        let mut t = table();
        t.var1 = TableAxisVar::TotalOutputNetCapacitance;
        t.var2 = TableAxisVar::InputNetTransition;
        t.index_1 = vec![10.0, 20.0];
        t.index_2 = vec![1.0, 2.0];
        t.values = vec![33.0, 35.0, 63.0, 65.0];
        assert!((eval_nldm_delay_or_slew(&t, 1.5, 15.0) - 49.0).abs() < 1e-4);
    }
}

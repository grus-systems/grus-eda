//! Effective capacitance of an RC load seen by a Liberty NLDM driver, after Dartu, Menezes and
//! Pileggi, "Performance Computation for Precharacterized CMOS Gates with RC Loads" (IEEE TCAD 1996).
//!
//! The driver is a saturated voltage ramp (start `t0`, duration `dt`) behind a resistance `rd`.
//! For a trial `ceff` the ramp is fitted so that a single capacitor `ceff` reproduces the table
//! delay (50%) and slew (20%-80%); `ceff` is then adjusted until the charge that this source
//! delivers into the pi-model load up to the 50% crossing equals the charge delivered into `ceff`.

/// Driving-point pi model: `c1` at the driver, `rpi` in series, `c2` at the far end.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PiModel {
    pub c1: f64,
    pub rpi: f64,
    pub c2: f64,
}

impl PiModel {
    pub fn total(&self) -> f64 {
        self.c1 + self.c2
    }

    /// O'Brien-Savarino reduction from the first three driving-point admittance moments
    /// `Y(s) = y1 s + y2 s^2 + y3 s^3` (`y2 < 0`).
    pub fn from_moments(y1: f64, y2: f64, y3: f64) -> Self {
        if y2.abs() < 1e-30 || y3.abs() < 1e-30 {
            return Self { c1: y1, rpi: 0.0, c2: 0.0 };
        }
        let c2 = y2 * y2 / y3;
        Self { c1: y1 - c2, rpi: -(y3 * y3) / (y2 * y2 * y2), c2 }
    }
}

/// Output of [`solve_dmp`]: the effective capacitance and the table delay and slew at it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DmpResult {
    pub ceff: f64,
    pub delay: f64,
    pub slew: f64,
}

/// Normalised (Vdd = 1) voltage of a capacitor charged through `tau = rd * c` by a ramp that
/// starts at 0 and lasts `dt`.
fn rc_ramp_voltage(u: f64, dt: f64, tau: f64) -> f64 {
    if u <= 0.0 {
        0.0
    } else if u <= dt {
        u / dt - (tau / dt) * (1.0 - (-u / tau).exp())
    } else {
        1.0 - (tau / dt) * (-u / tau).exp() * ((dt / tau).exp() - 1.0)
    }
}

/// Closed-form crossing of `level` after the ramp. `None` unless the crossing is after the ramp
/// (`v(dt) <= level`) and the formula gives a finite time at or after `dt`.
fn rc_crossing_closed(level: f64, dt: f64, tau: f64) -> Option<f64> {
    if rc_ramp_voltage(dt, dt, tau) > level {
        return None;
    }
    let u = -tau * ((1.0 - level) * dt / (tau * (dt / tau).exp_m1())).ln();
    (u.is_finite() && u >= dt).then_some(u)
}

/// Time (from the ramp start) at which the RC response first reaches `level`, by bisection
/// until the bracket is at most 1e-6 ns wide.
fn rc_crossing_bisect(level: f64, dt: f64, tau: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, dt + tau * 40.0);
    while hi - lo > 1e-6 {
        let mid = 0.5 * (lo + hi);
        if rc_ramp_voltage(mid, dt, tau) < level {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Time (from the ramp start) at which the RC response first reaches `level`.
fn rc_crossing(level: f64, dt: f64, tau: f64) -> f64 {
    rc_crossing_closed(level, dt, tau).unwrap_or_else(|| rc_crossing_bisect(level, dt, tau))
}

/// 20%-80% transition time of the RC response, as a function of the ramp duration.
fn rc_slew(dt: f64, tau: f64) -> f64 {
    rc_crossing(0.8, dt, tau) - rc_crossing(0.2, dt, tau)
}

/// Ramp parameters `(t0, dt)` such that a capacitor `c` behind `rd` has its 50% point at `delay`
/// and a 20%-80% slew of `slew`. The input 50% point is time 0.
fn fit_ramp(rd: f64, c: f64, delay: f64, slew: f64) -> (f64, f64) {
    let tau = rd * c;
    // The slew grows monotonically with the ramp duration; at dt -> 0 it is the step response.
    let mut lo = 1e-9 * tau.max(1e-12);
    let mut hi = (slew * 4.0).max(lo * 2.0) + tau * 8.0;
    if rc_slew(lo, tau) >= slew {
        let t0 = delay - rc_crossing(0.5, lo, tau);
        return (t0, lo);
    }
    while hi - lo > 1e-6 {
        let mid = 0.5 * (lo + hi);
        if rc_slew(mid, tau) < slew {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let dt = 0.5 * (lo + hi);
    (delay - rc_crossing(0.5, dt, tau), dt)
}

/// Charge delivered by the ramp source through `rd` into the pi load between `t0` and `tx`:
/// closed form, with the numeric integrator as fallback.
fn pi_charge(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> f64 {
    pi_charge_closed(pi, rd, t0, dt, tx).unwrap_or_else(|| pi_charge_numeric(pi, rd, t0, dt, tx))
}

/// Closed-form charge `Q = c1 V1 + c2 V2` at `tx`, where `(V1, V2)` solve
/// `C1 V1' = (Vs - V1)/rd - (V1 - V2)/rpi`, `C2 V2' = (V1 - V2)/rpi` from rest. `Vs` is a ramp
/// on `[t0, t0 + dt]` and 1 after it. `None` if the eigen-solve is ill-conditioned or the result
/// is not finite.
fn pi_charge_closed(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> Option<f64> {
    let t = tx - t0;
    if t <= 0.0 {
        return Some(0.0);
    }
    let c1 = pi.c1.max(1e-4 * pi.total());
    let c2 = pi.c2;
    let (g, gp) = (1.0 / rd, 1.0 / pi.rpi);
    // x' = M x + forcing, with M = [[a, b], [d, e]].
    let (a, b, d, e) = (-(g + gp) / c1, gp / c1, gp / c2, -gp / c2);
    let det = g * gp / (c1 * c2);
    let tr = a + e;
    // tr^2 - 4 det, written as a sum of squares so that it does not cancel.
    let sq = ((a - e) * (a - e) + 4.0 * b * d).sqrt();
    if sq.is_nan() || sq <= 1e-6 * -tr {
        return None;
    }
    // Both eigenvalues are negative. The larger-magnitude one comes from the trace; the other
    // from the determinant, which avoids cancellation for the slow mode.
    let lam_a = 0.5 * (tr - sq);
    let lam_b = det / lam_a;
    let shift = |v: [f64; 2], lam: f64| [(a - lam) * v[0] + b * v[1], d * v[0] + (e - lam) * v[1]];
    // (exp(M s) - I) v, from Sylvester's formula for distinct eigenvalues.
    let expm_minus_one = |s: f64, v: [f64; 2]| {
        let (pa, pb) = ((lam_a * s).exp_m1(), (lam_b * s).exp_m1());
        let (sa, sb) = (shift(v, lam_b), shift(v, lam_a));
        [(pa * sa[0] - pb * sb[0]) / (lam_a - lam_b), (pa * sa[1] - pb * sb[1]) / (lam_a - lam_b)]
    };
    // M^-1 applied to (1, 1)/dt: the ramp's particular solution is (t/dt)(1, 1) - E(t)gamma.
    let inv_apply = |v: [f64; 2]| [(e * v[0] - b * v[1]) / det, (-d * v[0] + a * v[1]) / det];
    let gamma = inv_apply([1.0 / dt, 1.0 / dt]);
    let (v1, v2) = if t <= dt {
        let e_gamma = expm_minus_one(t, gamma);
        (t / dt - e_gamma[0], t / dt - e_gamma[1])
    } else {
        // After the ramp both nodes relax to 1 along the homogeneous flow from x(dt) - 1.
        let offset = expm_minus_one(dt, gamma);
        let w = [-offset[0], -offset[1]];
        let decay = expm_minus_one(t - dt, w);
        (1.0 + w[0] + decay[0], 1.0 + w[1] + decay[1])
    };
    let q = c1 * v1 + c2 * v2;
    q.is_finite().then_some(q)
}

/// Charge delivered by the ramp source through `rd` into the pi load between `t0` and `tx`, by
/// the implicit trapezoid rule with 200 steps. Fallback for [`pi_charge_closed`], and the
/// reference it is tested against.
fn pi_charge_numeric(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> f64 {
    let c1 = pi.c1.max(1e-4 * pi.total());
    let (c2, rpi) = (pi.c2, pi.rpi);
    let steps = 200usize;
    let h = (tx - t0) / steps as f64;
    if h <= 0.0 {
        return 0.0;
    }
    let vs = |t: f64| ((t - t0) / dt).clamp(0.0, 1.0);
    // Implicit trapezoid on C1 V1' = (Vs - V1)/rd - (V1 - V2)/rpi, C2 V2' = (V1 - V2)/rpi.
    let (g, gp) = (1.0 / rd, 1.0 / rpi);
    let (a11, a12) = (-(g + gp) / c1, gp / c1);
    let (a21, a22) = (gp / c2, -gp / c2);
    let (m11, m12, m21, m22) = (1.0 - 0.5 * h * a11, -0.5 * h * a12, -0.5 * h * a21, 1.0 - 0.5 * h * a22);
    let det = m11 * m22 - m12 * m21;
    let (mut v1, mut v2, mut q) = (0.0, 0.0, 0.0);
    let mut i_prev = (vs(t0) - v1) * g;
    for k in 1..=steps {
        let t = t0 + h * k as f64;
        let rhs1 = v1 + 0.5 * h * (a11 * v1 + a12 * v2) + 0.5 * h * (vs(t - h) + vs(t)) * g / c1;
        let rhs2 = v2 + 0.5 * h * (a21 * v1 + a22 * v2);
        let (n1, n2) = ((rhs1 * m22 - m12 * rhs2) / det, (m11 * rhs2 - m21 * rhs1) / det);
        v1 = n1;
        v2 = n2;
        let i_now = (vs(t) - v1) * g;
        q += 0.5 * h * (i_prev + i_now);
        i_prev = i_now;
    }
    q
}

/// Solves for the effective capacitance. `table(c)` returns the table `(delay, slew)` at load `c`;
/// `rd_scale` converts the local delay-vs-capacitance slope of the table into the driver resistance.
pub fn solve_dmp(pi: &PiModel, rd_scale: f64, table: &dyn Fn(f64) -> (f64, f64)) -> DmpResult {
    let total = pi.total();
    let lumped = |c: f64| {
        let (delay, slew) = table(c);
        DmpResult { ceff: c, delay, slew }
    };
    if pi.c2 <= 0.0 || pi.rpi <= 1e-9 {
        return lumped(total);
    }
    // f(c) > 0 when the pi load takes more charge than a capacitor `c` would: c is too small.
    let residual = |c: f64| {
        let (delay, slew) = table(c);
        let h = 0.05 * c;
        let slope = (table(c + h).0 - table(c - h).0) / (2.0 * h);
        let rd = (rd_scale * slope).max(1e-6);
        let (t0, dt) = fit_ramp(rd, c, delay, slew);
        let tx = delay;
        pi_charge(pi, rd, t0, dt, tx) - 0.5 * c
    };
    let (mut lo, mut hi) = (pi.c1.max(1e-4 * total), total);
    if residual(hi) >= 0.0 {
        return lumped(total);
    }
    if residual(lo) <= 0.0 {
        return lumped(lo);
    }
    while hi - lo > 1e-6 * total {
        let mid = 0.5 * (lo + hi);
        if residual(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lumped(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linear driver: delay = 0.7 * rd * c + 0.05, slew = 2.2 * rd * c + 0.02 (ns, pF, kohm).
    fn linear_table(rd: f64) -> impl Fn(f64) -> (f64, f64) {
        move |c| (0.7 * rd * c + 0.05, 2.2 * rd * c + 0.02)
    }

    #[test]
    fn moments_of_a_single_rc_give_that_rc() {
        let (r, c) = (0.3, 0.02);
        let pi = PiModel::from_moments(c, -r * c * c, r * r * c * c * c);
        assert!((pi.c2 - c).abs() < 1e-12 && (pi.rpi - r).abs() < 1e-12 && pi.c1.abs() < 1e-12);
    }

    #[test]
    fn no_resistance_means_no_shielding() {
        let pi = PiModel { c1: 0.01, rpi: 0.0, c2: 0.02 };
        let r = solve_dmp(&pi, 1.0 / std::f64::consts::LN_2, &linear_table(2.0));
        assert!((r.ceff - 0.03).abs() < 1e-12);
    }

    #[test]
    fn resistive_shielding_lowers_ceff_between_c1_and_total() {
        let pi = PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 };
        let r = solve_dmp(&pi, 1.0 / std::f64::consts::LN_2, &linear_table(1.0));
        assert!(r.ceff > 0.01 && r.ceff < 0.05, "ceff = {}", r.ceff);
        // A stronger driver makes the same load look more shielded.
        let strong = solve_dmp(&pi, 1.0 / std::f64::consts::LN_2, &linear_table(0.2));
        assert!(strong.ceff < r.ceff);
    }

    /// Golden regression: `(table, pi, ceff, delay, slew)` recorded from the solver before the
    /// E3.6 speed-up (2026-11-13). Recorded from the pre-E3.6 implementation; the tolerance covers the stop-rule noise
    /// (~1e-5) only. The charge evaluation must not move the model: see design Section 7.
    #[test]
    fn golden_regression_values_are_reproduced() {
        let tables: [(&str, &dyn Fn(f64) -> (f64, f64)); 3] = [
            ("lin1", &|c| (0.7 * c + 0.05, 2.2 * c + 0.02)),
            ("lin2", &|c| (1.4 * c + 0.05, 4.4 * c + 0.02)),
            ("nl", &|c| (0.05 + 0.9 * c.powf(0.8), 0.02 + 1.4 * c.powf(0.9))),
        ];
        let pis = [
            PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 },
            PiModel { c1: 0.001, rpi: 2.0, c2: 0.2 },
            PiModel { c1: 0.005, rpi: 0.05, c2: 0.03 },
            PiModel { c1: 0.02, rpi: 50.0, c2: 0.01 },
            PiModel { c1: 0.002, rpi: 0.5, c2: 0.002 },
        ];
        #[rustfmt::skip]
        let golden: [(&str, usize, f64, f64, f64); 15] = [
            ("lin1", 0, 1.355989813805e-02, 5.949192869663e-02, 4.983177590370e-02),
            ("lin1", 1, 9.480459451675e-03, 5.663632161617e-02, 4.085701079369e-02),
            ("lin1", 2, 3.421102732420e-02, 7.394771912694e-02, 9.526426011324e-02),
            ("lin1", 3, 2.045850068331e-02, 6.432095047832e-02, 6.500870150328e-02),
            ("lin1", 4, 3.899842679501e-03, 5.272988987565e-02, 2.857965389490e-02),
            ("lin2", 0, 1.634207129478e-02, 7.287889981270e-02, 9.190511369705e-02),
            ("lin2", 1, 3.239255642891e-02, 9.534957900047e-02, 1.625272482872e-01),
            ("lin2", 2, 3.454947739840e-02, 9.836926835775e-02, 1.720177005529e-01),
            ("lin2", 3, 2.076532274485e-02, 7.907145184278e-02, 1.113674200773e-01),
            ("lin2", 4, 3.917413175106e-03, 5.548437844515e-02, 3.723661797047e-02),
            ("nl", 0, 1.250149369240e-02, 7.702788325274e-02, 4.712649929345e-02),
            ("nl", 1, 8.382279634476e-03, 6.963053904824e-02, 3.893014611227e-02),
            ("nl", 2, 3.337034612894e-02, 1.092831236217e-01, 8.563759741969e-02),
            ("nl", 3, 2.011763006449e-02, 8.954612763281e-02, 6.162432191976e-02),
            ("nl", 4, 3.884719073772e-03, 6.061036206277e-02, 2.947439946701e-02),
        ];
        let rel_ok = |got: f64, want: f64| (got - want).abs() <= 5e-5 * want.abs();
        let mut failures = Vec::new();
        for (name, pi_index, ceff, delay, slew) in golden {
            let table = tables.iter().find(|t| t.0 == name).expect("golden table").1;
            let r = solve_dmp(&pis[pi_index], 1.0 / std::f64::consts::LN_2, table);
            if !(rel_ok(r.ceff, ceff) && rel_ok(r.delay, delay) && rel_ok(r.slew, slew)) {
                failures.push(format!("{name} pi{pi_index}: got {r:?}, want ({ceff}, {delay}, {slew})"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Ramp-charge grid: the pi models of the golden set plus a stiff, strongly shielded one.
    fn charge_grid_models() -> Vec<PiModel> {
        vec![
            PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 },
            PiModel { c1: 0.001, rpi: 2.0, c2: 0.2 },
            PiModel { c1: 0.005, rpi: 0.05, c2: 0.03 },
            PiModel { c1: 0.02, rpi: 50.0, c2: 0.01 },
            PiModel { c1: 0.002, rpi: 0.5, c2: 0.002 },
            PiModel { c1: 0.001, rpi: 1e-3, c2: 0.2 },
        ]
    }

    #[test]
    fn closed_form_charge_matches_the_integrator() {
        for pi in charge_grid_models() {
            for rd in [0.2, 1.0, 5.0] {
                for dt in [0.02, 0.1, 0.4] {
                    let t0 = 0.05;
                    // Both regimes, the regime boundary, and points just either side of it.
                    for t in [0.5 * dt, dt - 1e-9, dt, dt + 1e-9, 1.5 * dt, 4.0 * dt] {
                        let closed = pi_charge_closed(&pi, rd, t0, dt, t0 + t)
                            .expect("closed form must not fall back on the test grid");
                        let numeric = pi_charge_numeric(&pi, rd, t0, dt, t0 + t);
                        assert!(
                            (closed - numeric).abs() <= 1e-4 * numeric.abs(),
                            "pi {pi:?} rd {rd} dt {dt} tx-t0 {t}: closed {closed} numeric {numeric}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn charge_is_exactly_zero_at_the_ramp_start_and_continuous_across_its_end() {
        for pi in charge_grid_models() {
            for (rd, dt) in [(0.2, 0.02), (1.0, 0.1), (5.0, 0.4)] {
                let t0 = 0.05;
                assert_eq!(pi_charge_closed(&pi, rd, t0, dt, t0), Some(0.0));
                let before = pi_charge_closed(&pi, rd, t0, dt, t0 + dt - 1e-9).unwrap();
                let after = pi_charge_closed(&pi, rd, t0, dt, t0 + dt + 1e-9).unwrap();
                assert!((before - after).abs() <= 1e-6 * after.abs(), "{before} vs {after}");
            }
        }
    }

    /// The 40-iteration bisection that `rc_crossing` replaced.
    fn rc_crossing_reference(level: f64, dt: f64, tau: f64) -> f64 {
        let mut hi = dt + tau * 40.0;
        let mut lo = 0.0;
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if rc_ramp_voltage(mid, dt, tau) < level {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// `dt / tau` at which the response reaches `level` exactly at the end of the ramp.
    fn ramp_end_ratio(level: f64) -> f64 {
        let (mut lo, mut hi) = (1e-6, 100.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if rc_ramp_voltage(mid, mid, 1.0) > level {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        0.5 * (lo + hi)
    }

    #[test]
    fn closed_form_crossing_matches_full_bisection_where_it_applies() {
        for level in [0.2, 0.5, 0.8] {
            let x_end = ramp_end_ratio(level);
            for tau in [0.003, 0.05, 0.4] {
                for x in [0.05 * x_end, 0.5 * x_end, 0.99 * x_end] {
                    let dt = x * tau;
                    assert!(rc_ramp_voltage(dt, dt, tau) <= level);
                    let closed = rc_crossing_closed(level, dt, tau)
                        .expect("closed form must apply when v(dt) <= level");
                    let reference = rc_crossing_reference(level, dt, tau);
                    assert!(
                        (closed - reference).abs() <= 1e-6,
                        "level {level} dt {dt} tau {tau}: closed {closed} reference {reference}"
                    );
                }
            }
        }
    }

    #[test]
    fn crossing_at_the_ramp_end_and_before_it_is_handled() {
        for level in [0.2, 0.5, 0.8] {
            let x_end = ramp_end_ratio(level);
            for tau in [0.003, 0.05, 0.4] {
                // Boundary: the level is exactly the response at the end of the ramp.
                let dt = x_end * tau;
                let boundary = rc_ramp_voltage(dt, dt, tau);
                let reference = rc_crossing_reference(boundary, dt, tau);
                assert!((rc_crossing(boundary, dt, tau) - reference).abs() <= 1e-6);
                // Not triggered: the crossing happens during the ramp.
                let dt = 1.5 * x_end * tau;
                assert!(rc_crossing_closed(level, dt, tau).is_none());
                let reference = rc_crossing_reference(level, dt, tau);
                assert!((rc_crossing(level, dt, tau) - reference).abs() <= 1e-6);
            }
        }
    }
}

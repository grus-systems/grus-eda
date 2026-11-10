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

/// Time (from the ramp start) at which the RC response first reaches `level`.
fn rc_crossing(level: f64, dt: f64, tau: f64) -> f64 {
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
    for _ in 0..40 {
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

/// Charge delivered by the ramp source through `rd` into the pi load between `t0` and `tx`.
fn pi_charge(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> f64 {
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
    for _ in 0..24 {
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
}

# E2: Global Placement & Abacus Legalization — Implementation Spec

This document breaks E2 into four concrete implementation steps (`E2.1`–`E2.4`) with exact equations and Rust pseudocode for the implementer.

---

## 1. Done Criteria

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E2.1** | Weighted-Average (WA) wirelength & gradient (`placement/wirelength.rs`) | Finite-difference gradient unit test | Analytical gradient matches central finite differences within `< 1e-4` relative error |
| **E2.2** | 2D DCT/IDXST Poisson electrostatic solver (`placement/density.rs`) | Analytical 2D cosine charge test + finite-difference force check | Electric field $(E_x, E_y)$ matches $-\nabla \phi$ and total net force sums to $0$ |
| **E2.3** | Nesterov global placer (`placement/nesterov.rs`) | `openroad`: `global_placement` | Converges to overflow $\tau \le 0.10$ in $< 300$ iterations |
| **E2.4** | Abacus legalizer with local refinement (`placement/legalize.rs`) & DEF writer | `openroad`: `check_placement -verbose` & `report_design_area` | `check_placement` reports 0 violations, `detailed_placement` accepts output DEF, and legalized HPWL is within 5% of OpenROAD |

---

## 2. Step-by-Step Algorithms & Equations

### Step E2.1: Smooth Weighted-Average (WA) Wirelength (`wirelength.rs`)

> **Three Mandatory `f32` Numerical Safeguards**:
> 1. **Exponent Shift (Preventing `expf` Overflow)**: In `f32`, `exp(x)` overflows to `+inf` as soon as the argument exceeds `88.72` (`NaN` in division). Never evaluate `exp(x_p / gamma)` directly; always use shifted coordinates $\tilde{x}_p^+ = x_p - x_{\max} \le 0$ and $\tilde{x}_p^- = x_p - x_{\min} \ge 0$ so every exponent argument is $\le 0$.
> 2. **Shifted Coordinates Inside $B_e^\pm$ and Gradient (Preventing Catastrophic Cancellation)**: If $B_e^+ = \sum x_p a_p^+$ is evaluated with large absolute coordinates (e.g., $x_p \approx 5000\,\mu\text{m}$ while net span is $2\,\mu\text{m}$), subtracting $x_p A_e^+ - B_e^+$ in `f32` cancels the leading 12+ bits of mantissa! Because shifting all $x_p$ on net $e$ by a constant shifts $W_x^+(e)$ by that constant and leaves the gradient $\frac{\partial W_x(e)}{\partial x_p}$ **mathematically invariant**, always compute $\tilde{B}_e^+$ and the gradient using $\tilde{x}_p^+ = x_p - x_{\max}$ and $\tilde{x}_p^- = x_p - x_{\min}$.
> 3. **`f64` Scalar Accumulators for Global Reductions (Preventing Mantissa Swamping)**: Keep all per-cell and per-pin arrays (`pos_x`, `grad_x`, `e_field_x`) as `Vec<f32>`, but accumulate global sums—total WA wirelength $\sum_e W(e)$, $L_1$ norm $\|\nabla W\|_1$, and Barzilai-Borwein $L_2$ dot products $\|v_k - v_{k-1}\|_2^2$, $\|g_k - g_{k-1}\|_2^2$—in scalar **`f64`** (and sum exact integer DBU HPWL in **`i64`**).

Let movable cell $c$ have center coordinate $(x_c, y_c)$ in microns (with die lower-left origin shifted to $(0, 0)$). Each pin $p \in e$ on net $e$ has position:
$$x_p = x_{c(p)} + \Delta x_p, \quad y_p = y_{c(p)} + \Delta y_p$$

For a smoothing parameter $\gamma > 0$, define shifted pin coordinates $\tilde{x}_p^+ = x_p - x_{\max} \le 0$ and $\tilde{x}_p^- = x_p - x_{\min} \ge 0$ (where $x_{\max} = \max_{p \in e} x_p$ and $x_{\min} = \min_{p \in e} x_p$):

$$a_p^+ = \exp\!\left(\frac{\tilde{x}_p^+}{\gamma}\right) \in (0, 1], \quad a_p^- = \exp\!\left(\frac{-\tilde{x}_p^-}{\gamma}\right) \in (0, 1]$$

Define the four shifted net accumulators:
$$A_e^+ = \sum_{p \in e} a_p^+, \quad \tilde{B}_e^+ = \sum_{p \in e} \tilde{x}_p^+ \, a_p^+, \quad A_e^- = \sum_{p \in e} a_p^-, \quad \tilde{B}_e^- = \sum_{p \in e} \tilde{x}_p^- \, a_p^-$$

Then the WA wirelength along $x$ for net $e$ is:
$$W_x(e) = (x_{\max} - x_{\min}) + \frac{\tilde{B}_e^+}{A_e^+} - \frac{\tilde{B}_e^-}{A_e^-}$$

And the analytical partial derivative with respect to pin $p \in e$ (free of catastrophic cancellation) is:
$$\frac{\partial W_x(e)}{\partial x_p} = \frac{a_p^+}{\gamma \, (A_e^+)^2} \Bigl(\bigl(\gamma + \tilde{x}_p^+\bigr) A_e^+ - \tilde{B}_e^+\Bigr) - \frac{a_p^-}{\gamma \, (A_e^-)^2} \Bigl(\bigl(\gamma - \tilde{x}_p^-\bigr) A_e^- + \tilde{B}_e^-\Bigr)$$

(Identical formulas apply independently to $y_p$.)
To accumulate cell gradients without atomic operations:
1. Compute pin gradients `pin_grad_x[p]` and `pin_grad_y[p]` across nets `e` (since each pin belongs to at most one net).
2. Gather into `cell_grad_x[c]` and `cell_grad_y[c]` across movable cells `c` by summing over `cell_pin_ids[cell_pin_offsets[c] .. cell_pin_offsets[c+1]]` (setting `0.0` for `Fixed`/`Cover`/`CoreSpacer` cells).

#### Concrete Structs & Signatures (`crates/grus-core/src/placement/wirelength.rs`)
```rust
#[derive(Clone, Debug)]
pub struct WaWirelengthPrecomp {
    pub dbu_per_um: f32,
    pub pin_dx_um: Vec<f32>, // length num_pins: offset from cell lower-left (or absolute I/O x_um)
    pub pin_dy_um: Vec<f32>, // length num_pins: offset from cell lower-left (or absolute I/O y_um)
    pub movable_cell_mask: Vec<bool>, // length num_cells: true if movable Core/CoreTie cell
}
```
- `WaWirelengthPrecomp::new(design_db: &DesignDb, tech_db: &TechDb) -> Self`:
  - For each pin `p`: if `design_db.pin_cells[p] == Some(c)`, compute `(px_dbu, py_dbu) = design_db.pin_pos_dbu(PinId(p as u32), tech_db)` and set `pin_dx_um[p] = (px_dbu - design_db.cell_x_dbu[c]) as f32 / dbu_per_um` (if `cell_status[c] == Unplaced`, use `design_db.pin_offset_x_dbu[p] as f32 / dbu_per_um`). If `design_db.pin_cells[p] == None` (primary I/O), set `pin_dx_um[p] = design_db.pin_offset_x_dbu[p] as f32 / dbu_per_um`.
  - A cell `c` has `movable_cell_mask[c] = true` iff `cell_status[c]` is `Placed` or `Unplaced` and its macro class is `MacroClass::Core` or `MacroClass::CoreTie`.
- `pub fn compute_wa_wirelength_and_grad(design_db: &DesignDb, precomp: &WaWirelengthPrecomp, cell_x_um: &[f32], cell_y_um: &[f32], gamma: f32, pin_grad_x: &mut [f32], pin_grad_y: &mut [f32], cell_grad_x: &mut [f32], cell_grad_y: &mut [f32]) -> f64`
- `pub fn compute_hpwl_um(design_db: &DesignDb, precomp: &WaWirelengthPrecomp, cell_x_um: &[f32], cell_y_um: &[f32]) -> f64`
- **Verification on `gcd.placed.def`**:
  - Verify `compute_hpwl_um` at placed coordinates equals `placed_hpwl_um` (`4529.224 um`) from `gcd.reference.json`.
  - Verify that as $\gamma$ decreases (`10.0 -> 1.0 -> 0.1 -> 0.001 um`), `compute_wa_wirelength_and_grad` increases monotonically and converges to `4529.224 um` within `< 0.01 um` at $\gamma = 0.001\,\mu\text{m}$.
  - Verify analytical gradients `(cell_grad_x[c], cell_grad_y[c])` against central finite differences across all 233 movable logic cells on `gcd.placed.def` at $\gamma = 5.0\,\mu\text{m}$ using an exact dyadic step $h = 2^{-6} = 0.015625\,\mu\text{m}$ (so `x_plus - x_minus == 2.0 * h` is exact in `f32`), asserting relative error `< 1e-4` on every cell with $\|\nabla_c W\| > 10^{-3}$.

---

### Step E2.2: Electrostatic Density & 2D Spectral Solver (`density.rs`)

#### 1. Physical-Only Cell Classification & Bin Grid Setup
- **Cell Filtering**:
  - `MacroClass::CoreSpacer` (`sky130_fd_sc_hd__fill_*`, post-placement decap fillers): excluded from E2 (if present in an input DEF, strip them on load).
  - `PlacementStatus::Fixed` cells (`tapvpwrvgnd`, endcap `decap`, block macros): stationary. Their area is added to the fixed background bin density $\rho_{\text{fixed}}(u, v)$ scaled by $\rho_{\text{target}}$, and they act as fixed obstacles splitting rows in Step E2.4.
  - `PlacementStatus::Placed` / `Unplaced` standard cells (`MacroClass::Core` / `CoreTie`): movable optimization variables.
- Choose grid dimension $M = 2^k \in \{16, 32, 64, 128, 256, 512\}$ as the smallest power of 2 such that bin height $h_b = H_{\text{core}} / M \le 2.0 \cdot h_{\text{site}}$ (for `gcd`, $H_{\text{core}} = 78.88\,\mu\text{m}$ and $h_{\text{site}} = 2.72\,\mu\text{m}$ gives $M = 16$, $w_b = 5.03\,\mu\text{m}$, $h_b = 4.93\,\mu\text{m} = 1.81 \times h_{\text{site}}$, matching OpenROAD `GPL-0028`).
- Local density stretching (ePlace rule): if movable cell $c$ has width $w_c < \sqrt{2} w_b$, stretch its density footprint width to $w'_c = \sqrt{2} w_b$ and scale its density weight by $s_c = \frac{w_c h_c}{w'_c h'_c}$ so total cell area (charge) is conserved while making the overlap function piecewise smooth across bins.
- Accumulate cell area overlap with each bin $(u, v) \in [0, M)^2$ into $\rho(u, v)$, and subtract the DC mean $\bar{\rho} = \frac{1}{M^2}\sum_{u, v} \rho(u, v)$ so net total charge is zero.

#### 2. Exact 1D Forward DCT-II, Inverse IDCT (DCT-III), and Inverse IDXST via `rustfft`
Let $M$ be a power of 2:

- **1D Forward DCT-II (`dct1d`, unnormalized $\sum_{n=0}^{M-1} x_n \cos(\frac{\pi u (2n+1)}{2M})$)**:
  1. Reorder $x[0..M]$ into complex buffer $v[0..M]$:
     $$v[k] = x[2k], \quad v[M - 1 - k] = x[2k + 1] \quad \text{for } k = 0, \dots, \frac{M}{2} - 1$$
  2. Run 1D forward complex FFT on $v \to V$.
  3. Multiply by twiddle factors:
     $$C[u] = \text{Re}\!\left(V[u] \cdot e^{-i \frac{\pi u}{2M}}\right) \quad \text{for } u = 0, \dots, M - 1$$

- **1D Inverse DCT (`idct1d`, satisfying $\text{idct1d}(\text{dct1d}(x)) = x$)**:
  1. Given spectral coefficients $C[0..M]$, form complex frequency vector $W[0..M]$:
     $$W[0] = \frac{1}{M} C[0], \quad W[u] = \frac{1}{M}\bigl(C[u] - i \, C[M - u]\bigr)\,e^{i \frac{\pi u}{2M}} \quad \text{for } u = 1, \dots, M - 1$$
     and run 1D inverse complex IFFT (`plan_fft_inverse`) on $W \to v$. (Because $C[u] - i C[M - u]$ already combines both positive and negative frequencies via $C[M - u] = -\text{Im}(V[u] e^{-i \pi u / (2M)})$, the scale factor is $\frac{1}{M}$, not $\frac{2}{M}$.)
  2. Un-permute the real part $v[0..M]$ back to spatial order:
     $$x[2k] = \text{Re}(v[k]), \quad x[2k + 1] = \text{Re}(v[M - 1 - k]) \quad \text{for } k = 0, \dots, \frac{M}{2} - 1$$

- **1D Inverse Shifted Sine Transform (`idxst1d`, computing $\sum_{u=1}^{M-1} S[u] \sin(\frac{\pi u (2n+1)}{2M})$)**:
  Using the identity $\sin\!\bigl(\frac{\pi u(2n+1)}{2M}\bigr) = (-1)^n \cos\!\bigl(\frac{\pi (M - u)(2n+1)}{2M}\bigr)$:
  1. Define reversed cosine coefficients $\tilde{C}[0] = 0$ and $\tilde{C}[u] = S[M - u]$ for $u = 1, \dots, M - 1$.
  2. Compute $y = \text{idct1d}(\tilde{C})$.
  3. Flip signs on odd spatial indices:
     $$\text{idxst1d}(S)[n] = (-1)^n \, y[n] \quad \text{for } n = 0, \dots, M - 1$$

- **2D Spectral Poisson Solve**:
  1. Apply `dct1d` to all rows of $\rho(u, v)$, then `dct1d` to all columns to obtain $a_{u, v}$.
  2. Define wavenumbers $\omega_u = \frac{\pi u}{M w_b}$ and $\omega_v = \frac{\pi v}{M h_b}$:
     $$\hat{\phi}_{u, v} = \begin{cases} 0 & \text{if } u = 0 \text{ and } v = 0 \\ \dfrac{a_{u, v}}{\omega_u^2 + \omega_v^2} & \text{otherwise} \end{cases}, \quad \hat{E}^x_{u, v} = \omega_u \, \hat{\phi}_{u, v}, \quad \hat{E}^y_{u, v} = \omega_v \, \hat{\phi}_{u, v}$$
  3. Recover $\phi(x, y)$ by applying `idct1d` along rows and `idct1d` along columns of $\hat{\phi}$.
  4. Recover $E_x(x, y)$ by applying `idxst1d` along $u$ (horizontal rows) and `idct1d` along $v$ (vertical columns) of $\hat{E}^x$.
  5. Recover $E_y(x, y)$ by applying `idct1d` along $u$ (horizontal rows) and `idxst1d` along $v$ (vertical columns) of $\hat{E}^y$.
- **Cell Electrostatic Force Gathering**:
  For each movable cell $c$, gather the repulsive electrostatic force:
  $$F_x[c] = s_c \sum_{u, v} \Delta x(c, u) \, \Delta y(c, v) \, E_x(u, v), \quad F_y[c] = s_c \sum_{u, v} \Delta x(c, u) \, \Delta y(c, v) \, E_y(u, v)$$
  (where the density penalty gradient is $\frac{\partial \Psi}{\partial x_c} = -F_x[c], \frac{\partial \Psi}{\partial y_c} = -F_y[c]$).

#### Concrete Structs & Signatures (`crates/grus-core/src/placement/density.rs`)
```rust
pub struct DensitySolver {
    pub m: usize, // power-of-2 grid dimension M x M
    pub core_xl_um: f32,
    pub core_yl_um: f32,
    pub core_xh_um: f32,
    pub core_yh_um: f32,
    pub bin_w_um: f32,
    pub bin_h_um: f32,
    pub bin_area_um2: f32,
    pub target_density: f32,
    pub total_movable_area_um2: f32,
    pub cell_w_um: Vec<f32>,
    pub cell_h_um: Vec<f32>,
    pub cell_stretched_w_um: Vec<f32>,
    pub cell_stretched_h_um: Vec<f32>,
    pub cell_density_scale: Vec<f32>, // s_c = (w_c * h_c) / (w'_c * h'_c)
    pub movable_cell_mask: Vec<bool>,
    pub fixed_area: Vec<f32>,   // length m * m
    pub movable_area: Vec<f32>, // length m * m
    pub rho: Vec<f32>,          // length m * m (zero-mean charge density)
    pub phi: Vec<f32>,          // length m * m (electrostatic potential)
    pub e_x: Vec<f32>,          // length m * m (horizontal electric field -dphi/dx)
    pub e_y: Vec<f32>,          // length m * m (vertical electric field -dphi/dy)
}
```
- `DensitySolver::new(design_db: &DesignDb, tech_db: &TechDb, target_density: f32, grid_m_override: Option<usize>) -> Self`
- `DensitySolver::solve_poisson_from_rho(&mut self)` (solves `self.phi`, `self.e_x`, `self.e_y` from `self.rho`)
- `DensitySolver::compute_density_and_forces(&mut self, cell_x_um: &[f32], cell_y_um: &[f32], force_x: &mut [f32], force_y: &mut [f32]) -> (f32, f64)` (returns `(overflow_tau, potential_energy_psi)`)

---

### Step E2.3: Nesterov Accelerated Gradient Loop (`nesterov.rs`)

Use `f64` scalar accumulators for all vector norms and dot products over `f32` slices:

```rust
#[inline]
pub fn l1_norm_f64(xs: &[f32], ys: &[f32]) -> f64 {
    xs.iter().chain(ys.iter()).map(|&v| (v as f64).abs()).sum()
}

#[inline]
pub fn l2_dist_f64(ax: &[f32], ay: &[f32], bx: &[f32], by: &[f32]) -> f32 {
    let sum_sq: f64 = ax.iter().zip(bx.iter())
        .chain(ay.iter().zip(by.iter()))
        .map(|(&a, &b)| {
            let d = (a as f64) - (b as f64);
            d * d
        })
        .sum();
    sum_sq.sqrt() as f32
}
```

1. **Initialization ($k = 0$)**:
   - Start all movable cells at the core center $(x_{\text{center}} - 0.5 w_c,\; y_{\text{center}} - 0.5 h_c)$, and run up to 60 iterations of **Jacobi-preconditioned Conjugate Gradient (CG)** solving the regularized star quadratic wirelength minimization problem ($\sum_{|e| \ge 2} \frac{1}{|e| - 1} \sum_{p \in e} ((x_p - \bar{x}_e)^2 + (y_p - \bar{y}_e)^2) + 10^{-6}\|x - x_0\|_2^2$, stopping early when $r^T z < 10^{-12}$), clamping movable cells to `[core_xl, core_xh - w_c] x [core_yl, core_yh - h_c]`. Compared to 15 damped Jacobi barycenter steps, CG propagates I/O anchor geometry across multi-hop combinational paths (reducing `gcd` initial overflow $\tau_0 \approx 0.40$ and lowering `ibex` and `jpeg` legalized HPWL).
   - Add a small deterministic perturbation $\delta x_c = (((c \cdot 73) \bmod 29) as f32 - 14.0) \cdot 0.005 \cdot w_b$, $\delta y_c = (((c \cdot 37) \bmod 29) as f32 - 14.0) \cdot 0.005 \cdot h_b$ (clamped to core bounds) so no two cells start on identical coordinates.
   - Set base wirelength smoothing $\gamma_{\text{base}} = 0.25 \cdot (w_b + h_b)$ and schedule $\gamma(\tau_k) = \gamma_{\text{base}} \cdot 10^{2.0 \cdot (\tau_k - 0.10)}$.
   - Evaluate initial wirelength gradient $\nabla W(x_0)$ and repulsive electric force $F(x_0)$, and set initial penalty multiplier:
     $$\lambda_0 = 0.15 \cdot \frac{\|\nabla W(x_0)\|_1}{\|F(x_0)\|_1}$$
   - Set Nesterov parameter $t_0 = 1.0$, solution $u_0 = x_0$, and reference point $v_0 = x_0$.
2. **Iteration Step ($k = 0, \dots, \text{max\_iters} - 1$)**:
   - Evaluate density overflow $\tau_k$ and repulsive force $F(v_k)$ via `DensitySolver::compute_density_and_forces`, and evaluate $\nabla W(v_k)$ at $\gamma_k = \gamma(\tau_k)$.
   - If $\tau_k \le \tau_{\text{stop}}$ (default `0.10`), commit $v_k$ (clamped to core bounds) and terminate.
   - Compute the ePlace diagonally preconditioned gradient at reference point $v_k$:
     $$g_k[c] = \frac{\nabla_c W(v_k) - \lambda_k \, F_c(v_k)}{\max\bigl(1.0,\; \text{pin\_count}[c] + \lambda_k \, a_c\bigr)}$$
   - Compute Barzilai-Borwein step size $\alpha_k$:
     $$\alpha_k = \begin{cases} \dfrac{0.05 \cdot (w_b + h_b)}{\max(10^{-6},\; \|g_0\|_{\infty})} & \text{if } k == 0, \\[8pt] \text{clamp}\!\left(\dfrac{\|v_k - v_{k-1}\|_2}{\max(10^{-9},\; \|g_k - g_{k-1}\|_2)},\; 0.25\,\alpha_{k-1},\; 2.0\,\alpha_{k-1}\right) & \text{if } k \ge 1. \end{cases}$$
     (Bounding relative to $\alpha_{k-1}$ rather than clamping globally by $0.5(w_b + h_b) / \|g_k\|_\infty$ avoids throttling the entire design when a single high-gradient cell has large $|g_k[c]|$.)
   - Update solution and Nesterov lookahead:
     $$u_{k+1} = \text{clamp}_{\text{core}}\bigl(v_k - \alpha_k \, g_k\bigr)$$
     $$t_{k+1} = \frac{1 + \sqrt{1 + 4 t_k^2}}{2}, \quad v_{k+1} = \text{clamp}_{\text{core}}\!\left(u_{k+1} + \frac{t_k - 1}{t_{k+1}}\bigl(u_{k+1} - u_k\bigr)\right)$$
   - Update $\lambda_{k+1}$ using RePlAce Eq. 11 with $\Delta \text{HPWL} = \text{HPWL}(u_{k+1}) - \text{HPWL}(u_k)$, dynamic wirelength scale $\text{hpwl\_est} = 0.05 \cdot N_{\text{nets}} \cdot (W_{\text{core}} + H_{\text{core}})$, $\Delta\text{HPWL}_{\text{ref}} = \max\bigl(50.0,\; 0.015 \cdot \max(\text{HPWL}(u_k),\; \text{hpwl\_est})\bigr)$, and two-phase upper multiplier $\mu_{\max} = 1.090$ when $\tau_k > 0.70$ else $1.055$:
     $$\mu_k = \text{clamp}\!\left(\mu_{\max}^{\,1.0 - \Delta\text{HPWL} / \Delta\text{HPWL}_{\text{ref}}},\; 0.95,\; \mu_{\max}\right), \quad \lambda_{k+1} = \mu_k \, \lambda_k$$
   - Stop when overflow $\tau_k \le 0.10$ (or at `max_iters = 300`).

#### Concrete Structs & Signatures (`crates/grus-core/src/placement/nesterov.rs`)
```rust
#[derive(Clone, Debug)]
pub struct NesterovConfig {
    pub target_density: f32,
    pub stop_overflow: f32,
    pub max_iters: usize,
    pub grid_m_override: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct NesterovResult {
    pub cell_x_um: Vec<f32>,
    pub cell_y_um: Vec<f32>,
    pub iterations: usize,
    pub initial_overflow: f32,
    pub final_overflow: f32,
    pub initial_hpwl_um: f64,
    pub final_hpwl_um: f64,
}

pub fn run_nesterov_placement(
    design_db: &DesignDb,
    tech_db: &TechDb,
    config: &NesterovConfig,
) -> NesterovResult
```

---

### Step E2.4: Abacus Legalizer & Local Refinement (`legalize.rs`)

After Nesterov finishes, snap every movable standard cell to a valid, non-overlapping `PlacementRow` site in `i32` DBU, avoiding all `Fixed` cells (such as tapcells and endcap decaps), and refine local cell ordering to minimize exact DBU HPWL:

```rust
#[derive(Clone, Debug)]
struct AbacusCluster {
    x_dbu: i32,         // left edge of cluster (snapped to site grid)
    width_dbu: i32,     // sum of cell widths in cluster
    weight_sum: f32,    // e_c = sum of weights (1.0 or pin count)
    q_sum: f32,         // q_c = sum of w_i * (x_i_gp - offset_i)
    cells: Vec<CellId>, // cells in left-to-right order
}

#[derive(Clone, Debug)]
pub struct LegalizeResult {
    pub avg_displacement_um: f64,
    pub max_displacement_um: f64,
    pub pre_legal_hpwl_dbu: i64,
    pub post_abacus_hpwl_dbu: i64,
    pub final_hpwl_dbu: i64,
    pub pass1_eval_fallbacks: usize,      // full net rescans by pass 1 trial evals
    pub refine_extrema_rebuilds: usize,   // full rescans by the shared extrema cache
}

pub fn legalize_placement(
    design_db: &mut DesignDb,
    tech_db: &TechDb,
    gp_x_um: &[f32],
    gp_y_um: &[f32],
) -> anyhow::Result<LegalizeResult>
```

#### Algorithm (Abacus Single-Row Dynamic Clustering + Local Detailed Placement Refinement)
1. **Row Sub-Segments (`SubRow`)**:
   - Sort `PlacementRow` indices by `orig_y_dbu` ascending.
   - For each `PlacementRow` $r$ (`[row_xl, row_xh] = [r.orig_x_dbu, r.orig_x_dbu + r.num_x * r.step_x_dbu]`, $y \in [r.\text{orig\_y\_dbu},\; r.\text{orig\_y\_dbu} + h_{\text{site}})$):
     - Collect all non-movable (`Fixed` or `Cover`) cells $f$ whose vertical span `[cell_y_dbu[f], cell_y_dbu[f] + h_f)` overlaps $[r.\text{orig\_y\_dbu},\; r.\text{orig\_y\_dbu} + h_{\text{site}})$.
     - Snap each obstacle's horizontal span `[cell_x_dbu[f], cell_x_dbu[f] + w_f]` outward to $r$'s site grid (`r.orig_x_dbu + k * r.step_x_dbu`), clip to `[row_xl, row_xh]`, sort and merge overlapping obstacle intervals, and extract the complementary free `SubRow` intervals `[x_min_dbu, x_max_dbu]` with `x_max_dbu > x_min_dbu`.
     - Track remaining capacity `free_width = (x_max_dbu - x_min_dbu) - used_width` per sub-row.
2. **Sort Movable Cells**:
   - Convert global placement coordinates to DBU: `x_gp_dbu[i] = (gp_x_um[i] * dbu_per_um).round() as i32`, `y_gp_dbu[i] = (gp_y_um[i] * dbu_per_um).round() as i32`.
   - Sort all movable cells (`PlacementStatus::Placed` or `PlacementStatus::Unplaced` with `MacroClass::Core` or `MacroClass::CoreTie`) by `(x_gp_dbu[i], y_gp_dbu[i], i)` ascending.
3. **Trial Row Placement (Abacus)**:
   - For cell $i$, search sub-rows within $\pm 10$ row heights of $y_i^{\text{gp}}$ that have `free_width >= cell_width[i]` (falling back to all sub-rows with `free_width >= cell_width[i]` if none exist within $\pm 10$ rows).
   - For each candidate sub-row, simulate appending cell $i$ as a singleton cluster `(x = snap_to_site(x_i_gp, ...), width = w_i, e = 1.0, q = x_i_gp as f32)`:
     - **Collapse Loop**: While the last cluster `c_last` overlaps the preceding cluster `c_prev` (`c_prev.x_dbu + c_prev.width_dbu > c_last.x_dbu`):
       - Merge `c_last` into `c_prev`:
         ```rust
         c_prev.q_sum += c_last.q_sum - c_last.weight_sum * (c_prev.width_dbu as f32);
         c_prev.weight_sum += c_last.weight_sum;
         c_prev.width_dbu += c_last.width_dbu;
         c_prev.x_dbu = snap_to_site(
             (c_prev.q_sum / c_prev.weight_sum).round() as i32,
             sub_row.orig_x_dbu,
             sub_row.step_x_dbu,
             sub_row.x_min_dbu,
             sub_row.x_max_dbu - c_prev.width_dbu,
         );
         ```
   - Commit cell $i$ to the candidate sub-row that yields the minimum quadratic displacement cost $\Delta = (x_i^{\text{legal}} - x_i^{\text{gp}})^2 + (y_{\text{row}} - y_i^{\text{gp}})^2$ (where $x_i^{\text{legal}} = c_{\text{last}}.\text{x\_dbu} + c_{\text{last}}.\text{width\_dbu} - w_i$).
   - Unpack clusters in each sub-row left-to-right, setting `cell_x_dbu[c]`, `cell_y_dbu[c] = sub_row.orig_y_dbu`, `cell_orient[c] = sub_row.orient`, and `cell_status[c] = PlacementStatus::Placed`.
4. **Post-Abacus Local Detailed Placement Refinement (4 Rounds)**:
   - Because odd rows have `Orient::FS` (vertical pin flip) and fixed tapcells split rows into multiple sub-rows, run 4 deterministic passes over the legalized sub-rows using exact integer DBU net bounding boxes (`design_db.pin_pos_dbu(pin, tech_db)`). One shared per-net `NetExtrema` cache (the two smallest and two largest pin coordinates per axis, each tagged with its pin) is built once after Abacus and maintained exactly across every accepted move — a moved pin inserts its new coordinate into the top-2, and only a moved pin that held a maintained slot triggers a full rescan of that net — so the passes evaluate incident nets in O(moved pins) instead of O(net degree):
     1. **Equal-Width Cell Swaps**: For pairs of movable cells $(c_1, c_2)$ with identical width (`w[c_1] == w[c_2]`) within $\pm 4$ rows and $\pm 25\,000\text{ DBU}$ horizontally, swap their positions/orientations if the incident nets' exact DBU HPWL strictly decreases ($\Delta\text{HPWL} < 0$). The per-band index is keyed `(width, x_dbu, movable index, cell)`, so the candidate window scans same-width entries only.
     2. **Adjacent Sub-Row Cell Swaps**: In each `SubRow` with $k \ge 2$ cells, test swapping adjacent cells `(cells[j], cells[j + 1])` within their combined span (`new_xb = xa`, `new_xa = xa + wb` when `new_xa + wa <= xb + wb`), keeping the swap if incident nets' exact DBU HPWL strictly decreases.
     3. **Single-Cell Optimal Site Sliding**:
        - Within its current `SubRow`, slide cell $c_j$ inside its non-overlapping site bounds `[left_limit, right_limit]` to the site minimizing the incident nets' x-HPWL sum $f(dx) = \sum_n (\max(o_{hi}, c_{hi} + dx) - \min(o_{lo}, c_{lo} + dx))$ over the spans `(other_lo, other_hi, c_lo, c_hi)` of each incident net. `f` is a sum of convex piecewise-linear terms whose only kinks sit at $dx = o_{lo} - c_{lo}$ and $dx = o_{hi} - c_{hi}$, so the candidate set — `left_limit`, `right_limit`, and `snap_down`/`snap_up` of each kink target — contains the exact site-grid argmin (brute-force-gated by `test_pass3_candidates_hit_site_argmin`).
5. **Self-Verification (`verify_legality`) & CLI (`grus-eda place`)**:
   - Before returning `LegalizeResult`, verify that all movable cells have `PlacementStatus::Placed`, lie on valid `PlacementRow` $y$-coordinates and site-grid $x$-coordinates with matching `row.orient`, stay within row `[orig_x_dbu, orig_x_dbu + num_x * step_x_dbu]`, and have zero overlaps with any other movable or fixed cell.
   - Wire up `grus-eda place` in `crates/grus-eda/src/main.rs`:
     ```bash
     grus-eda place \
       --tlef <path.tlef> \
       --lef <path.lef> \
       --def <floorplan.def> \
       --out-def <placed.def> \
       [--target-density 0.60]
     ```
     which runs `run_nesterov_placement`, `legalize_placement`, and `write_def(&design_db, &tech_db, &def_path, &out_def_path)`.

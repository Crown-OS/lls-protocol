/// Ordinary least squares over a handful of `(x, y)` points, in `f64`.
///
/// Accumulates the four sums rather than storing points, so a fit over any subset costs one pass
/// and no allocation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LinearFit {
    count: u32,
    sum_x: f64,
    sum_y: f64,
    sum_xx: f64,
    sum_xy: f64,
}

impl LinearFit {
    pub(crate) const fn new() -> Self {
        Self {
            count: 0,
            sum_x: 0.0,
            sum_y: 0.0,
            sum_xx: 0.0,
            sum_xy: 0.0,
        }
    }

    pub(crate) fn add(&mut self, x: f64, y: f64) {
        self.count = self.count.saturating_add(1);
        self.sum_x += x;
        self.sum_y += y;
        self.sum_xx += x * x;
        self.sum_xy += x * y;
    }

    /// `(intercept, slope)`, or `None` until two distinct `x` values have been seen.
    pub(crate) fn solve(&self) -> Option<(f64, f64)> {
        if self.count < 2 {
            return None;
        }
        let count = f64::from(self.count);
        let spread = count * self.sum_xx - self.sum_x * self.sum_x;
        if spread <= f64::EPSILON * count * self.sum_xx {
            return None;
        }
        let slope = (count * self.sum_xy - self.sum_x * self.sum_y) / spread;
        let intercept = (self.sum_y - slope * self.sum_x) / count;
        Some((intercept, slope))
    }
}

#[cfg(test)]
mod tests {
    use super::LinearFit;

    #[test]
    fn a_line_is_recovered_exactly() {
        let mut fit = LinearFit::new();
        for x in 0..10 {
            let x = f64::from(x);
            fit.add(x, 3.0 + 0.5 * x);
        }
        let (intercept, slope) = fit.solve().unwrap_or_default();
        assert!((intercept - 3.0) * (intercept - 3.0) < 1e-12);
        assert!((slope - 0.5) * (slope - 0.5) < 1e-12);
    }

    #[test]
    fn one_point_or_one_x_has_no_slope() {
        let mut fit = LinearFit::new();
        fit.add(2.0, 1.0);
        assert!(fit.solve().is_none());
        fit.add(2.0, 5.0);
        assert!(fit.solve().is_none());
    }
}

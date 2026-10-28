//! Statistiche descrittive di base.

use serde::Serialize;

/// Distribuzione di un insieme di misure (campi `None` se non ci sono misure).
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Dist {
    pub n: usize,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
}

impl Dist {
    pub fn from_values(mut values: Vec<f64>) -> Self {
        values.retain(|v| v.is_finite());
        values.sort_by(f64::total_cmp);
        Dist {
            n: values.len(),
            mean: mean(&values),
            median: median(&values),
            p95: percentile_nearest_rank(&values, 95),
            max: values.last().copied(),
        }
    }
}

pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// Mediana di valori ordinati; con un numero pari di valori e' la media dei due centrali.
pub fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0),
    }
}

/// Percentile "nearest rank" su valori ordinati: indice `ceil(p/100 * n)` (base 1).
pub fn percentile_nearest_rank(sorted: &[f64], percent: usize) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (percent * sorted.len())
        .div_ceil(100)
        .clamp(1, sorted.len());
    Some(sorted[rank - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_odd_and_even_counts() {
        assert_eq!(median(&[1.0, 2.0, 9.0]), Some(2.0));
        assert_eq!(median(&[1.0, 2.0, 4.0, 10.0]), Some(3.0));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn p95_is_nearest_rank() {
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile_nearest_rank(&twenty, 95), Some(19.0));
        let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile_nearest_rank(&hundred, 95), Some(95.0));
        assert_eq!(percentile_nearest_rank(&[7.0], 95), Some(7.0));
        assert_eq!(percentile_nearest_rank(&[1.0, 2.0, 3.0], 95), Some(3.0));
        assert_eq!(percentile_nearest_rank(&[], 95), None);
    }

    #[test]
    fn dist_ignores_non_finite_values_and_orders_input() {
        let dist = Dist::from_values(vec![5.0, f64::NAN, 1.0, 3.0]);
        assert_eq!(dist.n, 3);
        assert_eq!(dist.mean, Some(3.0));
        assert_eq!(dist.median, Some(3.0));
        assert_eq!(dist.max, Some(5.0));
    }

    #[test]
    fn empty_dist_has_no_values() {
        assert_eq!(Dist::from_values(vec![]), Dist::default());
    }
}

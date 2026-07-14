use crate::SimulationResult;

// ---------------------------------------------------------------------------
// Fuel pricing — adaptive per-node fuel limits
// ---------------------------------------------------------------------------

/// Trait for computing optimal fuel limits per node based on predictions/history.
/// Allows granular fuel pricing instead of blanket doubling.
pub trait FuelPricer: Send + Sync {
    /// Given a node name, its current fuel limit, and the simulation prediction,
    /// return the recommended fuel limit for the next evolution.
    fn price_fuel(&self, node_name: &str, current_fuel: u64, prediction: &SimulationResult) -> u64;
}

/// Simple fuel pricer that doubles fuel when ResourceExhausted detected (legacy behavior).
pub struct SimpleFuelPricer;

impl FuelPricer for SimpleFuelPricer {
    fn price_fuel(
        &self,
        _node_name: &str,
        current_fuel: u64,
        _prediction: &SimulationResult,
    ) -> u64 {
        current_fuel * 2
    }
}

/// Adaptive fuel pricer using MonteCarlo estimated_cost.
///
/// Strategy:
/// - If prediction.estimated_cost > current_fuel: set fuel = estimated_cost * 1.5 (headroom)
/// - If prediction.estimated_cost <= current_fuel * 0.5: reduce fuel by 20% (avoid over-provision)
/// - Otherwise keep current fuel
pub struct AdaptiveFuelPricer {
    pub headroom_factor: f64,
    pub shrink_factor: f64,
    pub min_fuel: u64,
    pub max_fuel: u64,
}

impl Default for AdaptiveFuelPricer {
    fn default() -> Self {
        Self {
            headroom_factor: 1.5,
            shrink_factor: 0.8,
            min_fuel: 100_000,
            max_fuel: 10_000_000,
        }
    }
}

impl AdaptiveFuelPricer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_bounds(mut self, min: u64, max: u64) -> Self {
        self.min_fuel = min;
        self.max_fuel = max;
        self
    }
}

impl FuelPricer for AdaptiveFuelPricer {
    fn price_fuel(
        &self,
        _node_name: &str,
        current_fuel: u64,
        prediction: &SimulationResult,
    ) -> u64 {
        let estimated = prediction.estimated_cost;

        let new_fuel = if estimated > current_fuel {
            // Need more fuel: add headroom
            ((estimated as f64) * self.headroom_factor) as u64
        } else if estimated < current_fuel / 2 {
            // Over-provisioned: shrink
            ((current_fuel as f64) * self.shrink_factor) as u64
        } else {
            current_fuel
        };

        new_fuel.clamp(self.min_fuel, self.max_fuel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pred(estimated_cost: u64) -> SimulationResult {
        SimulationResult {
            path_id: "t".into(),
            success_probability: 0.5,
            estimated_cost,
            predicted_outcome: "?".into(),
            predicted_effort: 0,
            predicted_stability: 1.0,
        }
    }

    #[test]
    fn test_simple_fuel_pricer() {
        let pricer = SimpleFuelPricer;
        assert_eq!(pricer.price_fuel("node1", 1000, &make_pred(100)), 2000);
        assert_eq!(pricer.price_fuel("node1", 5000, &make_pred(100)), 10000);
    }

    #[test]
    fn test_adaptive_fuel_pricer_increase() {
        let pricer = AdaptiveFuelPricer::new().with_bounds(100, 100_000);
        let new_fuel = pricer.price_fuel("node1", 1000, &make_pred(5000));
        assert!(new_fuel > 1000);
        assert_eq!(new_fuel, (5000.0 * 1.5) as u64); // 7500
    }

    #[test]
    fn test_adaptive_fuel_pricer_shrink() {
        let pricer = AdaptiveFuelPricer::new().with_bounds(100, 100_000);
        let new_fuel = pricer.price_fuel("node1", 10000, &make_pred(2000));
        assert!(new_fuel < 10000);
        assert_eq!(new_fuel, (10000.0 * 0.8) as u64); // 8000
    }

    #[test]
    fn test_adaptive_fuel_pricer_bounds() {
        let pricer = AdaptiveFuelPricer::new().with_bounds(500_000, 5_000_000);
        let pred_low = make_pred(100);
        let pred_high = make_pred(10_000_000);

        // Below min should clamp to min
        assert_eq!(pricer.price_fuel("node1", 100_000, &pred_low), 500_000);
        // Above max should clamp to max
        assert_eq!(pricer.price_fuel("node1", 1_000_000, &pred_high), 5_000_000);
    }
}

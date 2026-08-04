use crate::SimulationResult;

// ---------------------------------------------------------------------------
// 燃料定价 — 自适应的逐节点燃料上限
// ---------------------------------------------------------------------------

/// 基于预测/历史计算每节点最优燃料上限的 trait。
/// 允许细粒度燃料定价，而非一刀切加倍。
pub trait FuelPricer: Send + Sync {
    /// 给定节点名、当前燃料上限与仿真预测，
    /// 返回下一轮演进推荐的燃料上限。
    fn price_fuel(&self, node_name: &str, current_fuel: u64, prediction: &SimulationResult) -> u64;
}

/// 检测到资源耗尽时加倍燃料的简单定价器（旧版行为）。
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

/// 使用蒙特卡洛 estimated_cost 的自适应燃料定价器。
///
/// 策略：
/// - 若 prediction.estimated_cost > current_fuel：设 fuel = estimated_cost * 1.5（留余量）
/// - 若 prediction.estimated_cost <= current_fuel * 0.5：燃料减少 20%（避免过度供给）
/// - 否则维持当前燃料
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
            // 需要更多燃料：增加余量
            ((estimated as f64) * self.headroom_factor) as u64
        } else if estimated < current_fuel / 2 {
            // 过度供给：缩减
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

        // 低于下限应钳制到下限
        assert_eq!(pricer.price_fuel("node1", 100_000, &pred_low), 500_000);
        // 高于上限应钳制到上限
        assert_eq!(pricer.price_fuel("node1", 1_000_000, &pred_high), 5_000_000);
    }
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 资源类别定义，用于精细化控制。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Fuel,   // 代表计算开销
    Memory, // 代表内存消耗
}

/// 资源令牌桶，用于抢占式背压调度。
pub struct ResourceRegistry {
    /// 可用资源限额（例如：最大并发燃料数、内存上限）。
    pub limits: HashMap<ResourceType, u64>,
    /// 当前已分配的资源。
    allocated: Arc<Mutex<HashMap<ResourceType, u64>>>,
}

impl ResourceRegistry {
    pub fn new(fuel_limit: u64, memory_limit: u64) -> Self {
        let mut limits = HashMap::new();
        limits.insert(ResourceType::Fuel, fuel_limit);
        limits.insert(ResourceType::Memory, memory_limit);

        Self {
            limits,
            allocated: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 尝试抢占资源，如果成功返回 true，否则返回 false。
    pub fn try_acquire(&self, weights: &HashMap<ResourceType, u64>) -> bool {
        let mut allocated = self.allocated.lock().unwrap();

        // 校验：是否有足够的资源分配？
        for (r_type, cost) in weights {
            let current = *allocated.get(r_type).unwrap_or(&0);
            let limit = *self.limits.get(r_type).unwrap_or(&0);
            if current + cost > limit {
                return false; // 资源不足，拒绝抢占
            }
        }

        // 提交分配
        for (r_type, cost) in weights {
            *allocated.entry(*r_type).or_insert(0) += cost;
        }
        true
    }

    /// 释放资源。
    pub fn release(&self, weights: &HashMap<ResourceType, u64>) {
        let mut allocated = self.allocated.lock().unwrap();
        for (r_type, cost) in weights {
            if let Some(current) = allocated.get_mut(r_type) {
                *current = current.saturating_sub(*cost);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_resource_registry_concurrency() {
        let registry = Arc::new(ResourceRegistry::new(100, 100));
        let mut weights = HashMap::new();
        weights.insert(ResourceType::Fuel, 10);

        let mut handles = vec![];
        for _ in 0..10 {
            let reg = registry.clone();
            let w = weights.clone();
            handles.push(thread::spawn(move || reg.try_acquire(&w)));
        }

        let success_count =
            handles.into_iter().map(|h| h.join().unwrap()).filter(|&res| res).count();
        assert_eq!(
            success_count, 10,
            "All 10 threads should acquire 10 fuel each (total 100/100)"
        );

        // 第 11 个线程应失败
        assert!(
            !registry.try_acquire(&weights),
            "Should fail when over limit"
        );
    }
}

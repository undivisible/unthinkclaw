//! Cost tracking — token counting and billing for LLM calls
//! Phase 4 feature: Production billing support

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cost per 1M tokens (input/output separate)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
}

/// Token usage for a call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
}

impl TokenUsage {
    pub fn calculate_cost(&self, cost: &ModelCost) -> f64 {
        let input_cost = (self.input_tokens as f64 / 1_000_000.0) * cost.input_cost_per_1m;
        let output_cost = (self.output_tokens as f64 / 1_000_000.0) * cost.output_cost_per_1m;
        input_cost + output_cost
    }
}

/// Cost record for a single LLM call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {
    pub id: String,
    pub model: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub cost_usd: f64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Cost tracker (in-memory + SQLite persistence)
pub struct CostTracker {
    costs: Arc<RwLock<Vec<CostRecord>>>,
    models: Arc<RwLock<Vec<ModelCost>>>,
}

impl CostTracker {
    pub fn new() -> Self {
        let mut models = Vec::new();
        
        // Add standard model pricing (as of 2024)
        models.push(ModelCost {
            model: "claude-opus-4-6".to_string(),
            input_cost_per_1m: 15.0,
            output_cost_per_1m: 75.0,
        });
        models.push(ModelCost {
            model: "claude-3-5-sonnet-20241022".to_string(),
            input_cost_per_1m: 3.0,
            output_cost_per_1m: 15.0,
        });
        models.push(ModelCost {
            model: "gpt-4-turbo".to_string(),
            input_cost_per_1m: 10.0,
            output_cost_per_1m: 30.0,
        });
        models.push(ModelCost {
            model: "gpt-4".to_string(),
            input_cost_per_1m: 30.0,
            output_cost_per_1m: 60.0,
        });
        models.push(ModelCost {
            model: "gpt-3.5-turbo".to_string(),
            input_cost_per_1m: 0.5,
            output_cost_per_1m: 1.5,
        });
        models.push(ModelCost {
            model: "gemini-2.0-flash".to_string(),
            input_cost_per_1m: 0.075,
            output_cost_per_1m: 0.3,
        });
        
        Self {
            costs: Arc::new(RwLock::new(Vec::new())),
            models: Arc::new(RwLock::new(models)),
        }
    }
    
    /// Record a cost from an LLM call
    pub async fn record(&self, model: &str, usage: TokenUsage) -> anyhow::Result<()> {
        let models = self.models.read().await;
        let model_cost = models
            .iter()
            .find(|m| m.model == model)
            .cloned()
            .unwrap_or_else(|| ModelCost {
                model: model.to_string(),
                input_cost_per_1m: 0.0,
                output_cost_per_1m: 0.0,
            });
        
        let cost_usd = usage.calculate_cost(&model_cost);
        
        let record = CostRecord {
            id: uuid::Uuid::new_v4().to_string(),
            model: model.to_string(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_usd,
            timestamp: chrono::Utc::now(),
        };
        
        self.costs.write().await.push(record);
        Ok(())
    }
    
    /// Get cost summary
    pub async fn summary(&self) -> CostSummary {
        let costs = self.costs.read().await;
        
        let total_cost: f64 = costs.iter().map(|c| c.cost_usd).sum();
        let total_tokens: usize = costs.iter().map(|c| c.input_tokens + c.output_tokens).sum();
        
        let mut by_model: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        for cost in costs.iter() {
            *by_model.entry(cost.model.clone()).or_insert(0.0) += cost.cost_usd;
        }
        
        CostSummary {
            total_cost,
            total_tokens,
            by_model,
            call_count: costs.len(),
        }
    }
    
    /// Get cost history (with date filtering)
    pub async fn history(&self, days: usize) -> Vec<CostRecord> {
        let costs = self.costs.read().await;
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
        
        costs
            .iter()
            .filter(|c| c.timestamp > cutoff)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_cost: f64,
    pub total_tokens: usize,
    pub by_model: std::collections::HashMap<String, f64>,
    pub call_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_cost_calculation() {
        let cost = ModelCost {
            model: "test".to_string(),
            input_cost_per_1m: 1.0,
            output_cost_per_1m: 2.0,
        };
        
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            total_tokens: 2_000_000,
        };
        
        let calculated = usage.calculate_cost(&cost);
        assert_eq!(calculated, 3.0); // 1.0 + 2.0
    }

    #[tokio::test]
    async fn test_cost_tracking() {
        let tracker = CostTracker::new();
        
        tracker.record("claude-opus-4-6", TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        }).await.unwrap();
        
        let summary = tracker.summary().await;
        assert_eq!(summary.call_count, 1);
        assert!(summary.total_cost > 0.0);
    }
}

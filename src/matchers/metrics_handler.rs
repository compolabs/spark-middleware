use crate::matchers::types::MatcherResponse;
use crate::metrics::types::OrderMetrics;
use crate::middleware::aggregator::Aggregator;
use log::{error, info};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone)]
pub struct MetricsHandler {
    metrics: Arc<Mutex<OrderMetrics>>,
}

impl MetricsHandler {
    pub fn new(metrics: Arc<Mutex<OrderMetrics>>) -> Self {
        Self { metrics }
    }

    pub async fn process_matcher_response(
        &self,
        response: MatcherResponse,
        _aggregator: &Arc<Aggregator>,
        _sender: mpsc::Sender<String>,
        uuid: &str,
    ) {
        if response.success {
            let matched_count = response.orders.len() as u64;
            info!(
                "Matcher {} successfully processed {} orders",
                uuid,
                response.orders.len()
            );
            let mut metrics = self.metrics.lock().await;
            let remaining_count = metrics
                .total_remaining
                .saturating_sub(matched_count)
                .clone();
            metrics.update_metrics(matched_count, remaining_count);
        } else {
            error!("Matcher {} failed to process orders", uuid);
        }
    }
}

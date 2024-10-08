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
}

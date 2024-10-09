use crate::config::settings::Settings;
use crate::indexer::spot_order::SpotOrder;
use crate::middleware::order_pool::ShardedOrderPool;
use std::sync::Arc;

pub struct BatchProcessor {
    pub settings: Arc<Settings>,
}

impl BatchProcessor {
    pub fn new(settings: Arc<Settings>) -> Arc<Self> {
        Arc::new(Self { settings })
    }

    pub async fn form_batch(
        &self,
        batch_size: usize,
        order_pool: Arc<ShardedOrderPool>,
    ) -> Option<Vec<SpotOrder>> {
        let batch = order_pool.select_batch(batch_size).await;

        if batch.is_empty() {
            None
        } else {
            Some(batch)
        }
    }
}

use crate::middleware::manager::OrderManager;
use std::sync::Arc;

pub struct DataAggregator {
    managers: Vec<Arc<OrderManager>>,
}

impl DataAggregator {
    pub fn new(managers: Vec<Arc<OrderManager>>) -> Self {
        DataAggregator { managers }
    }

    pub async fn analyze(&self) {
        for (i, manager) in self.managers.iter().enumerate() {
            let buy_orders = manager.get_all_buy_orders().await;
            let sell_orders = manager.get_all_sell_orders().await;
            println!(
                "Indexer {}: Buy orders: {}, Sell orders: {}",
                i,
                buy_orders.len(),
                sell_orders.len()
            );
        }
    }

    pub async fn compare_response_times(&self) {}
}

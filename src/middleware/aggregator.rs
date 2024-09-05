use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::middleware::manager::OrderManager;
use crate::indexer::spot_order::SpotOrder;
use crate::indexer::spot_order::OrderType;
use log::info;

pub struct Aggregator {
    pub order_managers: HashMap<String, Arc<OrderManager>>,
    pub active_indexers: Vec<String>, 
}

impl Aggregator {
    pub fn new(order_managers: HashMap<String, Arc<OrderManager>>, active_indexers: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            order_managers,
            active_indexers,
        })
    }

    pub async fn get_aggregated_orders(&self, order_type: OrderType) -> Vec<SpotOrder> {
        if self.active_indexers.len() > 4 {
            self.get_orders_with_consensus(order_type).await
        } else {
          
            self.get_all_orders(order_type).await
        }
    }

    async fn get_orders_with_consensus(&self, order_type: OrderType) -> Vec<SpotOrder> {
        let mut order_counts: HashMap<String, u8> = HashMap::new();
        let mut order_map: HashMap<String, SpotOrder> = HashMap::new();

        for (indexer_name, manager) in &self.order_managers {
            if self.active_indexers.contains(indexer_name) {
                let orders = match order_type {
                    OrderType::Buy => manager.get_all_buy_orders().await,
                    OrderType::Sell => manager.get_all_sell_orders().await,
                };

                for order in orders {
                    let order_id = order.id.clone();
                    *order_counts.entry(order_id.clone()).or_insert(0) += 1;
                    order_map.insert(order_id, order);
                }
            }
        }

        order_counts
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .filter_map(|(order_id, _)| order_map.get(&order_id).cloned())
            .collect()
    }

    async fn get_all_orders(&self, order_type: OrderType) -> Vec<SpotOrder> {
        let mut aggregated_orders = vec![];

        for (indexer_name, manager) in &self.order_managers {
            if self.active_indexers.contains(indexer_name) {
                let orders = match order_type {
                    OrderType::Buy => manager.get_all_buy_orders().await,
                    OrderType::Sell => manager.get_all_sell_orders().await,
                };
                aggregated_orders.extend(orders);
            }
        }

        aggregated_orders
    }

    pub async fn log_aggregated_orders(&self, order_type: OrderType) {
        let aggregated_orders = self.get_aggregated_orders(order_type).await;
        info!("Aggregated {:?} Orders:", order_type);
        for order in &aggregated_orders {
            info!("{:?}", order);
        }
    }
}


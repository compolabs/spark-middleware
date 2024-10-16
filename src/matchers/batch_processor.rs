
use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::storage::order_book::OrderBook;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct BatchProcessor {
    pub matching_orders: Arc<Mutex<HashSet<String>>>, 
    pub order_book: Arc<OrderBook>,
}

impl BatchProcessor {
    pub fn new(order_book: Arc<OrderBook>) -> Self {
        BatchProcessor {
            matching_orders: Arc::new(Mutex::new(HashSet::new())),
            order_book,
        }
    }

    
    pub async fn form_batch(&self, batch_size: usize, order_type: OrderType) -> Vec<SpotOrder> {
        let mut available_orders = Vec::new();

        let matching_orders = self.matching_orders.lock().await;

        
        let orders = self.order_book.get_orders_in_range(0, u128::MAX, order_type);

        
        for order in orders {
            if !matching_orders.contains(&order.id) {
                available_orders.push(order);
            }
            if available_orders.len() >= batch_size {
                break;
            }
        }

        drop(matching_orders); 

        
        let mut matching_orders = self.matching_orders.lock().await;
        for order in &available_orders {
            matching_orders.insert(order.id.clone());
        }

        available_orders
    }

    
    pub async fn remove_from_matching(&self, orders: Vec<SpotOrder>) {
        let mut matching_orders = self.matching_orders.lock().await;
        for order in orders {
            matching_orders.remove(&order.id);
        }
    }
}

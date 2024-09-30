use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::middleware::order_pool::ShardedOrderPool;
use log::info;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum OrderManagerMessage {
    AddOrder(SpotOrder),
    RemoveOrder {
        order_id: String,
        price: u128,
        order_type: OrderType,
    },
    ClearAndAddOrders(Vec<SpotOrder>),
}

pub struct OrderManager {
    pub order_pool: Arc<ShardedOrderPool>,  
}

impl OrderManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            order_pool: ShardedOrderPool::new(),  
        })
    }

    pub async fn handle_message(&self, message: OrderManagerMessage) {
        match message {
            OrderManagerMessage::AddOrder(order) => {
                self.order_pool.add_order(order);  
            }
            OrderManagerMessage::RemoveOrder { order_id, price, order_type } => {
                self.order_pool.remove_order(&order_id, price, order_type);  
            }
            OrderManagerMessage::ClearAndAddOrders(orders) => {
                self.clear_and_add_orders(orders).await;
            }
        }
    }

    pub async fn add_order(&self, order: SpotOrder) {
        info!("Added new order: {:?}", order);
        self.order_pool.add_order(order);  
    }

    pub async fn remove_order(&self, order_id: &str, price: u128, order_type: OrderType) {
        self.order_pool.remove_order(order_id, price, order_type);  
        info!("Removed order with id: {}", order_id);
    }

    pub async fn clear_and_add_orders(&self, orders: Vec<SpotOrder>) {
        self.order_pool.clear_orders();  
        for order in orders {
            self.order_pool.add_order(order);  
        }
        info!("Cleared and added new orders");
    }

    pub async fn get_buy_orders_count(&self) -> usize {
        self.order_pool.get_best_buy_orders().len()  
    }

    pub async fn get_sell_orders_count(&self) -> usize {
        self.order_pool.get_best_sell_orders().len()  
    }

    pub async fn get_total_orders_count(&self) -> usize {
        self.get_buy_orders_count().await + self.get_sell_orders_count().await
    }

    pub async fn clear_orders(&self) {
        self.order_pool.clear_orders(); 
        info!("All orders have been cleared from OrderManager");
    }

    pub async fn get_orders(&self, price: u128, order_type: OrderType) -> Vec<SpotOrder> {
        self.order_pool.get_orders_by_price(price, order_type)  
    }

    pub async fn log_orders(&self) {
        let buy_orders = self.order_pool.get_best_buy_orders();  
        let sell_orders = self.order_pool.get_best_sell_orders();  

        info!("Current Buy Orders:");
        for order in buy_orders {
            info!("{:?}", order);
        }

        info!("Current Sell Orders:");
        for order in sell_orders {
            info!("{:?}", order);
        }
    }
}

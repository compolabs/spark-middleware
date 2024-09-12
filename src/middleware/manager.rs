use crate::indexer::spot_order::{OrderType, SpotOrder};
use log::info;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    pub buy_orders: RwLock<BTreeMap<u128, Vec<SpotOrder>>>,
    pub sell_orders: RwLock<BTreeMap<u128, Vec<SpotOrder>>>,
}

impl OrderManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buy_orders: RwLock::new(BTreeMap::new()),
            sell_orders: RwLock::new(BTreeMap::new()),
        })
    }

    pub async fn handle_message(&self, message: OrderManagerMessage) {
        match message {
            OrderManagerMessage::AddOrder(order) => self.add_order(order).await,
            OrderManagerMessage::RemoveOrder {
                order_id,
                price,
                order_type,
            } => self.remove_order(&order_id, price, order_type).await,
            OrderManagerMessage::ClearAndAddOrders(orders) => {
                self.clear_and_add_orders(orders).await;
            }
        }
    }

    pub async fn add_order(&self, order: SpotOrder) {
        let mut order_map = match order.order_type {
            OrderType::Buy => self.buy_orders.write().await,
            OrderType::Sell => self.sell_orders.write().await,
        };

        let orders = order_map.entry(order.price).or_default();

        if let Some(existing_order) = orders.iter_mut().find(|o| o.id == order.id) {
            *existing_order = order.clone();
            info!("Updated existing order: {:?}", existing_order);
        } else {
            orders.push(order.clone());
            info!("Added new order: {:?}", order);
        }
    }

    pub async fn remove_order(&self, order_id: &str, price: u128, order_type: OrderType) {
        let mut order_map = match order_type {
            OrderType::Buy => self.buy_orders.write().await,
            OrderType::Sell => self.sell_orders.write().await,
        };

        if let Some(orders) = order_map.get_mut(&price) {
            orders.retain(|order| order.id != order_id);
            if orders.is_empty() {
                order_map.remove(&price);
            }
            info!("Removed order with id: {}", order_id);
        }
    }

    pub async fn clear_and_add_orders(&self, orders: Vec<SpotOrder>) {
        println!("========================");
        println!("========================");
        println!("========================");
        println!("{:?}", orders);
        if !orders.is_empty() {
            self.clear_orders().await;
            for order in orders {
                self.add_order(order).await;
            }
        }
    }

    pub async fn clear_orders(&self) {
        let mut buy_orders = self.buy_orders.write().await;
        let mut sell_orders = self.sell_orders.write().await;
        buy_orders.clear();
        sell_orders.clear();
        info!("All orders have been cleared from OrderManager");
    }

    pub async fn get_orders(&self, price: u128, order_type: OrderType) -> Vec<SpotOrder> {
        let order_map = match order_type {
            OrderType::Buy => self.buy_orders.read().await,
            OrderType::Sell => self.sell_orders.read().await,
        };

        order_map.get(&price).cloned().unwrap_or_else(Vec::new)
    }

    pub async fn get_all_buy_orders(&self) -> Vec<SpotOrder> {
        let buy_orders = self.buy_orders.read().await;
        buy_orders.values().flatten().cloned().collect()
    }

    pub async fn get_all_sell_orders(&self) -> Vec<SpotOrder> {
        let sell_orders = self.sell_orders.read().await;
        sell_orders.values().flatten().cloned().collect()
    }

    pub async fn get_all_orders2(&self) -> (Vec<SpotOrder>, Vec<SpotOrder>) {
        let buy_orders = self.get_all_buy_orders().await;
        let sell_orders = self.get_all_sell_orders().await;
        (buy_orders, sell_orders)
    }

    pub async fn log_orders(&self) {
        let buy_orders = self.buy_orders.read().await;
        let sell_orders = self.sell_orders.read().await;

        info!("Current Buy Orders:");
        for (price, orders) in buy_orders.iter() {
            info!("Price: {} -> Orders: {:?}", price, orders);
        }

        info!("Current Sell Orders:");
        for (price, orders) in sell_orders.iter() {
            info!("Price: {} -> Orders: {:?}", price, orders);
        }
    }
}

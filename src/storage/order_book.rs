use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::indexer::spot_order::{OrderType, SpotOrder};

pub struct OrderBook {
    buy_orders: Arc<RwLock<BTreeMap<u128, Vec<SpotOrder>>>>,
    sell_orders: Arc<RwLock<BTreeMap<u128, Vec<SpotOrder>>>>,
}

impl OrderBook {
    pub fn new() -> Self {
        OrderBook {
            buy_orders: Arc::new(RwLock::new(BTreeMap::new())),
            sell_orders: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }


    pub fn add_order(&self, order: SpotOrder) {
        let mut target_tree = match order.order_type {
            OrderType::Buy => self.buy_orders.write().unwrap(),
            OrderType::Sell => self.sell_orders.write().unwrap(),
        };
        target_tree
            .entry(order.price)
            .or_insert(Vec::new())
            .push(order);
    }


    pub fn get_orders_in_range(
        &self,
        price_min: u128,
        price_max: u128,
        order_type: OrderType,
    ) -> Vec<SpotOrder> {
        let target_tree = match order_type {
            OrderType::Buy => self.buy_orders.read().unwrap(),
            OrderType::Sell => self.sell_orders.read().unwrap(),
        };
        let mut result = Vec::new();
        for (_price, order_list) in target_tree.range(price_min..=price_max) {
            result.extend(order_list.clone());
        }
        result
    }


    pub fn get_order(&self, id: &str, order_type: OrderType) -> Option<SpotOrder> {
        let target_tree = match order_type {
            OrderType::Buy => self.buy_orders.read().unwrap(),
            OrderType::Sell => self.sell_orders.read().unwrap(),
        };

        for (_price, order_list) in target_tree.iter() {
            if let Some(order) = order_list.iter().find(|o| o.id == id) {
                return Some(order.clone());
            }
        }
        None
    }


    pub fn update_order(&self, order: SpotOrder) {
        self.remove_order(&order.id, order.order_type);
        self.add_order(order);
    }


    pub fn remove_order(&self, id: &str, order_type: OrderType) {
        let mut target_tree = match order_type {
            OrderType::Buy => self.buy_orders.write().unwrap(),
            OrderType::Sell => self.sell_orders.write().unwrap(),
        };

        for (_price, order_list) in target_tree.iter_mut() {
            order_list.retain(|order| order.id != id);
        }
    }
}

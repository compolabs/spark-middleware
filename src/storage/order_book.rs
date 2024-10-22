use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::indexer::spot_order::{OrderType, SpotOrder};

pub struct OrderBook {
    buy_orders: Arc<RwLock<BTreeMap<u128, Vec<SpotOrder>>>>,
    sell_orders: Arc<RwLock<BTreeMap<u128, Vec<SpotOrder>>>>,
}

impl Default for OrderBook {
    fn default() -> Self {
        OrderBook {
            buy_orders: Arc::new(RwLock::new(BTreeMap::new())),
            sell_orders: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

impl OrderBook {
    pub fn new() -> Self {
        Self::default()
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

    pub fn get_buy_orders(&self) -> std::sync::RwLockReadGuard<BTreeMap<u128, Vec<SpotOrder>>> {
        self.buy_orders.read().unwrap()
    }

    pub fn get_sell_orders(&self) -> std::sync::RwLockReadGuard<BTreeMap<u128, Vec<SpotOrder>>> {
        self.sell_orders.read().unwrap()
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
        self.remove_order(&order.id, Some(order.order_type));
        self.add_order(order);
    }

    pub fn remove_order(&self, id: &str, order_type: Option<OrderType>) {
        match order_type {
            Some(order_type) => {
                let mut target_tree = match order_type {
                    OrderType::Buy => self.buy_orders.write().unwrap(),
                    OrderType::Sell => self.sell_orders.write().unwrap(),
                };
                self.remove_order_from_tree(&mut target_tree, id);
            }
            None => {
                {
                    let mut buy_tree = self.buy_orders.write().unwrap();
                    self.remove_order_from_tree(&mut buy_tree, id);
                }
                {
                    let mut sell_tree = self.sell_orders.write().unwrap();
                    self.remove_order_from_tree(&mut sell_tree, id);
                }
            }
        }
    }

    fn remove_order_from_tree(&self, target_tree: &mut BTreeMap<u128, Vec<SpotOrder>>, id: &str) {
        let mut empty_keys = Vec::new();

        for (&price, order_list) in target_tree.iter_mut() {
            order_list.retain(|order| order.id != id);
            if order_list.is_empty() {
                empty_keys.push(price);
            }
        }

        for key in empty_keys {
            target_tree.remove(&key);
        }
    }
}

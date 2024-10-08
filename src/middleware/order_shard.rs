use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use dashmap::DashMap;
use std::collections::BTreeSet;
use std::sync::Mutex;

pub struct OrderShard {
    buy_orders_by_price: DashMap<u128, DashMap<String, SpotOrder>>,
    sell_orders_by_price: DashMap<u128, DashMap<String, SpotOrder>>,
    buy_price_levels: Mutex<BTreeSet<u128>>,
    sell_price_levels: Mutex<BTreeSet<u128>>,
}

impl OrderShard {
    pub fn new() -> Self {
        Self {
            buy_orders_by_price: DashMap::new(),
            sell_orders_by_price: DashMap::new(),
            buy_price_levels: Mutex::new(BTreeSet::new()),
            sell_price_levels: Mutex::new(BTreeSet::new()),
        }
    }

    
    pub async fn add_order(&self, order: SpotOrder) {
        let price = order.price;
        let order_id = order.id.clone();

        match order.order_type {
            OrderType::Buy => {
                let price_level = self
                    .buy_orders_by_price
                    .entry(price)
                    .or_insert_with(DashMap::new);
                price_level.insert(order_id, order);
                let mut buy_price_levels = self.buy_price_levels.lock().unwrap();
                buy_price_levels.insert(price);
            }
            OrderType::Sell => {
                let price_level = self
                    .sell_orders_by_price
                    .entry(price)
                    .or_insert_with(DashMap::new);
                price_level.insert(order_id, order);
                let mut sell_price_levels = self.sell_price_levels.lock().unwrap();
                sell_price_levels.insert(price);
            }
        }
    }

    
    pub async fn remove_order(&self, order_id: &str, price: u128, order_type: OrderType) {
        match order_type {
            OrderType::Buy => {
                if let Some(price_level) = self.buy_orders_by_price.get_mut(&price) {
                    price_level.remove(order_id);
                    if price_level.is_empty() {
                        self.buy_orders_by_price.remove(&price);
                        let mut buy_price_levels = self.buy_price_levels.lock().unwrap();
                        buy_price_levels.remove(&price);
                    }
                }
            }
            OrderType::Sell => {
                if let Some(price_level) = self.sell_orders_by_price.get_mut(&price) {
                    price_level.remove(order_id);
                    if price_level.is_empty() {
                        self.sell_orders_by_price.remove(&price);
                        let mut sell_price_levels = self.sell_price_levels.lock().unwrap();
                        sell_price_levels.remove(&price);
                    }
                }
            }
        }
    }

    
    pub async fn update_order(
        &self,
        order_id: &str,
        price: u128,
        new_amount: u128,
        status: Option<OrderStatus>,
        order_type: OrderType,
    ) {
        match order_type {
            OrderType::Buy => {
                if let Some(price_level) = self.buy_orders_by_price.get_mut(&price) {
                    if let Some(mut order) = price_level.get_mut(order_id) {
                        if let Some(new_status) = status {
                            order.status = Some(new_status);
                        }
                        if new_amount > 0 {
                            order.amount = new_amount;
                        } else {
                            price_level.remove(order_id);
                        }
                    }
                }
            }
            OrderType::Sell => {
                if let Some(price_level) = self.sell_orders_by_price.get_mut(&price) {
                    if let Some(mut order) = price_level.get_mut(order_id) {
                        if let Some(new_status) = status {
                            order.status = Some(new_status);
                        }
                        if new_amount > 0 {
                            order.amount = new_amount;
                        } else {
                            price_level.remove(order_id);
                        }
                    }
                }
            }
        }
    }

    pub async fn update_order_status(
        &self,
        order_id: &str,
        price: u128,
        new_status: OrderStatus,
    ) {
        if let Some(price_level) = self.buy_orders_by_price.get_mut(&price) {
            if let Some(mut order) = price_level.get_mut(order_id) {
                order.status = Some(new_status);
            }
        }
        if let Some(price_level) = self.sell_orders_by_price.get_mut(&price) {
            if let Some(mut order) = price_level.get_mut(order_id) {
                order.status = Some(new_status);
            }
        }
    }

    pub fn get_best_buy_orders(&self) -> Vec<SpotOrder> {
        let buy_price_levels = self.buy_price_levels.lock().unwrap();
        let mut orders = Vec::new();
        for &price in buy_price_levels.iter().rev() {
            if let Some(price_level) = self.buy_orders_by_price.get(&price) {
                for order in price_level.iter() {
                    orders.push(order.value().clone());
                }
            }
        }
        orders
    }

    pub fn get_best_sell_orders(&self) -> Vec<SpotOrder> {
        let sell_price_levels = self.sell_price_levels.lock().unwrap();
        let mut orders = Vec::new();
        for &price in sell_price_levels.iter() {
            if let Some(price_level) = self.sell_orders_by_price.get(&price) {
                for order in price_level.iter() {
                    orders.push(order.value().clone());
                }
            }
        }
        orders
    }

    pub fn get_orders_by_price_and_time(
        &self,
        price: u128,
        _timestamp: u64,
        order_type: OrderType,
    ) -> Vec<SpotOrder> {
        match order_type {
            OrderType::Buy => {
                if let Some(price_level) = self.buy_orders_by_price.get(&price) {
                    price_level
                        .iter()
                        .map(|entry| entry.value().clone())
                        .collect()
                } else {
                    Vec::new()
                }
            }
            OrderType::Sell => {
                if let Some(price_level) = self.sell_orders_by_price.get(&price) {
                    price_level
                        .iter()
                        .map(|entry| entry.value().clone())
                        .collect()
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn clear_orders(&self) {
        self.buy_orders_by_price.clear();
        self.sell_orders_by_price.clear();
        let mut buy_price_levels = self.buy_price_levels.lock().unwrap();
        let mut sell_price_levels = self.sell_price_levels.lock().unwrap();
        buy_price_levels.clear();
        sell_price_levels.clear();
    }

    pub fn get_buy_price_range(&self) -> Option<(u128, u128)> {
        let buy_price_levels = self.buy_price_levels.lock().unwrap();
        if let (Some(min), Some(max)) = (
            buy_price_levels.iter().next(),
            buy_price_levels.iter().last(),
        ) {
            Some((*min, *max))
        } else {
            None
        }
    }

    pub fn get_sell_price_range(&self) -> Option<(u128, u128)> {
        let sell_price_levels = self.sell_price_levels.lock().unwrap();
        if let (Some(min), Some(max)) = (
            sell_price_levels.iter().next(),
            sell_price_levels.iter().last(),
        ) {
            Some((*min, *max))
        } else {
            None
        }
    }

    pub fn get_time_range(&self) -> Option<(u64, u64)> {
        None
    }

    pub fn get_buy_order_count(&self) -> usize {
        self.buy_orders_by_price
            .iter()
            .map(|entry| entry.value().len())
            .sum()
    }

    pub fn get_sell_order_count(&self) -> usize {
        self.sell_orders_by_price
            .iter()
            .map(|entry| entry.value().len())
            .sum()
    }

}

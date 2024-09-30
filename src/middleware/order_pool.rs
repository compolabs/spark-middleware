use dashmap::DashMap;
use std::sync::{Arc, Mutex};
use std::collections::BTreeSet;
use crate::indexer::spot_order::{SpotOrder, OrderType};

const NUM_SHARDS: usize = 8; 

pub struct ShardedOrderPool {
    
    shards: Vec<Arc<OrderShard>>,
}

pub struct OrderShard {
    buy_orders_by_price: DashMap<u128, DashMap<String, SpotOrder>>,  
    sell_orders_by_price: DashMap<u128, DashMap<String, SpotOrder>>, 
    buy_price_levels: Mutex<BTreeSet<u128>>,  
    sell_price_levels: Mutex<BTreeSet<u128>>, 
}

impl ShardedOrderPool {
    pub fn new() -> Arc<Self> {
        
        Arc::new(Self {
            shards: (0..NUM_SHARDS).map(|_| Arc::new(OrderShard::new())).collect(),
        })
    }

    fn get_shard(&self, price: u128) -> &Arc<OrderShard> {
        let shard_index = (price as usize) % NUM_SHARDS;
        &self.shards[shard_index]
    }

    pub fn add_order(&self, order: SpotOrder) {
        let shard = self.get_shard(order.price);
        shard.add_order(order);
    }

    pub fn remove_order(&self, order_id: &str, price: u128, order_type: OrderType) {
        let shard = self.get_shard(price);
        shard.remove_order(order_id, price, order_type);
    }

    pub fn get_best_buy_orders(&self) -> Vec<SpotOrder> {
        let mut all_orders = Vec::new();
        for shard in &self.shards {
            all_orders.extend(shard.get_best_buy_orders());
        }
        all_orders.sort_by(|a, b| b.price.cmp(&a.price)); 
        all_orders
    }

    pub fn get_best_sell_orders(&self) -> Vec<SpotOrder> {
        let mut all_orders = Vec::new();
        for shard in &self.shards {
            all_orders.extend(shard.get_best_sell_orders());
        }
        all_orders.sort_by(|a, b| a.price.cmp(&b.price)); 
        all_orders
    }

    pub fn clear_orders(&self) {
        for shard in &self.shards {
            shard.clear_orders();  
        }
    }

    pub fn get_orders_by_price(&self, price: u128, order_type: OrderType) -> Vec<SpotOrder> {
        let shard = self.get_shard(price); 
        shard.get_orders_by_price(price, order_type) 
    }
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

    pub fn add_order(&self, order: SpotOrder) {
        let price = order.price;
        let order_id = order.id.clone();

        match order.order_type {
            OrderType::Buy => {
                let price_level = self.buy_orders_by_price.entry(price).or_insert_with(DashMap::new);
                price_level.insert(order_id, order);
                let mut buy_price_levels = self.buy_price_levels.lock().unwrap();
                buy_price_levels.insert(price);
            }
            OrderType::Sell => {
                let price_level = self.sell_orders_by_price.entry(price).or_insert_with(DashMap::new);
                price_level.insert(order_id, order);
                let mut sell_price_levels = self.sell_price_levels.lock().unwrap();
                sell_price_levels.insert(price);
            }
        }
    }

    pub fn remove_order(&self, order_id: &str, price: u128, order_type: OrderType) {
        match order_type {
            OrderType::Buy => {
                if let Some(price_level) = self.buy_orders_by_price.get(&price) {
                    price_level.remove(order_id);
                    if price_level.is_empty() {
                        self.buy_orders_by_price.remove(&price);
                        let mut buy_price_levels = self.buy_price_levels.lock().unwrap();
                        buy_price_levels.remove(&price);
                    }
                }
            }
            OrderType::Sell => {
                if let Some(price_level) = self.sell_orders_by_price.get(&price) {
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

    pub fn clear_orders(&self) {
        self.buy_orders_by_price.clear();
        self.sell_orders_by_price.clear();
        let mut buy_price_levels = self.buy_price_levels.lock().unwrap();
        let mut sell_price_levels = self.sell_price_levels.lock().unwrap();
        buy_price_levels.clear();
        sell_price_levels.clear();
    }

    pub fn get_orders_by_price(&self, price: u128, order_type: OrderType) -> Vec<SpotOrder> {
        match order_type {
            OrderType::Buy => {
                if let Some(price_level) = self.buy_orders_by_price.get(&price) {
                    price_level.iter().map(|entry| entry.value().clone()).collect()
                } else {
                    Vec::new()
                }
            }
            OrderType::Sell => {
                if let Some(price_level) = self.sell_orders_by_price.get(&price) {
                    price_level.iter().map(|entry| entry.value().clone()).collect()
                } else {
                    Vec::new()
                }
            }
        }
    }
}

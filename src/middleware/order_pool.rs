use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use crate::matchers::types::MatcherOrderUpdate;
use crate::middleware::order_shard::OrderShard;
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct PriceTimeRange {
    pub min_price: u128,
    pub max_price: u128,
    pub min_time: u64,
    pub max_time: u64,
}

pub fn generate_price_time_ranges() -> Vec<PriceTimeRange> {
    let mut ranges = Vec::new();

    let price_ranges = vec![
        (0, 40_000 * 1_000_000),
        (40_000 * 1_000_000, 45_000 * 1_000_000),
        (45_000 * 1_000_000, 50_000 * 1_000_000),
        (50_000 * 1_000_000, 55_000 * 1_000_000),
        (55_000 * 1_000_000, 60_000 * 1_000_000),
        (60_000 * 1_000_000, 65_000 * 1_000_000),
        (65_000 * 1_000_000, 70_000 * 1_000_000),
        (70_000 * 1_000_000, u128::MAX),
    ];

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let time_interval = 4 * 60 * 60;

    for (min_price, max_price) in price_ranges {
        for i in 0..100 {
            let max_time = current_time.saturating_sub(i * time_interval);
            let min_time = current_time.saturating_sub((i + 1) * time_interval);
            ranges.push(PriceTimeRange {
                min_price,
                max_price,
                min_time,
                max_time,
            });
        }
    }

    ranges
}

#[derive(Serialize, JsonSchema)]
pub struct ShardDetails {
    pub shard_index: usize,
    pub buy_orders_count: usize,
    pub sell_orders_count: usize,
    pub buy_price_range: Option<(u128, u128)>,
    pub sell_price_range: Option<(u128, u128)>,
    pub time_range: Option<(u64, u64)>,
}

#[derive(Serialize, JsonSchema)]
pub struct ShardOrdersDetailedResponse {
    pub shard_details: Vec<ShardDetails>,
    pub total_buy_orders: usize,
    pub total_sell_orders: usize,
}

pub struct ShardedOrderPool {
    price_time_ranges: Vec<PriceTimeRange>,
    shards: Vec<Arc<OrderShard>>,
}

impl ShardedOrderPool {
    pub fn new(price_time_ranges: Vec<PriceTimeRange>) -> Arc<Self> {
        let num_shards = price_time_ranges.len();
        Arc::new(Self {
            price_time_ranges,
            shards: (0..num_shards)
                .map(|_| Arc::new(OrderShard::new()))
                .collect(),
        })
    }

    pub fn get_shard_by_price_and_time(&self, price: u128, time: u64) -> Option<&Arc<OrderShard>> {
        for (index, range) in self.price_time_ranges.iter().enumerate() {
            if price >= range.min_price
                && price <= range.max_price
                && time >= range.min_time
                && time <= range.max_time
            {
                return Some(&self.shards[index]);
            }
        }
        log::warn!(
            "No matching shard found for order with price {} and time {}",
            price,
            time
        );
        None
    }

    pub async fn add_order(&self, order: SpotOrder) {
        if let Some(shard) = self.get_shard_by_price_and_time(order.price, order.timestamp) {
            shard.add_order(order).await;
        } else {
            log::warn!(
                "Failed to add order with price {} and time {}",
                order.price,
                order.timestamp
            );
        }
    }

    pub async fn remove_order(
        &self,
        order_id: &str,
        price: u128,
        time: u64,
        order_type: OrderType,
    ) {
        if let Some(shard) = self.get_shard_by_price_and_time(price, time) {
            shard.remove_order(order_id, price, order_type).await;
        }
    }

    pub async fn update_order_status(&self, orders: Vec<MatcherOrderUpdate>) {
        for order_update in orders {
            let MatcherOrderUpdate {
                order_id,
                price,
                timestamp,
                new_amount,
                status,
                order_type,
            } = order_update;

            self.update_order_in_shard(&order_id, price, timestamp, new_amount, status, order_type)
                .await;
        }
    }

    pub async fn update_order_in_shard(
        &self,
        order_id: &str,
        price: u128,
        timestamp: u64,
        new_amount: u128,
        status: Option<OrderStatus>,
        order_type: OrderType,
    ) {
        if let Some(shard) = self.get_shard_by_price_and_time(price, timestamp) {
            shard
                .update_order(order_id, price, new_amount, status, order_type)
                .await;

            
            if let Some(new_status) = status {
                /*
                if new_status == OrderStatus::Filled || new_status == OrderStatus::Failed {
                    shard.remove_order(order_id, price, order_type).await;
                }*/
            }
        } else {
            log::warn!(
                "Shard not found for order_id {}, price {}, timestamp {}",
                order_id,
                price,
                timestamp
            );
        }
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
    /*
        pub async fn select_batches(&self, batch_size: usize) -> Vec<Batch> {
            let mut selected_batches = Vec::new();


            let mut all_buy_orders = Vec::new();
            let mut all_sell_orders = Vec::new();

            for shard in &self.shards {
                all_buy_orders.extend(shard.get_best_buy_orders());
                all_sell_orders.extend(shard.get_best_sell_orders());
            }


            all_buy_orders.sort_by(|a, b| b.price.cmp(&a.price));
            all_sell_orders.sort_by(|a, b| a.price.cmp(&b.price));

            let mut buy_index = 0;
            let mut sell_index = 0;

            while buy_index < all_buy_orders.len() && sell_index < all_sell_orders.len() {
                let mut current_buy_batch = Vec::new();
                let mut current_sell_batch = Vec::new();


                while current_buy_batch.len() < batch_size && buy_index < all_buy_orders.len() {
                    let buy_order = &all_buy_orders[buy_index];


                    let mut temp_sell_index = sell_index;

                    while current_sell_batch.len() < batch_size && temp_sell_index < all_sell_orders.len() {
                        let sell_order = &all_sell_orders[temp_sell_index];

                        if buy_order.price >= sell_order.price {
                            current_sell_batch.push(sell_order.clone());
                            temp_sell_index += 1;
                        } else {
                            break;
                        }
                    }

                    if !current_sell_batch.is_empty() {
                        current_buy_batch.push(buy_order.clone());
                        buy_index += 1;
                        sell_index = temp_sell_index;

                        if current_buy_batch.len() >= batch_size && current_sell_batch.len() >= batch_size {
                            break;
                        }
                    } else {

                        buy_index += 1;
                    }
                }

                if !current_buy_batch.is_empty() && !current_sell_batch.is_empty() {
                    let batch = Batch::new(current_buy_batch.clone(), current_sell_batch.clone());
                    selected_batches.push(batch);
                } else {

                    break;
                }
            }

            selected_batches
        }
    */
    pub fn clear_orders(&self) {
        for shard in &self.shards {
            shard.clear_orders();
        }
    }

    pub fn get_detailed_order_info_per_shard(&self) -> Vec<ShardDetails> {
        self.shards
            .iter()
            .enumerate()
            .map(|(index, shard)| ShardDetails {
                shard_index: index,
                buy_orders_count: shard.get_buy_order_count(),
                sell_orders_count: shard.get_sell_order_count(),
                buy_price_range: shard.get_buy_price_range(),
                sell_price_range: shard.get_sell_price_range(),
                time_range: shard.get_time_range(),
            })
            .collect()
    }

    pub fn get_orders_by_price_and_time(
        &self,
        price: u128,
        timestamp: u64,
        order_type: OrderType,
    ) -> Vec<SpotOrder> {
        if let Some(shard) = self.get_shard_by_price_and_time(price, timestamp) {
            shard.get_orders_by_price_and_time(price, timestamp, order_type)
        } else {
            Vec::new()
        }
    }

    pub async fn select_batch(&self, batch_size: usize) -> Vec<SpotOrder> {
        let mut selected_orders = Vec::new();

        
        for shard in &self.shards {
            
            let buy_orders = shard.get_best_buy_orders();
            let sell_orders = shard.get_best_sell_orders();

            
            let available_buy_orders: Vec<_> = buy_orders
                .into_iter()
                .filter(|order| order.status.unwrap_or(OrderStatus::New) == OrderStatus::New)
                .collect();

            let available_sell_orders: Vec<_> = sell_orders
                .into_iter()
                .filter(|order| order.status.unwrap_or(OrderStatus::New) == OrderStatus::New)
                .collect();

            
            

            
            for buy_order in available_buy_orders {
                if selected_orders.len() >= batch_size {
                    break;
                }

                
                if let Some(sell_order) = available_sell_orders
                    .iter()
                    .find(|sell_order| buy_order.price >= sell_order.price)
                {
                    selected_orders.push(buy_order.clone());
                    selected_orders.push(sell_order.clone());

                    
                    /*
                    self.update_order_status_internal(
                        &buy_order.id,
                        buy_order.price,
                        buy_order.timestamp,
                        OrderStatus::InProgress,
                    )
                    .await;
                    self.update_order_status_internal(
                        &sell_order.id,
                        sell_order.price,
                        sell_order.timestamp,
                        OrderStatus::InProgress,
                    )
                    .await;
                    */
                }
            }

            if selected_orders.len() >= batch_size {
                break;
            }
        }

        selected_orders
    }

    async fn update_order_status_internal(
        &self,
        order_id: &str,
        price: u128,
        timestamp: u64,
        new_status: OrderStatus,
    ) {
        if let Some(shard) = self.get_shard_by_price_and_time(price, timestamp) {
            shard.update_order_status(order_id, price, new_status).await;
        }
    }
}

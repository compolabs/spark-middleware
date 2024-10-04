use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use crate::matchers::batch_processor::Batch;
use crate::matchers::types::MatcherOrderUpdate;
use crate::middleware::order_shard::OrderShard;
use dashmap::DashMap;
use log::info;
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

            
            self.update_order_in_shard(
                &order_id,
                price,
                timestamp,
                new_amount,
                status,
                order_type,
            )
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
            shard.update_order(order_id, price, new_amount, status, order_type).await;
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

    pub async fn select_batches(&self, batch_size: usize) -> Vec<Batch> {
        let mut selected_batches = Vec::new();

        // Шаг 1: Собираем все ордера покупки и сортируем их по времени
        let mut all_buy_orders = self.get_all_buy_orders();
        all_buy_orders.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Собираем все ордера продажи и сортируем их по времени
        let mut all_sell_orders = self.get_all_sell_orders();
        all_sell_orders.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Храним оставшиеся объемы ордеров продажи
        let mut sell_order_remaining_amounts: DashMap<String, u128> = DashMap::new();
        for sell_order in &all_sell_orders {
            sell_order_remaining_amounts.insert(sell_order.id.clone(), sell_order.amount);
        }

        // Итерируемся по ордерам покупки
        for buy_order in &all_buy_orders {
            let mut remaining_buy_amount = buy_order.amount;

            // Хеш-таблицы для агрегации ордеров в батче
            let mut batch_buy_orders_map: HashMap<String, u128> = HashMap::new();
            let mut batch_sell_orders_map: HashMap<String, u128> = HashMap::new();

            // Итерируемся по ордерам продажи
            for sell_order in &all_sell_orders {
                // Проверяем, был ли ордер продажи полностью использован ранее
                if let Some(sell_remaining_amount) = sell_order_remaining_amounts.get_mut(&sell_order.id) {
                    if *sell_remaining_amount == 0 {
                        continue;
                    }

                    // Проверяем временную метку
                    if sell_order.timestamp > buy_order.timestamp {
                        continue;
                    }

                    // Проверяем цены
                    if sell_order.price > buy_order.price {
                        continue;
                    }

                    // Определяем объем для сопоставления
                    let match_amount = remaining_buy_amount.min(*sell_remaining_amount);

                    // Агрегируем объем для ордера покупки
                    *batch_buy_orders_map.entry(buy_order.id.clone()).or_insert(0) += match_amount;

                    // Агрегируем объем для ордера продажи
                    *batch_sell_orders_map.entry(sell_order.id.clone()).or_insert(0) += match_amount;

                    // Обновляем оставшиеся объемы
                    remaining_buy_amount -= match_amount;
                    *sell_remaining_amount -= match_amount;

                    // Если ордер покупки полностью сопоставлен, выходим из цикла
                    if remaining_buy_amount == 0 {
                        break;
                    }
                }
            }

            // Формируем батч, если удалось сопоставить хотя бы часть ордера
            if !batch_buy_orders_map.is_empty() && !batch_sell_orders_map.is_empty() {
                // Преобразуем хеш-таблицы в вектора ордеров с накопленными объемами
                let batch_buy_orders: Vec<SpotOrder> = batch_buy_orders_map
                    .iter()
                    .map(|(id, &amount)| {
                        let original_order = all_buy_orders.iter().find(|o| &o.id == id).unwrap();
                        SpotOrder {
                            amount,
                            ..original_order.clone()
                        }
                    })
                    .collect();

                let batch_sell_orders: Vec<SpotOrder> = batch_sell_orders_map
                    .iter()
                    .map(|(id, &amount)| {
                        let original_order = all_sell_orders.iter().find(|o| &o.id == id).unwrap();
                        SpotOrder {
                            amount,
                            ..original_order.clone()
                        }
                    })
                    .collect();

                let batch = Batch::new(batch_buy_orders, batch_sell_orders);
                selected_batches.push(batch);

                // Проверяем размер батча
                if selected_batches.len() >= batch_size {
                    break;
                }
            }
        }

        selected_batches
    }

    //BRUTE`
    pub async fn select_batches_brute(&self, batch_size: usize) -> Vec<Batch> {
        let mut selected_batches = Vec::new();

        // Шаг 1: Собираем все ордера
        let mut all_buy_orders = self.get_all_buy_orders();
        let mut all_sell_orders = self.get_all_sell_orders();

        // Шаг 2: Сортируем ордера по времени (от старых к новым)
        all_buy_orders.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        all_sell_orders.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Храним оставшиеся объемы ордеров продажи
        let mut sell_order_remaining_amounts: Vec<(usize, u128)> = all_sell_orders
            .iter()
            .enumerate()
            .map(|(i, order)| (i, order.amount))
            .collect();

        // Итерируемся по ордерам покупки
        let all_buy_len = all_buy_orders.len();
        let mut buy_i = 0;
        for buy_order in &all_buy_orders {
            info!("batching... buy {:?} from {:?}", &buy_i, &all_buy_len);
            buy_i +=1;
            let mut remaining_buy_amount = buy_order.amount;

            let mut batch_buy_orders = Vec::new();
            let mut batch_sell_orders = Vec::new();

            // Шаг 3: Перебираем ордера продажи
            let all_sell_len = all_sell_orders.len();
            for (sell_index, sell_order) in all_sell_orders.iter().enumerate() {
                //info!("batching... sell {:?} from {:?}", &sell_index, &all_sell_len);
                // Получаем оставшийся объем ордера продажи
                let (_, sell_remaining_amount) = &mut sell_order_remaining_amounts[sell_index];

                // Проверяем, не использован ли ордер полностью
                if *sell_remaining_amount == 0 {
                    continue;
                }

                // Проверяем временную метку (ордер продажи должен быть не новее ордера покупки)
                if sell_order.timestamp > buy_order.timestamp {
                    continue;
                }

                // Проверяем цены
                if sell_order.price > buy_order.price {
                    continue;
                }

                // Шаг 4: Определяем объем для сопоставления
                let match_amount = remaining_buy_amount.min(*sell_remaining_amount);

                // Создаем частичные ордера при необходимости
                let matched_buy_order = SpotOrder {
                    amount: match_amount,
                    ..buy_order.clone()
                };

                let matched_sell_order = SpotOrder {
                    amount: match_amount,
                    ..sell_order.clone()
                };

                batch_buy_orders.push(matched_buy_order);
                batch_sell_orders.push(matched_sell_order);

                // Обновляем оставшиеся объемы
                remaining_buy_amount -= match_amount;
                *sell_remaining_amount -= match_amount;

                // Если ордер покупки полностью сопоставлен, выходим из цикла
                if remaining_buy_amount == 0 {
                    break;
                }
            }

            // Шаг 5: Формируем батч, если удалось сопоставить хотя бы часть ордера
            if !batch_buy_orders.is_empty() && !batch_sell_orders.is_empty() {
                let batch = Batch::new(batch_buy_orders.clone(), batch_sell_orders.clone());
                selected_batches.push(batch);

                // Проверяем размер батча
                if selected_batches.len() >= batch_size {
                    break;
                }
            }
        }

        selected_batches
    }

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


    pub fn get_all_buy_orders(&self) -> Vec<SpotOrder> {
        let mut all_orders = Vec::new();
        for shard in &self.shards {
            all_orders.extend(shard.get_all_buy_orders());
        }
        all_orders
    }

    pub fn get_all_sell_orders(&self) -> Vec<SpotOrder> {
        let mut all_orders = Vec::new();
        for shard in &self.shards {
            all_orders.extend(shard.get_all_sell_orders());
        }
        all_orders
    }

}

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::matchers::websocket::MatcherResponse;
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
        if self.active_indexers.len() > 2 {
            self.get_orders_with_consensus(order_type).await
        } else {
          
            self.get_all_orders(order_type).await
        }
    }

    pub async fn get_all_aggregated_orders(&self) -> Vec<SpotOrder> {
        self.get_all_orders_without_type().await
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

    async fn get_all_orders_without_type(&self) -> Vec<SpotOrder> {
        let mut aggregated_orders = vec![];

        for (indexer_name, manager) in &self.order_managers {
            if self.active_indexers.contains(indexer_name) {
                let orders_buy = manager.get_all_buy_orders().await;
                let orders_sell = manager.get_all_sell_orders().await;
                aggregated_orders.extend(orders_buy);
                aggregated_orders.extend(orders_sell);
            }
        }

        aggregated_orders
    }

    async fn get_all_orders_without_type_tuple(&self) -> (Vec<SpotOrder>,Vec<SpotOrder>){
        let mut aggregated_buy_orders = vec![];
        let mut aggregated_sell_orders = vec![];

        for (indexer_name, manager) in &self.order_managers {
            if self.active_indexers.contains(indexer_name) {
                let orders_buy = manager.get_all_buy_orders().await;
                let orders_sell = manager.get_all_sell_orders().await;
                aggregated_buy_orders.extend(orders_buy);
                aggregated_sell_orders.extend(orders_sell);
            }
        }

        (aggregated_buy_orders, aggregated_sell_orders)
    }

    pub async fn log_aggregated_orders(&self, order_type: OrderType) {
        let aggregated_orders = self.get_aggregated_orders(order_type).await;
        info!("Aggregated {:?} Orders:", order_type);
        for order in &aggregated_orders {
            info!("{:?}", order);
        }
    }

    pub async fn process_matcher_response(
        &self,
        response: MatcherResponse,
    ) {
        if let MatcherResponse::MatchResult { success, matched_orders } = response {
            if success {
                self.remove_matched_orders(matched_orders).await;
            } else {
                info!("Matcher was unable to match the orders.");
            }
        }
    }

    pub async fn remove_matched_orders(&self, matched_order_ids: Vec<String>) {
        for manager in self.order_managers.values() {
            let mut buy_orders = manager.buy_orders.write().await;
            let mut sell_orders = manager.sell_orders.write().await;

            buy_orders.retain(|_, orders| {
                orders.retain(|order| !matched_order_ids.contains(&order.id));
                !orders.is_empty()
            });

            sell_orders.retain(|_, orders| {
                orders.retain(|order| !matched_order_ids.contains(&order.id));
                !orders.is_empty()
            });
        }
        info!("Matched orders removed from the collections.");
    }
    
    pub async fn select_matching_orders(&self) -> Vec<(SpotOrder, SpotOrder)> {
        let mut buy_queue = BinaryHeap::new();
        let mut sell_queue = BinaryHeap::new();

        {
            let (buy_orders, sell_orders) = self.get_all_orders_without_type_tuple().await;
            for order in buy_orders {
                buy_queue.push(order.clone()); 
            }
            for order in sell_orders {
                sell_queue.push(Reverse(order.clone())); 
            }
        }

        let mut matches = Vec::new();

        while let (Some(mut buy_order), Some(Reverse(mut sell_order))) = (buy_queue.pop(), sell_queue.pop()) {
            if buy_order.price >= sell_order.price {
                matches.push((buy_order.clone(), sell_order.clone()));

                if buy_order.amount > sell_order.amount {
                    buy_order.amount -= sell_order.amount;
                    buy_queue.push(buy_order);
                } else if sell_order.amount > buy_order.amount {
                    sell_order.amount -= buy_order.amount;
                    sell_queue.push(Reverse(sell_order));
                }
            } else {
                
                sell_queue.push(Reverse(sell_order));
            }
        }

        matches
    }


    pub async fn select_matching_batches(&self, batch_size: usize) -> Vec<Vec<(SpotOrder, SpotOrder)>> {
        let mut buy_queue = BinaryHeap::new();
        let mut sell_queue = BinaryHeap::new();

        {
            // Получаем все buy и sell ордера и добавляем их в соответствующие очереди
            let (buy_orders, sell_orders) = self.get_all_orders_without_type_tuple().await;
            for order in buy_orders {
                buy_queue.push(order.clone());
            }
            for order in sell_orders {
                sell_queue.push(Reverse(order.clone()));
            }
        }

        let mut matches = Vec::new(); // Хранение текущего батча
        let mut batches = Vec::new(); // Хранение всех батчей

        // Пока есть ордера в обеих очередях
        while let (Some(mut buy_order), Some(Reverse(mut sell_order))) = (buy_queue.pop(), sell_queue.pop()) {
            // Проверяем, может ли buy ордер быть сматчен с sell ордером
            if buy_order.price >= sell_order.price {
                // Добавляем ордера в текущий батч
                matches.push((buy_order.clone(), sell_order.clone()));

                // Обновляем количество сматченных ордеров
                if buy_order.amount > sell_order.amount {
                    buy_order.amount -= sell_order.amount;
                    buy_queue.push(buy_order);
                } else if sell_order.amount > buy_order.amount {
                    sell_order.amount -= buy_order.amount;
                    sell_queue.push(Reverse(sell_order));
                }

                // Если батч полон, добавляем его в список и начинаем новый
                if matches.len() == batch_size {
                    batches.push(matches);
                    matches = Vec::new();
                }
            } else {
                // Если нельзя сматчить, возвращаем sell ордер обратно в очередь
                sell_queue.push(Reverse(sell_order));
            }
        }

        // Если были найдены сматченные ордера, добавляем последние матчи в батч
        if !matches.is_empty() {
            batches.push(matches);
        }

        // Если не было ни одного матча, возвращаем пустое множество
        if batches.is_empty() {
            info!("No matching orders found.");
        }


        info!("====================================");
        for i in &batches {
            info!("batch {:?}", i);
        };
        info!("====================================");

        batches
    }

}


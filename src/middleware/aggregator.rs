use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::middleware::manager::OrderManager;
use log::info;
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

pub struct Aggregator {
    pub order_managers: HashMap<String, Arc<OrderManager>>,
    pub active_indexers: Vec<String>,
}

impl Aggregator {
    pub fn new(
        order_managers: HashMap<String, Arc<OrderManager>>,
        active_indexers: Vec<String>,
    ) -> Arc<Self> {
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

    pub async fn get_all_aggregated_buy_orders(&self) -> Vec<SpotOrder> {
        self.get_all_orders(OrderType::Buy).await
    }

    pub async fn get_all_aggregated_sell_orders(&self) -> Vec<SpotOrder> {
        self.get_all_orders(OrderType::Sell).await
    }

    async fn get_orders_with_consensus(&self, order_type: OrderType) -> Vec<SpotOrder> {
        let mut order_counts: HashMap<String, u8> = HashMap::new();
        let mut order_map: HashMap<String, SpotOrder> = HashMap::new();

        for (indexer_name, manager) in &self.order_managers {
            if self.active_indexers.contains(indexer_name) {
                let orders = match order_type {
                    OrderType::Buy => manager.order_pool.get_best_buy_orders(),
                    OrderType::Sell => manager.order_pool.get_best_sell_orders(),
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
                    OrderType::Buy => manager.order_pool.get_best_buy_orders(),
                    OrderType::Sell => manager.order_pool.get_best_sell_orders(),
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
                let buy_orders = manager.order_pool.get_best_buy_orders();
                let sell_orders = manager.order_pool.get_best_sell_orders();
                aggregated_orders.extend(buy_orders);
                aggregated_orders.extend(sell_orders);
            }
        }

        aggregated_orders
    }

    async fn get_all_orders_without_type_tuple(&self) -> (Vec<SpotOrder>, Vec<SpotOrder>) {
        let mut aggregated_buy_orders = vec![];
        let mut aggregated_sell_orders = vec![];

        for (indexer_name, manager) in &self.order_managers {
            if self.active_indexers.contains(indexer_name) {
                let buy_orders = manager.order_pool.get_best_buy_orders();
                let sell_orders = manager.order_pool.get_best_sell_orders();
                aggregated_buy_orders.extend(buy_orders);
                aggregated_sell_orders.extend(sell_orders);
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

    pub async fn remove_matched_orders(&self, matched_order_ids: Vec<String>) {
        for manager in self.order_managers.values() {
            manager.order_pool.clear_orders();
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

        while let (Some(mut buy_order), Some(Reverse(mut sell_order))) =
            (buy_queue.pop(), sell_queue.pop())
        {
            match buy_order.price.cmp(&sell_order.price) {
                Ordering::Greater | Ordering::Equal => {
                    matches.push((buy_order.clone(), sell_order.clone()));

                    match buy_order.amount.cmp(&sell_order.amount) {
                        Ordering::Greater => {
                            buy_order.amount -= sell_order.amount;
                            buy_queue.push(buy_order);
                        }
                        Ordering::Less => {
                            sell_order.amount -= buy_order.amount;
                            sell_queue.push(Reverse(sell_order));
                        }
                        Ordering::Equal => {}
                    }
                }
                Ordering::Less => {
                    sell_queue.push(Reverse(sell_order));
                }
            }
        }

        matches
    }

    pub async fn select_matching_batches(
        &self,
        batch_size: usize,
    ) -> Vec<Vec<(SpotOrder, SpotOrder)>> {
        let mut buy_queue = BinaryHeap::new();
        let mut sell_queue = BinaryHeap::new();

        let (buy_orders, sell_orders) = self.get_all_orders_without_type_tuple().await;

        info!(
            "Found {} buy orders and {} sell orders for matching",
            buy_orders.len(),
            sell_orders.len()
        );

        buy_queue.extend(buy_orders.into_iter());
        sell_queue.extend(sell_orders.into_iter().map(Reverse));

        let mut matches = Vec::with_capacity(batch_size);
        let mut batches = Vec::new();

        while let (Some(mut buy_order), Some(Reverse(mut sell_order))) =
            (buy_queue.pop(), sell_queue.pop())
        {
            if buy_order.price >= sell_order.price {
                matches.push((buy_order.clone(), sell_order.clone()));

                match buy_order.amount.cmp(&sell_order.amount) {
                    Ordering::Greater => {
                        buy_order.amount -= sell_order.amount;
                        buy_queue.push(buy_order);
                    }
                    Ordering::Less => {
                        sell_order.amount -= buy_order.amount;
                        sell_queue.push(Reverse(sell_order));
                    }
                    Ordering::Equal => {}
                }

                if matches.len() == batch_size {
                    batches.push(std::mem::take(&mut matches));
                }
            } else {
                info!(
                    "No match for buy order at price {} and sell order at price {}",
                    buy_order.price, sell_order.price
                );
                sell_queue.push(Reverse(sell_order));
            }
        }

        if !matches.is_empty() {
            batches.push(matches);
        }
        batches
    }
}

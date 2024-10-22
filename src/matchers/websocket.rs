use crate::config::settings::Settings;
use crate::indexer::spot_order::SpotOrder;
use crate::matchers::types::{MatcherRequest, MatcherResponse};
use crate::storage::order_book::OrderBook;
use futures_util::{SinkExt, StreamExt};
use log::{info,error};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;

use super::types::{MatcherConnectRequest, MatcherOrderUpdate};

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
    pub order_book: Arc<OrderBook>,
    pub matching_orders: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

impl MatcherWebSocket {
    pub fn new(settings: Arc<Settings>, order_book: Arc<OrderBook>) -> Self {
        Self {
            settings,
            order_book,
            matching_orders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn handle_connection(self: Arc<Self>, ws_stream: WebSocketStream<TcpStream>) {
        let (mut write, mut read) = ws_stream.split();

        let mut matcher_uuid: Option<String> = None;

        while let Some(Ok(message)) = read.next().await {
            if let Message::Text(text) = message {
                match serde_json::from_str::<MatcherRequest>(&text) {
                    Ok(MatcherRequest::BatchRequest(_)) => {
                        if let Some(uuid) = &matcher_uuid {
                            self.handle_batch_request(&mut write, uuid.clone()).await;
                        }
                    }
                    Ok(MatcherRequest::OrderUpdates(order_updates)) => {
                        if let Some(uuid) = &matcher_uuid {
                            self.handle_order_updates(order_updates, uuid.clone()).await;
                        }
                    }
                    Ok(MatcherRequest::Connect(MatcherConnectRequest { uuid })) => {
                        matcher_uuid = Some(uuid.clone());
                        // Инициализируем matching_orders для нового матчера
                        let mut matching_orders = self.matching_orders.lock().await;
                        matching_orders
                            .entry(uuid.clone())
                            .or_insert_with(HashSet::new);
                    }
                    _ => {
                        error!("{:?}", text);
                        error!("Invalid message format received");
                    }
                }
            }
        }
    }

    async fn handle_batch_request(
        &self,
        write: &mut futures_util::stream::SplitSink<WebSocketStream<TcpStream>, Message>,
        uuid: String,
    ) {
        let batch_size = self.settings.matchers.batch_size;
        let available_orders = self.get_available_orders(batch_size, &uuid).await;

        let response = if available_orders.is_empty() {
            MatcherResponse::NoOrders
        } else {
            MatcherResponse::Batch(available_orders)
        };

        let response_text = serde_json::to_string(&response).unwrap();

        if let Err(e) = write.send(Message::Text(response_text)).await {
            error!("Failed to send response to matcher: {}", e);
        }
    }

    async fn handle_order_updates(&self, order_updates: Vec<MatcherOrderUpdate>, uuid: String) {
        info!("=======================");
        info!("order updates {:?}", order_updates);
        info!("=======================");

        let mut matching_orders = self.matching_orders.lock().await;
        if let Some(order_ids) = matching_orders.get_mut(&uuid) {
            for update in order_updates {
                order_ids.remove(&update.order_id);
                info!(
                    "Order {} removed from matching_orders for matcher {}",
                    update.order_id, uuid
                );
            }
        } else {
            info!("No matching orders found for matcher {}", uuid);
        }
    }

    async fn get_available_orders(&self, batch_size: usize, uuid: &str) -> Vec<SpotOrder> {
        let mut available_orders = Vec::new();

        let matching_order_ids = {
            let matching_orders = self.matching_orders.lock().await;
            matching_orders.get(uuid).cloned().unwrap_or_default()
        };

        let buy_orders = {
            let buy_orders_lock = self.order_book.get_buy_orders();
            buy_orders_lock
                .values()
                .flat_map(|orders| orders.iter().cloned())
                .collect::<Vec<_>>()
        };

        let sell_orders = {
            let sell_orders_lock = self.order_book.get_sell_orders();
            sell_orders_lock
                .values()
                .flat_map(|orders| orders.iter().cloned())
                .collect::<Vec<_>>()
        };

        let mut buy_queue: Vec<SpotOrder> = buy_orders
            .into_iter()
            .filter(|order| !matching_order_ids.contains(&order.id))
            .collect();

        let mut sell_queue: Vec<SpotOrder> = sell_orders
            .into_iter()
            .filter(|order| !matching_order_ids.contains(&order.id))
            .collect();

        buy_queue.sort_by_key(|o| (std::cmp::Reverse(o.price), o.timestamp));
        sell_queue.sort_by_key(|o| (o.price, o.timestamp));

        let mut buy_index = 0;
        let mut sell_index = 0;
        let mut new_matching_order_ids = Vec::new();

        while buy_index < buy_queue.len()
            && sell_index < sell_queue.len()
            && available_orders.len() < batch_size
        {
            let buy_order = &buy_queue[buy_index];
            let sell_order = &sell_queue[sell_index];

            if buy_order.price >= sell_order.price {
                let trade_amount = std::cmp::min(buy_order.amount, sell_order.amount);

                let mut matched_buy_order = buy_order.clone();
                matched_buy_order.amount = trade_amount;

                let mut matched_sell_order = sell_order.clone();
                matched_sell_order.amount = trade_amount;

                available_orders.push(matched_buy_order);
                available_orders.push(matched_sell_order);

                new_matching_order_ids.push(buy_order.id.clone());
                new_matching_order_ids.push(sell_order.id.clone());

                buy_queue[buy_index].amount -= trade_amount;
                sell_queue[sell_index].amount -= trade_amount;

                if buy_queue[buy_index].amount == 0 {
                    buy_index += 1;
                }

                if sell_queue[sell_index].amount == 0 {
                    sell_index += 1;
                }

                if available_orders.len() >= batch_size {
                    break;
                }
            } else {
                buy_index += 1;
            }
        }

        {
            let mut matching_orders = self.matching_orders.lock().await;
            if let Some(matcher_orders) = matching_orders.get_mut(uuid) {
                for order_id in new_matching_order_ids {
                    matcher_orders.insert(order_id);
                }
            }
        }

        available_orders
    }

    pub async fn update_order(&self, order: SpotOrder) {
        //self.order_book.update_order(order.clone());

        let mut matching_orders = self.matching_orders.lock().await;
        matching_orders.remove(&order.id);
    }
}

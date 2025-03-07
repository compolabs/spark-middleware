use crate::config::env::ev;
use crate::indexer::spot_order::SpotOrder;
use crate::matchers::types::{MatcherRequest, MatcherResponse};
use crate::storage::order_storage::OrderStorage;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;
use tracing::debug;
use tracing::{error, info};

use super::types::{MatcherConnectRequest, MatcherOrderUpdate};

pub struct MatcherWebSocket {
    pub storage: Arc<OrderStorage>,
}

impl MatcherWebSocket {
    pub fn new(storage: Arc<OrderStorage>) -> Self {
        Self { storage }
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
                            debug!(
                                uuid=%uuid,
                                count=order_updates.len(),
                                "Received order updates request"
                            );
                            self.handle_order_updates(order_updates, uuid.clone()).await;
                        }
                    }
                    Ok(MatcherRequest::Connect(MatcherConnectRequest { uuid })) => {
                        matcher_uuid = Some(uuid.clone());
                        info!(uuid=%uuid, "Matcher connected");
                    }
                    _ => {
                        error!("Invalid message format received: {:?}", text);
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
        let batch_size = ev("BATCH_SIZE")
            .unwrap_or_else(|_| "25".into())
            .parse()
            .unwrap_or(10);

        let available_orders = self.get_available_orders(batch_size).await;

        let response = if available_orders.is_empty() {
            debug!(uuid=%uuid, "No orders in the batch. Sending NoOrders response.");
            MatcherResponse::NoOrders
        } else {
            debug!(
                uuid=%uuid,
                batch_size=available_orders.len(),
                "Sending batch of orders to matcher"
            );
            MatcherResponse::Batch(available_orders.clone())
        };

        let response_text = serde_json::to_string(&response).unwrap();
        if let Err(e) = write.send(Message::Text(response_text.into())).await {
            error!(uuid=%uuid, "Failed to serialize response: {}", e);
            for order in available_orders {
                self.storage.matching_orders.remove(&order.id);
            }
        }
    }

    async fn handle_order_updates(&self, order_updates: Vec<MatcherOrderUpdate>, uuid: String) {
        debug!(uuid=%uuid, count=order_updates.len(), "Processing order updates");
    }

    async fn get_available_orders(&self, batch_size: usize) -> Vec<SpotOrder> {
        let mut available_orders = Vec::new();

        let matching_order_ids = self.storage.matching_orders.get_all();

        let buy_orders = self
            .storage
            .order_book
            .get_buy_orders()
            .values()
            .flat_map(|orders| orders.iter().cloned())
            .filter(|order| !matching_order_ids.contains(&order.id))
            .collect::<Vec<_>>();

        let sell_orders = self
            .storage
            .order_book
            .get_sell_orders()
            .values()
            .flat_map(|orders| orders.iter().cloned())
            .filter(|order| !matching_order_ids.contains(&order.id))
            .collect::<Vec<_>>();

        let mut buy_queue = buy_orders;
        let mut sell_queue = sell_orders;
        buy_queue.sort_by_key(|o| (std::cmp::Reverse(o.price), o.timestamp));
        sell_queue.sort_by_key(|o| (o.price, o.timestamp));

        let mut buy_index = 0;
        let mut sell_index = 0;
        let mut new_matching_order_ids: Vec<String> = Vec::new();

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
                sell_index += 1;
            }
        }

        for order_id in new_matching_order_ids {
            self.storage.matching_orders.add(&order_id);
            debug!("Order {} added to matching_orders", order_id);
        }

        available_orders
    }
}

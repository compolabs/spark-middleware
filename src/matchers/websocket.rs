use crate::config::settings::Settings;
use crate::matchers::types::{MatcherRequest, MatcherResponse};
use crate::storage::order_book::OrderBook;
use crate::indexer::spot_order::{OrderType, SpotOrder};
use futures_util::{StreamExt, SinkExt};
use log::error;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;
use tokio::net::TcpStream;

use super::types::MatcherOrderUpdate;

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
    pub order_book: Arc<OrderBook>,
    pub matching_orders: Arc<Mutex<HashSet<String>>>,  
}

impl MatcherWebSocket {
    pub fn new(settings: Arc<Settings>, order_book: Arc<OrderBook>) -> Self {
        Self {
            settings,
            order_book,
            matching_orders: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn handle_connection(
        self: Arc<Self>,
        ws_stream: WebSocketStream<TcpStream>,
    ) {
        let (mut write, mut read) = ws_stream.split();

        
        while let Some(Ok(message)) = read.next().await {
            if let Message::Text(text) = message {
                match serde_json::from_str::<MatcherRequest>(&text) {
                    Ok(MatcherRequest::BatchRequest(_)) => {
                        self.handle_batch_request(&mut write).await;
                    }
                    Ok(MatcherRequest::OrderUpdates(order_updates)) => {
                        self.handle_order_updates(order_updates).await;
                    }
                    _ => {
                        error!("Invalid message format received");
                    }
                }
            }
        }
    }

    async fn handle_batch_request(
        &self,
        write: &mut futures_util::stream::SplitSink<
            WebSocketStream<TcpStream>,
            Message,
        >,
    ) {
        
        let batch_size = self.settings.matchers.batch_size;
        let available_orders = self.get_available_orders(batch_size).await;

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

    async fn handle_order_updates(
        &self,
        order_updates: Vec<MatcherOrderUpdate>,
    ) {
        
        for update in order_updates {
            let order = SpotOrder {
                id: update.order_id.clone(),
                amount: update.new_amount,
                price: update.price,
                order_type: update.order_type,
                timestamp: update.timestamp,
                status: update.status,
                user: String::new(), 
                asset: String::new(),
            };
            self.update_order(order).await;
        }
    }

    async fn get_available_orders(&self, batch_size: usize) -> Vec<SpotOrder> {
        let mut matching_orders = self.matching_orders.lock().await;
        let mut available_orders = Vec::new();

        let buy_orders = self.order_book.get_buy_orders();  
        let sell_orders = self.order_book.get_sell_orders();  

        let mut buy_iter = buy_orders.iter().rev();  
        let mut sell_iter = sell_orders.iter();      

        let mut buy_cursor = buy_iter.next();
        let mut sell_cursor = sell_iter.next();

        while let (Some((buy_price, buy_list)), Some((sell_price, sell_list))) = (buy_cursor, sell_cursor) {
            if *buy_price >= *sell_price {
                
                for buy_order in buy_list {
                    if matching_orders.contains(&buy_order.id) {
                        continue; 
                    }

                    for sell_order in sell_list {
                        if matching_orders.contains(&sell_order.id) {
                            continue; 
                        }

                        
                        available_orders.push(buy_order.clone());
                        available_orders.push(sell_order.clone());

                        matching_orders.insert(buy_order.id.clone());
                        matching_orders.insert(sell_order.id.clone());

                        if available_orders.len() >= batch_size {
                            return available_orders;
                        }
                    }
                }

                
                buy_cursor = buy_iter.next();
                sell_cursor = sell_iter.next();
            } else {
                
                buy_cursor = buy_iter.next();
            }
        }

        available_orders
    }


    pub async fn update_order(&self, order: SpotOrder) {
        self.order_book.update_order(order.clone());
        
        
        let mut matching_orders = self.matching_orders.lock().await;
        matching_orders.remove(&order.id);
    }
}

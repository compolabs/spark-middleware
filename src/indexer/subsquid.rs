use crate::indexer::spot_order::OrderType;
use crate::middleware::manager::OrderManagerMessage;
use crate::{
    error::Error,
    indexer::spot_order::{SpotOrder, SubsquidOrder},
    subscription::subsquid::format_graphql_subscription,
};
use async_tungstenite::tokio::{connect_async, TokioAdapter};
use async_tungstenite::tungstenite::protocol::Message;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use url::Url;

pub struct WebSocketClientSubsquid {
    pub url: Url,
}

impl WebSocketClientSubsquid {
    pub fn new(url: Url) -> Self {
        WebSocketClientSubsquid { url }
    }

    pub async fn connect(&self, sender: mpsc::Sender<OrderManagerMessage>) -> Result<(), Error> {
        loop {
            let mut ws_stream = match self.connect_to_ws().await {
                Ok(ws_stream) => ws_stream,
                Err(e) => {
                    error!("Failed to establish websocket connection: {:?}", e);
                    continue;
                }
            };

            info!("WebSocket connected to Subsquid");

            ws_stream
                .send(Message::Text(
                    r#"{"type": "connection_init", "payload": {}}"#.into(),
                ))
                .await
                .expect("Failed to send init message");

            info!("Connection init message sent, waiting for connection_ack...");

            while let Some(message) = ws_stream.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        if text.contains("\"type\":\"connection_ack\"") {
                            info!("Connection established, subscribing to orders...");

                            self.subscribe_to_orders(OrderType::Buy, &mut ws_stream)
                                .await?;
                            self.subscribe_to_orders(OrderType::Sell, &mut ws_stream)
                                .await?;
                        }

                        if text.contains("\"type\":\"next\"") {
                            let mut orders_to_add = Vec::new();

                            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(orders) =
                                    response["payload"]["data"]["activeBuyOrders"].as_array()
                                {
                                    for order in orders {
                                        let subsquid_order: SubsquidOrder =
                                            serde_json::from_value(order.clone()).unwrap();
                                        match SpotOrder::from_indexer_subsquid(subsquid_order) {
                                            Ok(spot_order) => {
                                                info!(
                                                    "Adding Buy Order to the list: {:?}",
                                                    spot_order
                                                );
                                                orders_to_add.push(spot_order);
                                            }
                                            Err(e) => {
                                                error!("Failed to parse Subsquid order: {:?}", e)
                                            }
                                        }
                                    }
                                }
                                if let Some(orders) =
                                    response["payload"]["data"]["activeSellOrders"].as_array()
                                {
                                    for order in orders {
                                        let subsquid_order: SubsquidOrder =
                                            serde_json::from_value(order.clone()).unwrap();
                                        match SpotOrder::from_indexer_subsquid(subsquid_order) {
                                            Ok(spot_order) => {
                                                info!(
                                                    "Adding Sell Order to the list: {:?}",
                                                    spot_order
                                                );
                                                orders_to_add.push(spot_order);
                                            }
                                            Err(e) => {
                                                error!("Failed to parse Subsquid order: {:?}", e)
                                            }
                                        }
                                    }
                                }

                                if !orders_to_add.is_empty() {
                                    sender
                                        .send(OrderManagerMessage::ClearAndAddOrders(orders_to_add))
                                        .await
                                        .map_err(|_| Error::OrderManagerSendError)?;
                                }
                            } else {
                                error!("Failed to deserialize WebSocket response: {:?}", text);
                            }
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            error!(
                                "WebSocket connection closed by server: code = {}, reason = {}",
                                frame.code, frame.reason
                            );
                        } else {
                            error!("WebSocket connection closed by server without a frame.");
                        }
                        break;
                    }
                    Err(e) => {
                        error!("Error in websocket connection: {:?}", e);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    async fn connect_to_ws(
        &self,
    ) -> Result<
        async_tungstenite::WebSocketStream<TokioAdapter<TcpStream>>,
        Box<dyn std::error::Error>,
    > {
        let url = self.url.to_string();

        let request = async_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(url)
            .header("Sec-WebSocket-Protocol", "graphql-transport-ws")
            .body(())
            .unwrap();

        let (ws_stream, _) = connect_async(request).await?;

        Ok(ws_stream)
    }

    async fn subscribe_to_orders(
        &self,
        order_type: OrderType,
        client: &mut async_tungstenite::WebSocketStream<TokioAdapter<TcpStream>>,
    ) -> Result<(), Error> {
        let subscription_query = format_graphql_subscription(order_type);
        let subscription_message = serde_json::json!({
            "id": format!("{}", order_type as u8),
            "type": "subscribe",
            "payload": {
                "query": subscription_query
            }
        });

        info!("Subscribing to {:?} orders...", order_type);

        client
            .send(Message::Text(subscription_message.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to send subscription message: {:?}", e);
                e
            })?;

        Ok(())
    }
}

use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use tokio::{net::TcpStream, sync::mpsc, time::Instant};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::{
    error::Error,
    indexer::spot_order::{OrderType, SpotOrder, WebSocketResponseEnvio},
    middleware::manager::OrderManagerMessage,
    subscription::envio::format_graphql_subscription,
};

pub struct WebSocketClientEnvio {
    pub url: Url,
}

impl WebSocketClientEnvio {
    pub fn new(url: Url) -> Self {
        WebSocketClientEnvio { url }
    }

    pub async fn connect(&self, sender: mpsc::Sender<OrderManagerMessage>) -> Result<(), Error> {
        loop {
            let mut initialized = false;
            let mut ws_stream = match self.connect_to_ws().await {
                Ok(ws_stream) => ws_stream,
                Err(e) => {
                    error!("Failed to establish websocket connection: {:?}", e);
                    continue;
                }
            };

            info!("WebSocket connected");

            ws_stream
                .send(Message::Text(r#"{"type": "connection_init"}"#.into()))
                .await
                .expect("Failed to send init message");

            let mut last_real_data_time = Instant::now(); 
            while let Some(message) = ws_stream.next().await {
                if Instant::now().duration_since(last_real_data_time)
                    > tokio::time::Duration::from_secs(20)
                {
                    error!("No real data received for 20 seconds, reconnecting...");
                    break;
                }
                
                match message {
                    Ok(Message::Text(text)) => {
                        if let Ok(response) = serde_json::from_str::<WebSocketResponseEnvio>(&text) {
                            match response.r#type.as_str() {
                                "ka" => {
                                    info!("Received keep-alive message.");
                                    continue; 
                                }
                                "connection_ack" => {
                                    if (!initialized) {
                                        info!("Connection established, subscribing to orders...");
                                        self.subscribe_to_orders(OrderType::Buy, &mut ws_stream)
                                            .await?;
                                        self.subscribe_to_orders(OrderType::Sell, &mut ws_stream)
                                            .await?;
                                        initialized = true;
                                    }
                                }
                                "data" => {
                                    last_real_data_time = Instant::now();

                                    if let Some(payload) = response.payload {
                                        if let Some(orders) = payload.data.active_buy_order {
                                            for order_indexer in orders {
                                                let spot_order =
                                                    SpotOrder::from_indexer_envio(order_indexer)?;
                                                sender
                                                    .send(OrderManagerMessage::AddOrder(spot_order))
                                                    .await
                                                    .map_err(|_| Error::OrderManagerSendError)?;
                                            }
                                        }
                                        if let Some(orders) = payload.data.active_sell_order {
                                            for order_indexer in orders {
                                                let spot_order =
                                                    SpotOrder::from_indexer_envio(order_indexer)?;
                                                sender
                                                    .send(OrderManagerMessage::AddOrder(spot_order))
                                                    .await
                                                    .map_err(|_| Error::OrderManagerSendError)?;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            error!("Failed to deserialize WebSocketResponse: {:?}", text);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error in websocket connection: {:?}", e);
                        break;
                    }
                }
            }

            self.unsubscribe_orders(&mut ws_stream, OrderType::Buy)
                .await?;
            self.unsubscribe_orders(&mut ws_stream, OrderType::Sell)
                .await?;

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    async fn connect_to_ws(
        &self,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Box<dyn std::error::Error>> {
        match connect_async(&self.url).await {
            Ok((ws_stream, response)) => {
                info!(
                    "WebSocket handshake has been successfully completed with response: {:?}",
                    response
                );
                Ok(ws_stream)
            }
            Err(e) => {
                error!("Failed to establish websocket connection: {:?}", e);
                Err(Box::new(e))
            }
        }
    }

    async fn subscribe_to_orders(
        &self,
        order_type: OrderType,
        client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> Result<(), Error> {
        let subscription_query = format_graphql_subscription(order_type);
        let start_msg = serde_json::json!({
            "id": format!("{}", order_type as u8),
            "type": "start",
            "payload": {
                "query": subscription_query
            }
        })
        .to_string();

        client.send(Message::Text(start_msg)).await.map_err(|e| {
            error!("Failed to send subscription: {:?}", e);
            e
        })?;
        Ok(())
    }

    async fn unsubscribe_orders(
        &self,
        client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        order_type: OrderType,
    ) -> Result<(), Error> {
        let stop_msg = serde_json::json!({
            "id": format!("{}", order_type as u8),
            "type": "stop"
        })
        .to_string();
        client.send(Message::Text(stop_msg)).await.map_err(|e| {
            error!("Failed to send unsubscribe message: {:?}", e);
            e
        })?;
        Ok(())
    }
}

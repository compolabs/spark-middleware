use async_trait::async_trait;
use chrono::NaiveDateTime;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::config::env::ev;
use crate::error::Error;
use crate::storage::order_storage::OrderStorage;
use crate::storage::order_book::OrderBook;
use crate::indexer::spot_order::{OrderType, SpotOrder, LimitType, OrderStatus};
use super::indexer::Indexer;
use crate::indexer::envio_event_handler::handle_envio_event;

pub struct EnvioIndexer {
    storage: Arc<OrderStorage>,
    client: Client,
}

impl EnvioIndexer {
    pub fn new(storage: Arc<OrderStorage>) -> Self {
        Self {
            storage,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Indexer for EnvioIndexer {
    async fn initialize(&self, tasks: &mut Vec<JoinHandle<()>>) -> Result<(), Error> {
        let storage = Arc::clone(&self.storage);
        let client = self.client.clone();

        let historical_task = tokio::spawn(async move {
            if let Err(e) = fetch_historical_data(&client, &storage).await {
                error!("Envio historical data fetch error: {}", e);
            }
        });
        tasks.push(historical_task);

        let storage_clone = Arc::clone(&self.storage);
        let client_clone = self.client.clone();
        
        let realtime_task = tokio::spawn(async move {
            if let Err(e) = listen_for_envio_events(&client_clone, &storage_clone).await {
                error!("Envio real-time event listener error: {}", e);
            }
        });
        tasks.push(realtime_task);
        
        Ok(())
    }
}

async fn fetch_historical_data(client: &Client, storage: &Arc<OrderStorage>) -> Result<(), Error> {
    let graphql_url = ev("ENVIO_HTTP_URL")?;
    let market = ev("CONTRACT_ID")?;

    let buy_query = format!(
        r#"{{ "query": "query ActiveBuyQuery {{ ActiveBuyOrder(where: {{market: {{_eq: \"{}\"}}}}) {{ id price amount user orderType limitType asset status timestamp }} }}" }}"#,
        market
    );
    
    let sell_query = format!(
        r#"{{ "query": "query ActiveSellQuery {{ ActiveSellOrder(where: {{market: {{_eq: \"{}\"}}}}) {{ id price amount user orderType limitType asset status timestamp }} }}" }}"#,
        market
    );
    
    let buy_orders = fetch_orders(client, &graphql_url, &buy_query).await?;
    let sell_orders = fetch_orders(client, &graphql_url, &sell_query).await?;
    
    for order in buy_orders {
        storage.order_book.add_order(order);
    }
    for order in sell_orders {
        storage.order_book.add_order(order);
    }
    
    info!("✅ Envio historical sync complete");
    Ok(())
}

async fn fetch_orders(client: &Client, url: &str, query: &str) -> Result<Vec<SpotOrder>, Error> {
    let response = client.post(url)
        .header("Content-Type", "application/json")
        .body(query.to_string())
        .send()
        .await.unwrap();
    
    let json: Value = response.json().await.unwrap();
    let orders = json["data"]["ActiveBuyOrder"].as_array()
        .or_else(|| json["data"]["ActiveSellOrder"].as_array())
        .ok_or_else(|| Error::Other("Invalid response format".into()))?;
    
    let mut result = Vec::new();
    for order in orders {
        let id = order["id"].as_str().unwrap_or_default().to_string();
        let price = order["price"].as_str().unwrap_or("0").parse::<u128>().unwrap_or(0);
        let amount = order["amount"].as_str().unwrap_or("0").parse::<u128>().unwrap_or(0);
        let user = order["user"].as_str().unwrap_or_default().to_string();
        let asset = order["asset"].as_str().unwrap_or_default().to_string();
        let order_type = match order["orderType"].as_str() {
            Some("Buy") => OrderType::Buy,
            Some("Sell") => OrderType::Sell,
            _ => continue,
        };
        let timestamp = order["timestamp"].as_str()
            .and_then(|ts| NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.3fZ").ok())
            .map(|dt| dt.timestamp() as u64)
            .unwrap_or(0);

        result.push(SpotOrder {
            id,
            user,
            asset,
            amount,
            price,
            timestamp,
            order_type,
            limit_type: None,
            status: Some(OrderStatus::New),
        });
    }
    
    Ok(result)
}

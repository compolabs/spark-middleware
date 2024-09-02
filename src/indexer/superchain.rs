use log::info;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use superchain_client::{
    core::types::fuel::OrderChangeType,
    futures::StreamExt,
    provider::FuelProvider,
    query::Bound,
    requests::fuel::{GetFuelBlocksRequest, GetSparkOrderRequest},
    Client, ClientBuilder, Format, WsProvider,
};
use tokio::sync::{mpsc, Mutex};
use tokio::time::Instant;

use crate::{config::env::ev, indexer::spot_order::SpotOrder};

#[derive(Debug)]
struct ReceivedOrder {
    order: SuperchainOrder,
    receive_time: u64, // UNIX timestamp
}

#[derive(Debug, Deserialize)]
struct SuperchainOrder {
    chain: u64,
    block_number: u64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: u64,
    log_index: u64,
    market_id: String,
    order_id: String,
    state_type: String,
    asset: String,
    amount: Option<u128>,
    asset_type: Option<String>,
    order_type: Option<String>,
    price: Option<u128>,
    user: Option<String>,
    order_matcher: Option<String>,
    owner: Option<String>,
    counterparty: Option<String>,
    match_size: Option<u128>,
    match_price: Option<u128>,
}

#[derive(Debug, Deserialize)]
struct BlockData {
    chain: u64,
    block_number: String,
    hash: String,
    parent_hash: String,
    da_block_number: String,
    transactions_root: String,
    transactions_count: u64,
    message_receipt_count: u64,
    message_outbox_root: String,
    event_inbox_root: String,
    timestamp: u64,
}

pub async fn start_superchain_indexer(
    sender: mpsc::Sender<SpotOrder>,
) -> Result<(), Box<dyn std::error::Error>> {
    let username = ev("SUPERCHAIN_USERNAME")?;
    let password = ev("SUPERCHAIN_PASSWORD")?;
    let _contract_id = ev("CONTRACT_ID_BTC_USDC")?;

    let mut h_s_state_types = HashSet::new();
    h_s_state_types.insert(OrderChangeType::Open);
    h_s_state_types.insert(OrderChangeType::Match);
    h_s_state_types.insert(OrderChangeType::Cancel);

    let client = ClientBuilder::default()
        .credential(&username, &password)
        .endpoint("fuel.beta.superchain.network")
        .build::<WsProvider>()
        .await?;

    info!("Superchain ws client created. Trying to connect");

    let request = GetSparkOrderRequest {
        from_block: Bound::FromLatest(3),
        to_block: Bound::None,
        order_type__in: h_s_state_types,
        ..Default::default()
    };

    info!("Superchain ws request: {:?}", request);

    let stream = client
        .get_fuel_spark_orders_by_format(request, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders");
    superchain_client::futures::pin_mut!(stream);

    let received_orders: Arc<Mutex<HashMap<String, ReceivedOrder>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let client_clone = ClientBuilder::default()
        .credential(username.clone(), password.clone())
        .endpoint("fuel.beta.superchain.network")
        .build::<WsProvider>()
        .await?;

    let mut start_time = Instant::now();
    let mut total_delay = 0u64;
    let mut total_orders = 0u64;

    // Основная обработка входящих ордеров
    while let Some(data) = stream.next().await {
        info!("===============");
        let data = data.unwrap();
        let data_str = String::from_utf8(data).unwrap();
        info!("data str {:?}",data_str);
        let superchain_order: SuperchainOrder = serde_json::from_str(&data_str)?;
        let order_id = &superchain_order.order_id.clone();
        let b_num = superchain_order.block_number.clone();

        // Записываем время получения данных в UNIX формате
        let receive_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        {
            let mut orders = received_orders.lock().await;
            orders.insert(
                superchain_order.order_id.clone(),
                ReceivedOrder {
                    order: superchain_order,
                    receive_time,
                },
            );
        }

        // Обработка метрик
        if let Ok(block_timestamp) = get_block_timestamp(&client_clone, b_num).await {
            let delay = if receive_time > block_timestamp {
                receive_time - block_timestamp
            } else {
                0
            };

            info!("Order ID: {:?} Delay: {} seconds", order_id, delay);

            total_delay += delay;
            total_orders += 1;
        }

        // Печать метрик каждые 5 минут
        if start_time.elapsed().as_secs() >= 30 {
            let average_delay = total_delay as f64 / total_orders as f64;
            let average_update_time = start_time.elapsed().as_secs() as f64 / total_orders as f64;

            info!("==================== METRICS ====================");
            info!(
                "Average WebSocket update time: {:.2} seconds",
                average_update_time
            );
            info!(
                "Average block to WebSocket time: {:.2} seconds",
                average_delay
            );
            info!("Total orders processed: {}", total_orders);
            info!("=================================================");

            // Сбрасываем таймер и метрики
            total_delay = 0;
            total_orders = 0;
            start_time = Instant::now();
        }
    }

    Ok(())
}

async fn get_block_timestamp(
    client: &Client<WsProvider>,
    block_number: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    let request = GetFuelBlocksRequest {
        from_block: Bound::Exact(block_number as i64),
        to_block: Bound::Exact(block_number as i64 + 1),
        ..Default::default()
    };

    let mut block_stream = client
        .get_fuel_blocks_by_format(request, Format::JsonStream, false)
        .await?;

    if let Some(block_data) = block_stream.next().await {
        let data_str = String::from_utf8(block_data?)?;
        let block_info: BlockData = serde_json::from_str(&data_str)?;
        info!("=========");
        info!("Block data: {:?}", block_info);
        info!("Block ts: {:?}", block_info.timestamp);
        info!("=========");
        Ok(block_info.timestamp)
    } else {
        Err("Failed to retrieve block timestamp".into())
    }
}

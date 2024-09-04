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

use crate::error::Error;
use crate::{config::env::ev, indexer::spot_order::SpotOrder};

#[derive(Debug)]
struct ReceivedOrder {
    order: SuperchainOrder,
    receive_time: u64, 
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

pub async fn start_superchain_indexer(
    sender: mpsc::Sender<SpotOrder>,
) -> Result<(), Error> {
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

    while let Some(data) = stream.next().await {
        let data = data.unwrap();
        let data_str = String::from_utf8(data).unwrap();
        let superchain_order: SuperchainOrder = serde_json::from_str(&data_str)?;
        let order_id = &superchain_order.order_id.clone();
        let b_num = superchain_order.block_number.clone();

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

        info!("Order ID: {:?} received at UNIX time: {}", order_id, receive_time);

    }

    Ok(())
}

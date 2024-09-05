use log::{info, error};
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

use crate::config::settings::Settings;
use crate::error::Error;
use crate::{config::env::ev, indexer::spot_order::SpotOrder};

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
    config: Settings
) -> Result<(), Error> {
    let username = config.websockets.superchain_username;
    let password = config.websockets.superchain_pass; 

    let mut h_s_state_types = HashSet::new();
    h_s_state_types.insert(OrderChangeType::Open);
    // ====================
    // NTD Not works at all
    //[2024-09-05T02:24:29Z ERROR spark_middleware::indexer::superchain] Error ErrorMsg("{\"status\":400,\"error\":\"unknown order type: Open\"}\n")


    let client = ClientBuilder::default()
        .credential(&username, &password)
        .endpoint("fuel.beta.superchain.network")
        .build::<WsProvider>()
        .await?;

    info!("Superchain ws client created. Trying to connect");

    let request = GetSparkOrderRequest {
        from_block: Bound::FromLatest(1000),
        to_block: Bound::None,
        ..Default::default()
    };

    let stream = client
        .get_fuel_spark_orders_by_format(request, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders");
    superchain_client::futures::pin_mut!(stream);

    while let Some(data) = stream.next().await {
        match data {
            Ok(valid_data) => {
                let data_str = String::from_utf8(valid_data).unwrap();
                let superchain_order: SuperchainOrder = serde_json::from_str(&data_str)?;
                info!("superchain order {:?}", superchain_order);
            }
            Err(e) => {
                error!("Error {:?}", e);
            }
        }
    }

    Ok(())
}

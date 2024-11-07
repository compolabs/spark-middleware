use ethers_core::types::H256;
use fuels::accounts::provider::Provider;
use log::{error, info};
use pangea_client::{ChainId, Client};
use pangea_client::{
    futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest,
    ClientBuilder, Format, WsProvider,
};
use tokio::time::{interval, sleep};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::config::env::ev;
use crate::error::Error;
use crate::indexer::order_event_handler::handle_order_event;
use crate::indexer::order_event_handler::PangeaOrderEvent;
use crate::storage::matching_orders::MatchingOrders;
use crate::storage::order_book::OrderBook;

pub async fn initialize_pangea_indexer(
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
    matching_orders: Arc<MatchingOrders>,
) -> Result<(), Error> {
    let ws_task_pangea = tokio::spawn(async move {
        if let Err(e) = start_pangea_indexer(order_book, matching_orders).await {
            eprintln!("Pangea error: {}", e);
        }
    });

    tasks.push(ws_task_pangea);
    Ok(())
}

async fn start_pangea_indexer(order_book: Arc<OrderBook>, matching_orders: Arc<MatchingOrders>) -> Result<(), Error> {
    let client = create_pangea_client().await?;

    let contract_start_block: i64 = ev("CONTRACT_START_BLOCK")?.parse()?;
    let contract_h256 = H256::from_str(&ev("CONTRACT_ID")?)?;

    let mut last_processed_block =
        fetch_historical_data(&client, &order_book, &matching_orders,
        contract_start_block, contract_h256).await?;

    if last_processed_block == 0 {
        last_processed_block = contract_start_block;
    }

    info!("Switching to listening for new orders (deltas)");

    listen_for_new_deltas(&client, &order_book, &matching_orders,
        last_processed_block, contract_h256).await
}

async fn create_pangea_client() -> Result<Client<WsProvider>, Error> {

    let username = ev("PANGEA_USERNAME")?; 
    let password = ev("PANGEA_PASSWORD")?; 
    let url = ev("PANGEA_URL")?; 

    let client = ClientBuilder::default()
        .endpoint(&url)
        .credential(username, password)
        .build::<WsProvider>()
        .await?;

    info!("Pangea ws client created and connected.");
    Ok(client)
}

async fn get_latest_block(chain_id: ChainId) -> Result<i64, Error> {
    let provider_url = match chain_id {
        ChainId::FUEL => Ok("mainnet.fuel.network"),
        ChainId::FUELTESTNET => Ok("testnet.fuel.network"),
        _ => Err(Error::UnknownChainIdError)
    }?;
    let provider = Provider::connect(provider_url).await?;
    Ok(provider.latest_block_height().await.map(|height| height as i64)?)
}

async fn fetch_historical_data(
    client: &Client<WsProvider>,
    order_book: &Arc<OrderBook>,
    matching_orders: &Arc<MatchingOrders>,
    contract_start_block: i64,
    contract_h256: H256,
) -> Result<i64, Error> {
    let fuel_chain = match ev("CHAIN")?.as_str() { 
        "FUEL" => ChainId::FUEL,
        _ => ChainId::FUELTESTNET,
    };
    let batch_size = 10_000;
    let mut last_processed_block = contract_start_block;

    let target_latest_block = get_latest_block(fuel_chain).await?;
    info!("Target last block for processing: {}", target_latest_block);

    while last_processed_block < target_latest_block {
        let to_block = (last_processed_block + batch_size).min(target_latest_block);

        let request_batch = GetSparkOrderRequest {
            from_block: Bound::Exact(last_processed_block),
            to_block: Bound::Exact(to_block),
            market_id__in: HashSet::from([contract_h256]),
            chains: HashSet::from([fuel_chain]),
            ..Default::default()
        };

        let stream_batch = client
            .get_fuel_spark_orders_by_format(request_batch, Format::JsonStream, false)
            .await
            .expect("Failed to get fuel spark orders batch");

        pangea_client::futures::pin_mut!(stream_batch);

        while let Some(data) = stream_batch.next().await {
            match data {
                Ok(data) => {
                    let data = String::from_utf8(data)?;
                    let order: PangeaOrderEvent = serde_json::from_str(&data)?;
                    handle_order_event(order_book.clone(), matching_orders.clone(),
                        order).await;
                }
                Err(e) => {
                    error!("Error in the stream of historical orders: {e}");
                    break;
                }
            }
        }

        last_processed_block = to_block;
        info!(
            "Processed events up to block {}. Moving to the next batch...",
            last_processed_block
        );
    }

    Ok(last_processed_block)
}


async fn listen_for_new_deltas(
    client: &Client<WsProvider>,
    order_book: &Arc<OrderBook>,
    matching_orders: &Arc<MatchingOrders>,
    mut last_processed_block: i64,
    contract_h256: H256,
) -> Result<(), Error> {
    let mut retry_delay = Duration::from_secs(1);
    let reconnect_interval = Duration::from_secs(10*60); 
    let mut reconnect_timer = interval(reconnect_interval);
    let mut processing = false;

    loop {
        tokio::select! {
            _ = reconnect_timer.tick(), if !processing => {
                info!("Scheduled reconnect to refresh connection...");
                processing = false; 
                let fuel_chain = match ev("CHAIN")?.as_str() {
                    "FUEL" => ChainId::FUEL,
                    _ => ChainId::FUELTESTNET,
                };
                let latest_block = get_latest_block(fuel_chain).await?;
                let buffer_blocks = 10; 
                last_processed_block = latest_block.saturating_sub(buffer_blocks);
                info!("Updated last_processed_block to {}", last_processed_block);
            },
            result = async {
                processing = true;
                let fuel_chain = match ev("CHAIN")?.as_str() {
                    "FUEL" => ChainId::FUEL,
                    _ => ChainId::FUELTESTNET,
                };

                let request_deltas = GetSparkOrderRequest {
                    from_block: Bound::Exact(last_processed_block + 1),
                    to_block: Bound::Subscribe,
                    market_id__in: HashSet::from([contract_h256]),
                    chains: HashSet::from([fuel_chain]),
                    ..Default::default()
                };

                match client
                    .get_fuel_spark_orders_by_format(request_deltas, Format::JsonStream, true)
                    .await
                {
                    Ok(stream_deltas) => {
                        retry_delay = Duration::from_secs(1);
                        pangea_client::futures::pin_mut!(stream_deltas);

                        while let Some(data_result) = stream_deltas.next().await {
                            match data_result {
                                Ok(data) => {
                                    if let Err(e) = process_order_data(&data, order_book, matching_orders, &mut last_processed_block).await {
                                        error!("Failed to process order data: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("Error in the stream of new orders (deltas): {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to initiate stream: {}", e);
                    }
                }

                info!("Reconnecting to listen for new deltas in {} seconds...", retry_delay.as_secs());
                sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(Duration::from_secs(60));
                Ok::<(), Error>(())
            } => {
                processing = false;
                if let Err(e) = result {
                    error!("Error in listen_for_new_deltas: {:?}", e);
                }
            },
        }
    }
}

async fn process_order_data(
    data: &[u8],
    order_book: &Arc<OrderBook>,
    matching_orders: &Arc<MatchingOrders>,
    last_processed_block: &mut i64,
) -> Result<(), Error> {
    let data_str = String::from_utf8(data.to_vec())?;
    let order_event: PangeaOrderEvent = serde_json::from_str(&data_str)?;
    *last_processed_block = order_event.block_number;
    handle_order_event(order_book.clone(),
        matching_orders.clone(),
        order_event).await;
    Ok(())
}

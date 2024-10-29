use ethers_core::types::H256;
use fuels::accounts::provider::Provider;
use log::{error, info};
use pangea_client::{ChainId, Client};
use pangea_client::{
    futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest,
    ClientBuilder, Format, WsProvider,
};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use crate::config::settings::Settings;
use crate::error::Error;
use crate::indexer::order_event_handler::handle_order_event;
use crate::indexer::order_event_handler::PangeaOrderEvent;
use crate::storage::order_book::OrderBook;

pub async fn initialize_pangea_indexer(
    settings: Arc<Settings>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
) -> Result<(), Error> {
    let ws_task_pangea = tokio::spawn(async move {
        if let Err(e) = start_pangea_indexer((*settings).clone(), order_book).await {
            eprintln!("Pangea error: {}", e);
        }
    });

    tasks.push(ws_task_pangea);
    Ok(())
}

async fn start_pangea_indexer(config: Settings, order_book: Arc<OrderBook>) -> Result<(), Error> {
    let client = create_pangea_client(&config).await?;

    let contract_start_block: i64 = config.contract.contract_block;
    let contract_h256 = H256::from_str(&config.contract.contract_id)?;

    let mut last_processed_block =
        fetch_historical_data(&client, &order_book, contract_start_block, contract_h256).await?;

    if last_processed_block == 0 {
        last_processed_block = contract_start_block;
    }

    info!("Switching to listening for new orders (deltas)");

    listen_for_new_deltas(&client, &order_book, last_processed_block, contract_h256).await
}

async fn create_pangea_client(config: &Settings) -> Result<Client<WsProvider>, Error> {
    let username = &config.websockets.pangea_username;
    let password = &config.websockets.pangea_pass;
    let url = "app.pangea.foundation";

    let client = ClientBuilder::default()
        .endpoint(url)
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
    contract_start_block: i64,
    contract_h256: H256,
) -> Result<i64, Error> {
    let fuel_chain = ChainId::FUEL;
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
                    handle_order_event(order_book.clone(), order).await;
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
    mut last_processed_block: i64,
    contract_h256: H256,
) -> Result<(), Error> {
    loop {
        let fuel_chain = ChainId::FUEL;
        let request_deltas = GetSparkOrderRequest {
            from_block: Bound::Exact(last_processed_block + 1),
            to_block: Bound::Subscribe,
            market_id__in: HashSet::from([contract_h256]),
            chains: HashSet::from([fuel_chain]),
            ..Default::default()
        };

        let stream_deltas = client
            .get_fuel_spark_orders_by_format(request_deltas, Format::JsonStream, true)
            .await
            .expect("Failed to get fuel spark deltas");

        pangea_client::futures::pin_mut!(stream_deltas);

        while let Some(data) = stream_deltas.next().await {
            match data {
                Ok(data) => {
                    let data = String::from_utf8(data)?;
                    let order: PangeaOrderEvent = serde_json::from_str(&data)?;
                    last_processed_block = order.block_number;
                    handle_order_event(order_book.clone(), order).await;
                }
                Err(e) => {
                    error!("Error in the stream of new orders (deltas): {e}");
                    break;
                }
            }
        }

        info!("Reconnecting to listen for new deltas...");
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

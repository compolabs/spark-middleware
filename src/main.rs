use config::env::ev;
use error::Error;
use futures_util::future::{join_all, select, FutureExt};
use indexer::{envio::EnvioIndexer, pangea::PangeaIndexer, Indexer};
use lazy_static::lazy_static;
use matchers::websocket::MatcherWebSocket;
use prometheus::{
    register_histogram, register_int_counter, register_int_gauge, Histogram, IntCounter, IntGauge,
};
use std::sync::Arc;
use storage::order_storage::OrderStorage;
use tokio::net::TcpListener;
use tokio::signal;
use tokio_tungstenite::accept_async;
use tracing::{debug, info};
use web::server::rocket;

pub mod config;
pub mod error;
pub mod indexer;
pub mod matchers;
pub mod storage;
pub mod web;

lazy_static! {
    static ref BUY_ORDERS_TOTAL: IntGauge =
        register_int_gauge!("buy_orders_total", "Total buy orders").unwrap();
    static ref SELL_ORDERS_TOTAL: IntGauge =
        register_int_gauge!("sell_orders_total", "Total sell orders").unwrap();
    static ref ORDER_PROCESSING_DURATION: Histogram = register_histogram!(
        "order_processing_duration_seconds",
        "Order processing latency"
    )
    .unwrap();
    static ref ERRORS_TOTAL: IntCounter =
        register_int_counter!("errors_total", "Total errors").unwrap();
    static ref SYNC_STATUS: IntGauge =
        register_int_gauge!("sync_status", "Sync status of the system").unwrap();
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_target(true)
        .with_level(true)
        .init();
    info!("Logger initialized");

    let storage = Arc::new(OrderStorage::new());
    let mut tasks = vec![];

    let indexer_type = ev("INDEXER")
        .unwrap_or_else(|_| "PANGEA".to_string())
        .to_uppercase();
    info!("Selected indexer: {}", indexer_type);

    let indexer: Arc<dyn Indexer + Send + Sync> = match indexer_type.as_str() {
        "ENVIO" => Arc::new(EnvioIndexer::new(Arc::clone(&storage))),
        "PANGEA" => Arc::new(PangeaIndexer::new(Arc::clone(&storage))),
        _ => Arc::new(PangeaIndexer::new(Arc::clone(&storage))),
    };

    indexer.initialize(&mut tasks).await?;

    let port = ev("SERVER_PORT")?.parse()?;
    info!("Starting Rocket HTTP server on port {}", port);
    let rocket_task = tokio::spawn(run_rocket_server(port, Arc::clone(&storage.order_book)));
    tasks.push(rocket_task);

    let matcher_ws_port = ev("MATCHERS_PORT")?.parse()?;
    info!("Starting WebSocket server on port {}", matcher_ws_port);
    let matcher_websocket = Arc::new(MatcherWebSocket::new(Arc::clone(&storage)));
    let matcher_ws_task = tokio::spawn(run_matcher_websocket_server(
        matcher_websocket.clone(),
        matcher_ws_port,
    ));
    tasks.push(matcher_ws_task);

    let ctrl_c_task = tokio::spawn(async {
        signal::ctrl_c().await.expect("failed to listen for event");
        debug!("Ctrl+C received!");
    });
    tasks.push(ctrl_c_task);

    let shutdown_signal = signal::ctrl_c().map(|_| {
        info!("Shutting down gracefully...");
    });

    select(join_all(tasks).boxed(), shutdown_signal.boxed()).await;

    info!("Middleware stopped");
    Ok(())
}

async fn run_rocket_server(port: u16, order_book: Arc<storage::order_book::OrderBook>) {
    let rocket = rocket(port, order_book);
    let _ = rocket.launch().await;
}

async fn run_matcher_websocket_server(
    matcher_websocket: Arc<MatcherWebSocket>,
    matcher_ws_port: u16,
) {
    let matcher_ws_str = format!("0.0.0.0:{}", matcher_ws_port);
    let listener = TcpListener::bind(matcher_ws_str)
        .await
        .expect("Can't bind WebSocket port");

    while let Ok((stream, _)) = listener.accept().await {
        let matcher_websocket_clone = matcher_websocket.clone();

        tokio::spawn(async move {
            let ws_stream = accept_async(stream)
                .await
                .expect("Error during WebSocket handshake");
            matcher_websocket_clone.handle_connection(ws_stream).await;
        });
    }
}

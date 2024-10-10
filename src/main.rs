use config::settings::Settings;
use error::Error;
use futures_util::future::FutureExt;
use futures_util::future::{join_all, select};
use indexer::pangea::initialize_pangea_indexer;
use storage::order_book::OrderBook;
use web::server::rocket;
use std::sync::Arc;
use tokio::signal;

pub mod config;
pub mod error;
pub mod indexer;
pub mod matchers;
pub mod middleware;
pub mod storage;
pub mod subscription;
pub mod web;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    env_logger::init();

    let settings = Arc::new(Settings::new());
    let order_book= Arc::new(OrderBook::new());
    let mut tasks = vec![];

    if settings
        .settings
        .active_indexers
        .contains(&"pangea".to_string())
    {
        initialize_pangea_indexer(settings.clone(), &mut tasks, Arc::clone(&order_book)).await?;
    }
    let rocket_task = tokio::spawn(run_rocket_server(
        Arc::clone(&settings),
        Arc::clone(&order_book),
    ));
    tasks.push(rocket_task);

    /*
    let matcher_task = tokio::spawn(run_matcher_server(
        settings.clone(),
        Arc::clone(&envio_manager.order_pool),
        order_metrics,
    ));
    tasks.push(matcher_task);
    */

    let ctrl_c_task = tokio::spawn(async {
        signal::ctrl_c().await.expect("failed to listen for event");
        println!("Ctrl+C received!");
    });
    tasks.push(ctrl_c_task);

    let shutdown_signal = signal::ctrl_c().map(|_| {
        println!("Shutting down gracefully...");
    });

    select(join_all(tasks).boxed(), shutdown_signal.boxed()).await;

    println!("Application is shutting down.");
    Ok(())
}

async fn run_rocket_server(
    settings: Arc<Settings>,
    order_book: Arc<OrderBook>,
) {
    let rocket = rocket(settings, order_book);
    let _ = rocket.launch().await;
}

/*
async fn run_matcher_server(
    settings: Arc<Settings>,
    order_pool: Arc<ShardedOrderPool>,
) {
    let listener = TcpListener::bind(&settings.matchers.matcher_ws_url)
        .await
        .unwrap();
    let matcher_websocket = Arc::new(MatcherWebSocket::new(
        settings.clone(),
        BatchProcessor::new(settings.clone()),
    ));

    info!(
        "Matcher WebSocket server started at {}",
        &settings.matchers.matcher_ws_url
    );

    while let Ok((stream, _)) = listener.accept().await {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Error during WebSocket handshake");

        let matcher_websocket = Arc::clone(&matcher_websocket);
        let order_pool_clone = Arc::clone(&order_pool);

        tokio::spawn(async move {
            matcher_websocket
                .handle_connection(ws_stream, order_pool_clone)
                .await;
        });
    }
}
*/

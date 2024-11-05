use config::env::ev;
use error::Error;
use futures_util::future::FutureExt;
use futures_util::future::{join_all, select};
use indexer::pangea::initialize_pangea_indexer;
use matchers::websocket::MatcherWebSocket;
use storage::matching_orders::MatchingOrders;
use std::sync::Arc;
use storage::order_book::OrderBook;
use tokio::net::TcpListener;
use tokio::signal;
use tokio_tungstenite::accept_async;
use web::server::rocket;

pub mod config;
pub mod error;
pub mod indexer;
pub mod matchers;
pub mod storage;
pub mod web;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    env_logger::init();

    let order_book = Arc::new(OrderBook::new());
    let matching_orders = Arc::new(MatchingOrders::new());
    let mut tasks = vec![];

    initialize_pangea_indexer(&mut tasks, Arc::clone(&order_book)).await?;

    let port = ev("SERVER_PORT")?.parse()?;
    let rocket_task = tokio::spawn(run_rocket_server(port, Arc::clone(&order_book)));
    tasks.push(rocket_task);
    let matcher_ws_port = ev("MATCHERS_PORT")?.parse()?;
    let matcher_websocket = Arc::new(MatcherWebSocket::new(order_book.clone()));
    let matcher_ws_task = tokio::spawn(run_matcher_websocket_server(
        matcher_websocket.clone(),
        matcher_ws_port
    ));
    tasks.push(matcher_ws_task);

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

async fn run_rocket_server(port: u16, order_book: Arc<OrderBook>) {
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

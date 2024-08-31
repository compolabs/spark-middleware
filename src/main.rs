use config::env::ev;
use error::Error;
use indexer::websocket::WebSocketClient;
use url::Url;

pub mod config;
pub mod error;
pub mod indexer;
pub mod middleware;
pub mod subscription;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    let ws_url_envio= Url::parse(&ev("WEBSOCKET_URL_ENVIO")?)?;

    let websocket_client_envio = WebSocketClient::new(ws_url_envio);

    println!("websocket_client_envio {:?}", websocket_client_envio.url);

    Ok(())
}

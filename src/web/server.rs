use std::collections::HashMap;
use std::sync::Arc;

use crate::config::settings::Settings;
use crate::storage::order_book::OrderBook;
use crate::web::routes::{get_docs, get_routes};
use rocket::{Build, Config, Rocket};
use rocket_okapi::swagger_ui::make_swagger_ui;


pub fn rocket(
    settings: Arc<Settings>,
    order_book: Arc<OrderBook>,
) -> Rocket<Build> {
    let config = Config {
        port: settings.server.server_port,
        ..Config::default()
    };

    rocket::custom(config)
        .manage(order_book)
        .mount("/", get_routes())
        .mount("/swagger", make_swagger_ui(&get_docs()))
}

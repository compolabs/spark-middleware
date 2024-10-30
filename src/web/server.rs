use std::sync::Arc;
use std::net::Ipv4Addr;

use crate::storage::order_book::OrderBook;
use crate::web::routes::{get_docs, get_routes};
use async_graphql::Schema;
use rocket::{Build, Config, Rocket};
use rocket_okapi::swagger_ui::make_swagger_ui;

use super::graphql::Query;
use super::routes::get_graphql_routes;

pub fn rocket(port: u16, order_book: Arc<OrderBook>) -> Rocket<Build> {
    let config = Config {
        address: Ipv4Addr::new(0, 0, 0, 0).into(),
        port,
        ..Config::default()
    };

    let schema = Schema::build(
        Query,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
    .data(Arc::clone(&order_book))
    .finish();

    rocket::custom(config)
        .manage(order_book)
        .manage(schema)
        .mount("/", get_routes())
        .mount("/api", get_graphql_routes())
        .mount("/swagger", make_swagger_ui(&get_docs()))
}

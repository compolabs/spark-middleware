use std::collections::HashMap;
use std::sync::Arc;

use crate::config::settings::Settings;
use crate::metrics::types::OrderMetrics;
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;
use crate::web::routes::{get_docs, get_routes};
use rocket::{Build, Config, Rocket};
use rocket_okapi::swagger_ui::make_swagger_ui;
use tokio::sync::Mutex;

//use super::graphql::create_schema;

pub fn rocket(
    order_managers: HashMap<String, Arc<OrderManager>>,
    aggregator: Arc<Aggregator>,
    settings: Arc<Settings>,
    metrics: Arc<Mutex<OrderMetrics>>,
) -> Rocket<Build> {
    let config = Config {
        port: settings.server.server_port,
        ..Config::default()
    };

    rocket::custom(config)
        .manage(order_managers)
        .manage(aggregator)
        .manage(metrics)
 //       .manage(create_schema())
        .mount("/", get_routes())
//        .mount("/graphql", routes![graphql_query, graphql_request])
//        .mount("/graphiql", routes![graphiql])
        .mount("/swagger", make_swagger_ui(&get_docs()))
}

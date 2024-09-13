use std::collections::HashMap;
use std::sync::Arc;

use crate::config::settings::Settings;
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;
use crate::web::routes::{get_docs, get_routes, graphql_query, graphql_request, graphiql};
use rocket::{routes, Build, Config, Rocket};
use rocket_okapi::swagger_ui::make_swagger_ui;

use super::graphql::create_schema;

pub fn rocket(
    order_managers: HashMap<String, Arc<OrderManager>>,
    aggregator: Arc<Aggregator>,
    settings: Arc<Settings>,
) -> Rocket<Build> {
    let config = Config {
        port: settings.server.server_port,
        ..Config::default()
    };

    rocket::custom(config)
        .manage(order_managers)
        .manage(aggregator)
        .manage(create_schema()) 
        .mount("/", get_routes()) 
        .mount("/graphql", routes![graphql_query, graphql_request]) 
        .mount("/graphiql", routes![graphiql]) 
        .mount("/swagger", make_swagger_ui(&get_docs())) 
}

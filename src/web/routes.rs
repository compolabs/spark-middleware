use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use async_graphql_rocket::{GraphQLRequest, GraphQLResponse};
use log::warn;
use rocket::request::FromParam;
use rocket::response::content;
use rocket::serde::json::Json;
use rocket::{get, routes, Route, State};
use rocket_okapi::swagger_ui::SwaggerUIConfig;
use rocket_okapi::{openapi, openapi_get_routes, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::storage::order_book::OrderBook;

use super::graphql::Query;

#[derive(Serialize, JsonSchema)]
pub struct OrdersResponse {
    pub orders: Vec<SpotOrder>,
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
pub enum Indexer {
    Envio,
    Subsquid,
    Pangea,
}

impl Indexer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Indexer::Envio => "envio",
            Indexer::Subsquid => "subsquid",
            Indexer::Pangea => "superchain",
        }
    }

    pub fn all() -> Vec<Indexer> {
        vec![Indexer::Envio, Indexer::Subsquid, Indexer::Pangea]
    }
}

impl<'r> FromParam<'r> for Indexer {
    type Error = &'r str;

    fn from_param(param: &'r str) -> Result<Self, Self::Error> {
        match param {
            "Envio" => Ok(Indexer::Envio),
            "Subsquid" => Ok(Indexer::Subsquid),
            "Pangea" => Ok(Indexer::Pangea),
            _ => Err(param),
        }
    }
}

#[derive(Serialize, JsonSchema)]
pub struct SpreadResponse {
    pub buy: Option<u128>,
    pub sell: Option<u128>,
    pub spread: Option<i128>,
}

#[openapi]
#[get("/orders/buy")]
pub fn get_buy_orders(order_book: &State<Arc<OrderBook>>) -> Json<OrdersResponse> {
    let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
    Json(OrdersResponse { orders: buy_orders })
}

#[openapi]
#[get("/orders/sell")]
pub fn get_sell_orders(order_book: &State<Arc<OrderBook>>) -> Json<OrdersResponse> {
    let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);
    Json(OrdersResponse {
        orders: sell_orders,
    })
}

#[openapi]
#[get("/spread")]
pub fn get_indexer_spread(order_book: &State<Arc<OrderBook>>) -> Json<SpreadResponse> {
    let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
    let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);

    let max_buy_price = buy_orders.iter().map(|o| o.price).max();
    let min_sell_price = sell_orders.iter().map(|o| o.price).min();

    let spread = if let (Some(max_buy), Some(min_sell)) = (max_buy_price, min_sell_price) {
        Some(min_sell as i128 - max_buy as i128)
    } else {
        None
    };

    Json(SpreadResponse {
        buy: max_buy_price,
        sell: min_sell_price,
        spread,
    })
}

#[openapi]
#[get("/orders/count")]
pub fn get_orders_count(order_book: &State<Arc<OrderBook>>) -> Json<HashMap<String, usize>> {
    let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
    let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);

    let mut counts = HashMap::new();
    counts.insert("buy_orders".to_string(), buy_orders.len());
    counts.insert("sell_orders".to_string(), sell_orders.len());

    Json(counts)
}

#[rocket::post("/graphql", data = "<request>")]
pub async fn graphql_handler(
    schema: &State<Schema<Query, EmptyMutation, EmptySubscription>>,
    request: GraphQLRequest,
) -> GraphQLResponse {
    request.execute(&**schema).await 
}

#[rocket::get("/graphql/playground")]
pub fn graphql_playground() -> content::RawHtml<String> {
    warn!("======GQPLGRND========");
    let gqlpgc = GraphQLPlaygroundConfig::new("/api/graphql");

    content::RawHtml(playground_source(gqlpgc))
}

pub fn get_routes() -> Vec<Route> {
    openapi_get_routes![
        get_buy_orders,
        get_sell_orders,
        get_indexer_spread,
        get_orders_count,
    ]
}

pub fn get_graphql_routes() -> Vec<Route> {
    routes![graphql_handler, graphql_playground]
}

pub fn get_docs() -> SwaggerUIConfig {
    SwaggerUIConfig {
        url: "/openapi.json".to_string(),
        ..Default::default()
    }
}

use async_graphql::{Context, Object, Schema, SimpleObject};
use std::sync::Arc;
use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::storage::order_book::OrderBook;

// Определение структуры SpotOrder для GraphQL
#[derive(SimpleObject, Clone)]
struct Order {
    id: String,
    user: String,
    asset: String,
    amount: String, // Преобразуем u128 в String
    price: String,  // Преобразуем u128 в String
    timestamp: u64,
    order_type: String,
    status: Option<String>,
}

// Определение Query для получения данных
pub struct Query;

#[Object]
impl Query {
    // Получение всех buy ордеров
    pub async fn buy_orders(&self, ctx: &Context<'_>) -> Vec<Order> {
        let order_book = ctx.data::<Arc<OrderBook>>().unwrap();
        let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
        buy_orders.into_iter().map(|order| {
            Order {
                id: order.id,
                user: order.user,
                asset: order.asset,
                amount: order.amount.to_string(), // Преобразование u128 в строку
                price: order.price.to_string(),   // Преобразование u128 в строку
                timestamp: order.timestamp,
                order_type: "Buy".to_string(),
                status: order.status.map(|s| format!("{:?}", s)),
            }
        }).collect()
    }

    // Получение всех sell ордеров
    pub async fn sell_orders(&self, ctx: &Context<'_>) -> Vec<Order> {
        let order_book = ctx.data::<Arc<OrderBook>>().unwrap();
        let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);
        sell_orders.into_iter().map(|order| {
            Order {
                id: order.id,
                user: order.user,
                asset: order.asset,
                amount: order.amount.to_string(), // Преобразование u128 в строку
                price: order.price.to_string(),   // Преобразование u128 в строку
                timestamp: order.timestamp,
                order_type: "Sell".to_string(),
                status: order.status.map(|s| format!("{:?}", s)),
            }
        }).collect()
    }

    // Получение спреда
    pub async fn spread(&self, ctx: &Context<'_>) -> Option<String> {
        let order_book = ctx.data::<Arc<OrderBook>>().unwrap();
        let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
        let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);

        let max_buy_price = buy_orders.iter().map(|o| o.price).max();
        let min_sell_price = sell_orders.iter().map(|o| o.price).min();

        if let (Some(max_buy), Some(min_sell)) = (max_buy_price, min_sell_price) {
            Some((min_sell as i128 - max_buy as i128).to_string()) // Преобразование в строку
        } else {
            None
        }
    }
}

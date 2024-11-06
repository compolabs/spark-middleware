use crate::indexer::spot_order::{LimitType, OrderStatus, OrderType, SpotOrder};
use crate::storage::matching_orders::MatchingOrders;
use crate::storage::order_book::OrderBook;
use chrono::Utc;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
pub struct PangeaOrderEvent {
    pub chain: u64,
    pub block_number: i64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: u64,
    pub log_index: u64,
    pub market_id: String,
    pub order_id: String,
    pub event_type: Option<String>,
    pub asset: Option<String>,
    pub amount: Option<u128>,
    pub asset_type: Option<String>,
    pub order_type: Option<String>,
    pub price: Option<u128>,
    pub user: Option<String>,
    pub order_matcher: Option<String>,
    pub owner: Option<String>,
    pub limit_type: Option<String>,
}

pub async fn handle_order_event(order_book: Arc<OrderBook>, matching_orders: Arc<MatchingOrders>,
    event: PangeaOrderEvent) {
    if let Some(event_type) = event.event_type.as_deref() {
        match event_type {
            "Open" => {
                if let Some(order) = create_new_order_from_event(&event) {
                    order_book.add_order(order);
                    info!("Added new order with id: {}", event.order_id);
                }
            }
            "Trade" => {
                matching_orders.remove(&event.order_id);
                info!("Order {} removed from matching_orders", &event.order_id);
                if let Some(match_size) = event.amount {
                    let o_type = event.order_type_to_enum();
                    let l_type = event.limit_type_to_enum();
                    process_trade(&order_book, &event.order_id, match_size, o_type, l_type);
                }
            }
            "Cancel" => {
                matching_orders.remove(&event.order_id);
                info!("Order {} removed from matching_orders", &event.order_id);
                order_book.remove_order(&event.order_id, event.order_type_to_enum());
                info!(
                    "Removed order with id: {} due to Cancel event",
                    event.order_id
                );
            }
            _ => {
                error!("Unknown event type: {}", event_type);
            }
        }
    }
}

fn create_new_order_from_event(event: &PangeaOrderEvent) -> Option<SpotOrder> {
    if let (Some(price), Some(amount), Some(order_type), Some(user)) = (
        event.price,
        event.amount,
        event.order_type.as_deref(),
        &event.user,
    ) {
        let order_type_enum = match order_type {
            "Buy" => OrderType::Buy,
            "Sell" => OrderType::Sell,
            _ => return None,
        };

        Some(SpotOrder {
            id: event.order_id.clone(),
            user: user.clone(),
            asset: event.asset.clone().unwrap_or_default(),
            amount,
            price,
            timestamp: Utc::now().timestamp_millis() as u64,
            order_type: order_type_enum,
            status: Some(OrderStatus::New),
        })
    } else {
        None
    }
}

pub fn process_trade(
    order_book: &OrderBook,
    order_id: &str,
    trade_amount: u128,
    order_type: Option<OrderType>,
    limit_type: Option<LimitType>,
) {
    match (order_type, limit_type) {
        (Some(order_type), Some(limit_type)) => match limit_type {
            LimitType::GTC => {
                if let Some(mut order) = order_book.get_order(order_id, order_type) {
                    if order.amount > trade_amount {
                        order.amount -= trade_amount;
                        order.status = Some(OrderStatus::PartiallyMatched);
                        order_book.update_order(order.clone());

                        info!(
                            "Updated order with id: {} - partially matched, remaining amount: {}",
                            order_id, order.amount
                        );
                    } else {
                        order.status = Some(OrderStatus::Matched);
                        order_book.remove_order(order_id, Some(order_type));
                        info!("Removed order with id: {} - fully matched", order_id);
                    }
                } else {
                    error!("Order with id: {} not found for trade event", order_id);
                }
            }
            _ => {
                order_book.remove_order(order_id, Some(order_type));
                info!("Removed order with id: {} - FOK or IOC matched", order_id);
            }
        },
        _ => {
            error!(
                "Order type or limit type is None for order_id: {}. Cannot process trade event.",
                order_id
            );
        }
    }
}

impl PangeaOrderEvent {
    pub fn order_type_to_enum(&self) -> Option<OrderType> {
        self.order_type
            .as_deref()
            .and_then(|order_type| match order_type {
                "Buy" => Some(OrderType::Buy),
                "Sell" => Some(OrderType::Sell),
                _ => None,
            })
    }

    pub fn limit_type_to_enum(&self) -> Option<LimitType> {
        self.limit_type
            .as_deref()
            .and_then(|limit_type| match limit_type {
                "FOK" => Some(LimitType::FOK),
                "IOC" => Some(LimitType::IOC),
                "GTC" => Some(LimitType::GTC),
                _ => None,
            })
    }
}

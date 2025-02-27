use crate::indexer::spot_order::{LimitType, OrderStatus, OrderType, SpotOrder};
use crate::storage::matching_orders::MatchingOrders;
use crate::storage::order_book::OrderBook;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error, warn};

#[derive(Debug, Deserialize, Serialize)]
pub struct EnvioOrderEvent {
    pub event_type: String,
    pub id: String,
    pub order_id: Option<String>,
    pub user: Option<String>,
    pub asset: Option<String>,
    pub amount: Option<u128>,
    pub price: Option<u128>,
    pub order_type: Option<String>,
    pub limit_type: Option<String>,
    pub timestamp: Option<String>,
}

pub async fn handle_envio_event(
    order_book: Arc<OrderBook>,
    matching_orders: Arc<MatchingOrders>,
    event: EnvioOrderEvent,
) {
    match event.event_type.as_str() {
        "OpenOrderEvent" => {
            if let Some(order) = create_new_order_from_event(&event) {
                if let Some(existing_order) = order_book.get_order(&order.id, order.order_type) {
                    // Ордер уже существует → обновляем данные
                    debug!("🔄 Updating existing order (id: {})", order.id);

                    let mut updated_order = existing_order.clone();
                    updated_order.amount = order.amount;
                    updated_order.price = order.price;
                    updated_order.timestamp = order.timestamp;
                    updated_order.limit_type = order.limit_type;
                    updated_order.status = order.status;

                    order_book.update_order(updated_order);
                } else {
                    // Ордер новый → добавляем
                    debug!("🟢 New order added (id: {})", order.id);
                    order_book.add_order(order);
                }
            }
        }
        "TradeOrderEvent" => {
            if let Some(order_id) = &event.order_id {
                matching_orders.remove(order_id);
                debug!(
                    "🔄 Order {} removed from matching_orders (Trade event)",
                    order_id
                );
                if let Some(match_size) = event.amount {
                    let o_type = event.order_type_to_enum();
                    let l_type = event.limit_type_to_enum();
                    process_trade(&order_book, order_id, match_size, o_type, l_type);
                }
            }
        }
        "CancelOrderEvent" => {
            if let Some(order_id) = &event.order_id {
                matching_orders.remove(order_id);
                order_book.remove_order(order_id, event.order_type_to_enum());
                debug!("❌ Removed order with id: {} due to Cancel event", order_id);
            }
        }
        _ => {
            error!("❌ Unknown event type: {:?}", event);
        }
    }
}

fn create_new_order_from_event(event: &EnvioOrderEvent) -> Option<SpotOrder> {
    if let (Some(price), Some(amount), Some(order_type), Some(limit_type), Some(user)) = (
        event.price,
        event.amount,
        event.order_type.as_deref(),
        event.limit_type.as_deref(),
        &event.user,
    ) {
        let real_order_id = match &event.order_id {
            Some(oid) => oid.clone(),
            None => {
                warn!(
                    "No 'order_id' in event, fallback to 'id'. Event: {:?}",
                    event
                );
                event.id.clone()
            }
        };

        let order_type_enum = match order_type {
            "Buy" => OrderType::Buy,
            "Sell" => OrderType::Sell,
            _ => return None,
        };
        let limit_type_enum = match limit_type {
            "GTC" => Some(LimitType::GTC),
            "IOC" => Some(LimitType::IOC),
            "FOK" => Some(LimitType::FOK),
            "MKT" => Some(LimitType::MKT),
            _ => None,
        };

        Some(SpotOrder {
            // Здесь используем orderId
            id: real_order_id,
            user: user.clone(),
            asset: event.asset.clone().unwrap_or_default(),
            amount,
            price,
            timestamp: Utc::now().timestamp_millis() as u64,
            order_type: order_type_enum,
            limit_type: limit_type_enum,
            status: Some(OrderStatus::New),
        })
    } else {
        warn!("Missing required fields in event: {:?}", event);
        None
    }
}

fn process_trade(
    order_book: &OrderBook,
    order_id: &str,
    trade_amount: u128,
    order_type: Option<OrderType>,
    limit_type: Option<LimitType>,
) {
    if let (Some(order_type), Some(limit_type)) = (order_type, limit_type) {
        match limit_type {
            LimitType::GTC => {
                if let Some(mut order) = order_book.get_order(order_id, order_type) {
                    if order.amount > trade_amount {
                        order.amount -= trade_amount;
                        order.status = Some(OrderStatus::PartiallyMatched);
                        order_book.update_order(order.clone());
                        debug!(
                            "Updated order with id: {} - partially matched, remaining amount: {}",
                            order_id, order.amount
                        );
                    } else {
                        order.status = Some(OrderStatus::Matched);
                        order_book.remove_order(order_id, Some(order_type));
                        debug!("Removed order with id: {} - fully matched", order_id);
                    }
                } else {
                    error!("Order with id: {} not found for trade event", order_id);
                }
            }
            _ => {
                order_book.remove_order(order_id, Some(order_type));
                debug!("Removed order with id: {} - FOK or IOC matched", order_id);
            }
        }
    } else {
        error!(
            "Order type or limit type is None for order_id: {}. Cannot process trade event.",
            order_id
        );
    }
}

impl EnvioOrderEvent {
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
                "MKT" => Some(LimitType::MKT),
                _ => None,
            })
    }
}

pub fn parse_envio_event(json: &Value, event_type: &str) -> Option<EnvioOrderEvent> {
    Some(EnvioOrderEvent {
        event_type: event_type.to_string(),
        id: json.get("id")?.as_str()?.to_string(),
        order_id: json
            .get("orderId")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        user: json
            .get("user")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        asset: json
            .get("asset")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        amount: json
            .get("amount")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok())),
        price: json
            .get("price")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok())),
        order_type: json
            .get("orderType")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        limit_type: json
            .get("limitType")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        timestamp: json
            .get("timestamp")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
    })
}

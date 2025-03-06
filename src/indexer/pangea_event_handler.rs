use crate::indexer::spot_order::{LimitType, OrderStatus, OrderType, SpotOrder};
use crate::storage::matching_orders::MatchingOrders;
use crate::storage::order_book::OrderBook;
use chrono::Utc;
use prometheus::{register_int_counter, IntCounter};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error, warn};

lazy_static::lazy_static! {
    static ref PROCESSED_ORDERS_TOTAL: IntCounter = register_int_counter!(
        "processed_orders_total",
        "Total number of processed orders"
    )
    .unwrap();
}

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

pub async fn handle_order_event(
    order_book: Arc<OrderBook>,
    matching_orders: Arc<MatchingOrders>,
    event: PangeaOrderEvent,
) {
    if let Some(ref et) = event.event_type {
        info!(
            target: "pangea_events",
            ">> handle_order_event: event_type = {:?}, order_id = {}, block = {}, tx = {}, log_index = {}",
            et,
            event.order_id,
            event.block_number,
            event.transaction_hash,
            event.log_index
        );
    } else {
        info!(
            target: "pangea_events",
            ">> handle_order_event: NO event_type, order_id = {}, block = {}, tx = {}, log_index = {}",
            event.order_id,
            event.block_number,
            event.transaction_hash,
            event.log_index
        );
    }

    if let Some(event_type) = event.event_type.as_deref() {
        match event_type {
            "Open" => {
                if let Some(order) = create_new_order_from_event(&event) {
                    info!(
                        ">> Adding new order to order_book: id = {}, amount = {}, price = {}, limit_type = {:?}",
                        order.id, order.amount, order.price, order.limit_type
                    );
                    order_book.add_order(order);
                    info!("🟢 New order added (id: {})", event.order_id);
                } else {
                    warn!(
                        "Open event received but could not create SpotOrder (fields missing?) for order_id = {}",
                        event.order_id
                    );
                }
            }
            "Trade" => {
                matching_orders.remove(&event.order_id);
                info!(
                    "🔄 Order {} removed from matching_orders (Trade event)",
                    &event.order_id
                );
                if let Some(match_size) = event.amount {
                    let o_type = event.order_type_to_enum();
                    let l_type = event.limit_type_to_enum();

                    info!(
                        ">> process_trade called. event_tx_id = {:?}, order_id = {}, match_size = {}, order_type = {:?}, limit_type = {:?}",
                        event.transaction_hash, event.order_id, match_size, o_type, l_type
                    );
                    
                    process_trade(&order_book, &event.order_id, match_size, o_type, l_type);
                } else {
                    warn!(
                        "Trade event received without amount (match_size) for order_id = {}",
                        event.order_id
                    );
                }
            }
            "Cancel" => {
                matching_orders.remove(&event.order_id);
                info!(
                    "🔄 Order {} removed from matching_orders (Cancel event)",
                    &event.order_id
                );
                order_book.remove_order(&event.order_id, event.order_type_to_enum());
                info!(
                    "Removed order with id: {} due to Cancel event",
                    event.order_id
                );
            }
            _ => {
                error!("❌ Received order event with unsupported type: {:?}", event.event_type);
            }
        }
    } else {
        error!("❌ Received order event without type: {:?}", event);
    }
}

fn create_new_order_from_event(event: &PangeaOrderEvent) -> Option<SpotOrder> {
    let price = event.price?;
    let amount = event.amount?;
    let order_type_str = event.order_type.as_deref()?;
    let user = event.user.as_ref()?;

    let order_type_enum = match order_type_str {
        "Buy" => OrderType::Buy,
        "Sell" => OrderType::Sell,
        _ => {
            warn!("Unknown order_type: {} for order_id {}", order_type_str, event.order_id);
            return None;
        }
    };

    let limit_type_enum = event.limit_type_to_enum();

    Some(SpotOrder {
        id: event.order_id.clone(),
        user: user.clone(),
        asset: event.asset.clone().unwrap_or_default(),
        amount,
        price,
        timestamp: Utc::now().timestamp_millis() as u64,
        order_type: order_type_enum,
        limit_type: Some(limit_type_enum),
        status: Some(OrderStatus::New),
    })
}

pub fn process_trade(
    order_book: &OrderBook,
    order_id: &str,
    trade_amount: u128,
    order_type: Option<OrderType>,
    limit_type: LimitType,
) {
    let order_type = match order_type {
        Some(t) => t,
        None => {
            error!(
                "!!! process_trade: Unknown order_type for {}. Ignoring trade event.",
                order_id
            );
            return;
        }
    };

    info!(
        "process_trade START: order_id = {}, trade_amount = {}, order_type = {:?}, limit_type = {:?}",
        order_id, trade_amount, order_type, limit_type
    );

    match limit_type {
        LimitType::GTC | LimitType::MKT => {
            if let Some(mut order) = order_book.get_order(order_id, order_type) {
                info!(
                    ">> Found order in order_book: id = {}, current_amount = {}, status = {:?}",
                    order.id, order.amount, order.status
                );

                if order.amount > trade_amount {
                    order.amount -= trade_amount;
                    order.status = Some(OrderStatus::PartiallyMatched);
                    order_book.update_order(order.clone());

                    info!(
                        "🟡 Order {} partially matched. Trade amount: {}, remaining amount: {}",
                        order_id, trade_amount, order.amount
                    );
                } else {
                    order.status = Some(OrderStatus::Matched);
                    order_book.remove_order(order_id, Some(order_type));

                    info!(
                        "✅ Order {} fully matched and removed. Trade amount: {}, previous amount: {}",
                        order_id,
                        trade_amount,
                        order.amount
                    );
                }
            } else {
                error!("❌ process_trade: order {} not found in order_book", order_id);
            }
        }
        LimitType::FOK | LimitType::IOC => {
            order_book.remove_order(order_id, Some(order_type));
            info!(
                "🗑️ Order {} removed (FOK/IOC). trade_amount = {}",
                order_id, trade_amount
            );
        }
    }

    // Дополнительный лог, когда выходим
    info!(
        "process_trade END: order_id = {}, limit_type = {:?}",
        order_id, limit_type
    );
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

    pub fn limit_type_to_enum(&self) -> LimitType {
        match self.limit_type.as_deref() {
            Some("FOK") => LimitType::FOK,
            Some("IOC") => LimitType::IOC,
            Some("MKT") => LimitType::MKT,
            _ => LimitType::GTC,
        }
    }
}

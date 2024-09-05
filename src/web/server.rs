use std::collections::HashMap;
use std::sync::Arc;

use rocket::{Build, Rocket, State};
use rocket_okapi::swagger_ui::make_swagger_ui;
use crate::middleware::manager::OrderManager;
use crate::web::routes::{get_routes, get_docs};

pub fn rocket(
    order_managers: HashMap<String, Arc<OrderManager>>,  
) -> Rocket<Build> {
    rocket::build()
        .manage(order_managers)  
        .mount("/", get_routes())  
        .mount("/swagger", make_swagger_ui(&get_docs()))  
}

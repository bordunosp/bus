extern crate proc_macro;

mod bus_event;
mod bus_event_handler;
mod bus_event_handler_database;

use crate::bus_event::bus_event;
use crate::bus_event_handler::{
    bus_event_handler, bus_event_handler_context, bus_event_handler_sea,
    bus_event_handler_sqlx_mysql, bus_event_handler_sqlx_pg,
};
use crate::bus_event_handler_database::{
    bus_event_handler_database_sea, bus_event_handler_database_sqlx_mysql,
    bus_event_handler_database_sqlx_pg,
};
use proc_macro::TokenStream;

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEvent(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event(attr, item)
}

// In memory

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler(item)
}
#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerSea(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_sea(item)
}
#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerSqlxPg(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_sqlx_pg(item)
}
#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerSqlxMysql(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_sqlx_mysql(item)
}

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerContext(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_context(item)
}

// Database

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerDatabaseSea(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_database_sea(item)
}

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerDatabaseSqlxPg(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_database_sqlx_pg(item)
}

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusEventHandlerDatabaseSqlxMysql(_attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler_database_sqlx_mysql(item)
}

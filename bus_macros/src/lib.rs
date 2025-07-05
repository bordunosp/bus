use proc_macro::TokenStream;

extern crate proc_macro;

mod bus_event_database_handler;
mod bus_event_database_pipeline;
mod bus_event_handler;
mod bus_event_pipeline;
mod bus_request_handler;
mod bus_request_pipeline;

#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn BusRequestHandler(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_request_handler::bus_request_handler(attr, item)
}

#[proc_macro_attribute]
#[allow(non_snake_case)]
pub fn BusEventHandler(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_handler::bus_event_handler(attr, item)
}

#[proc_macro_attribute]
#[allow(non_snake_case)]
pub fn BusEventDatabaseHandler(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_database_handler::bus_event_database_handler(attr, item)
}

#[proc_macro_attribute]
#[allow(non_snake_case)]
pub fn BusEventDatabasePipeline(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_database_pipeline::bus_event_database_pipeline(attr, item)
}

#[proc_macro_attribute]
#[allow(non_snake_case)]
pub fn BusRequestPipeline(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_request_pipeline::bus_request_pipeline(attr, item)
}

#[proc_macro_attribute]
#[allow(non_snake_case)]
pub fn BusEventPipeline(attr: TokenStream, item: TokenStream) -> TokenStream {
    bus_event_pipeline::bus_event_pipeline(attr, item)
}

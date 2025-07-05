/// Attribute macro for registering a request handler.
/// Usage:
///   #[RequestHandler]
///   #[RequestHandler(factory)]
pub use bus_macros::BusRequestHandler;

/// Attribute macro for registering a event handler.
/// Usage:
///   #[BusEventHandler]
///   #[BusEventHandler(factory)]
pub use bus_macros::BusEventHandler;

/// Attribute macro for registering a request Pipeline.
/// Usage:
/// #[BusRequestPipeline]
pub use bus_macros::BusRequestPipeline;

/// Attribute macro for registering a event database Pipeline.
/// Usage:
/// #[BusEventPipeline]
pub use bus_macros::BusEventPipeline;

/// Attribute macro for registering a event database handler.
/// Usage:
///   #[BusEventDatabaseHandler]
///   #[BusEventDatabaseHandler(factory)]
#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub use bus_macros::BusEventDatabaseHandler;

/// Attribute macro for registering a event database Pipeline.
/// Usage:
///   #[BusEventDatabasePipeline]
#[cfg(any(feature = "sea-orm-postgres", feature = "sea-orm-mysql"))]
pub use bus_macros::BusEventDatabasePipeline;

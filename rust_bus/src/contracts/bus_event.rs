/// #[BusEvent]
pub trait IBusEvent: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone {
    const EVENT_IDENTITY: &'static str;
}

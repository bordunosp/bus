#[cfg(test)]
mod tests {
    use crate as rust_bus;
    use crate::bus;
    use crate::contracts::meta::BusMetadata;
    use crate::tests::base::setup_bus;
    use once_cell::sync::Lazy;
    use rust_bus::{BusEvent, BusEventHandler};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    static TEST_1: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
    static TEST_2: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));
    static TEST_3: Lazy<Mutex<BusMetadata>> = Lazy::new(|| Mutex::new(BusMetadata::default()));

    #[BusEvent]
    pub struct Test1Event {
        pub msg: String,
    }

    #[BusEventHandler]
    async fn handle_test_event(
        event: &Test1Event,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_1.lock().await;
        *data = event.msg.to_owned();
        Ok(())
    }

    #[tokio::test]
    async fn test_dispatch() {
        setup_bus().await;

        bus::event(
            Test1Event {
                msg: "ok".to_string(),
            },
            &BusMetadata::default(),
        )
        .await
        .unwrap();

        assert_eq!(TEST_1.lock().await.as_str(), "ok");
    }

    #[BusEvent]
    pub struct Test2Event {}

    #[BusEventHandler]
    async fn handle2test1(
        _event: &Test2Event,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 1;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle2test2(
        _event: &Test2Event,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 2;
        Ok(())
    }

    #[tokio::test]
    async fn test_dispatch_multiple_handlers() {
        setup_bus().await;

        bus::event(Test2Event {}, &BusMetadata::default())
            .await
            .unwrap();
        assert_eq!(TEST_2.lock().await.clone(), 3);
    }

    #[BusEvent]
    pub struct Test3Event {}

    #[BusEventHandler]
    async fn test_context_handle(
        _event: &Test3Event,
        meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_3.lock().await;
        *data = meta.clone();
        Ok(())
    }

    #[tokio::test]
    async fn test_context() {
        setup_bus().await;

        let request_id = Uuid::now_v7();
        let correlation_id = Uuid::now_v7();
        let causation_id = Uuid::now_v7();
        let extra_1 = Uuid::now_v7();
        let extra_2 = Uuid::now_v7();

        let mut meta = BusMetadata::default();
        meta.add("request_id", request_id).unwrap();
        meta.add("correlation_id", correlation_id).unwrap();
        meta.add("causation_id", causation_id).unwrap();
        meta.add("extra_1", extra_1).unwrap();
        meta.add("extra_2", extra_2).unwrap();

        bus::event(Test3Event {}, &meta).await.unwrap();
        let res_meta = TEST_3.lock().await.clone();

        assert_eq!(res_meta.request_id, Some(request_id));
        assert_eq!(res_meta.correlation_id, Some(correlation_id));
        assert_eq!(res_meta.causation_id, Some(causation_id));
        assert_eq!(res_meta.get::<Uuid>("request_id"), None);
        assert_eq!(res_meta.get::<Uuid>("correlation_id"), None);
        assert_eq!(res_meta.get::<Uuid>("causation_id"), None);
        assert_eq!(res_meta.get::<Uuid>("extra_1"), Some(extra_1));
        assert_eq!(res_meta.get::<Uuid>("extra_2"), Some(extra_2));
    }

    #[BusEvent]
    pub struct ParentEvent {}
    #[BusEvent]
    pub struct ChildEvent {}

    static NESTED_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEventHandler]
    async fn handle_parent(
        _e: &ParentEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        bus::event(ChildEvent {}, &BusMetadata::default()).await?;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle_child(
        _e: &ChildEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = NESTED_COUNTER.lock().await;
        *count += 1;
        Ok(())
    }

    #[tokio::test]
    async fn test_nested_dispatch() {
        setup_bus().await;

        bus::event(ParentEvent {}, &BusMetadata::default())
            .await
            .unwrap();

        assert_eq!(
            *NESTED_COUNTER.lock().await,
            1,
            "Child handler should be called by Parent handler"
        );
    }

    #[tokio::test]
    async fn test_deep_nested_dispatch() {
        setup_bus().await;

        *NESTED_COUNTER.lock().await = 0;

        for _ in 0..100 {
            bus::event(ParentEvent {}, &BusMetadata::default())
                .await
                .unwrap();
        }

        assert_eq!(*NESTED_COUNTER.lock().await, 100);
    }

    static STOP_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEvent]
    pub struct StopChainEvent {}

    #[BusEventHandler]
    async fn handle_fail_a(
        _e: &StopChainEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = STOP_COUNTER.lock().await;
        *count += 1;
        Err("Fail A".into())
    }

    #[BusEventHandler]
    async fn handle_fail_b(
        _e: &StopChainEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = STOP_COUNTER.lock().await;
        *count += 1;
        Err("Fail B".into())
    }

    #[tokio::test]
    async fn test_handler_chain_stops_immediately() {
        setup_bus().await;
        {
            let mut count = STOP_COUNTER.lock().await;
            *count = 0;
        }

        let result = bus::event(StopChainEvent {}, &BusMetadata::default()).await;
        assert!(result.is_err());

        assert_eq!(
            *STOP_COUNTER.lock().await,
            1,
            "Має відпрацювати рівно один хендлер до зупинки ланцюжка"
        );
    }

    #[BusEvent]
    pub struct PanicEvent {}

    #[BusEventHandler]
    async fn handle_panic_1(
        _e: &PanicEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        panic!("Boom!");
    }

    #[tokio::test]
    async fn test_handler_panic_safety() {
        setup_bus().await;

        let result = bus::event(PanicEvent {}, &BusMetadata::default()).await;

        assert!(
            result.is_err(),
            "Диспетчер мав повернути Err після паніки хендлера"
        );

        let err_msg = format!("{:?}", result.err().unwrap());
        assert!(
            err_msg.contains("Boom!"),
            "Помилка повинна містити текст паніки"
        );
    }
}

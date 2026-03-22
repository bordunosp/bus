#[cfg(test)]
mod tests {
    use crate as rust_bus;
    use crate::contracts::ctx::IBusContext;
    use crate::contracts::meta::BusMetadata;
    use crate::tests::base::setup_bus;
    use crate::{ExampleBusContext, bus};
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
        _ctx: &ExampleBusContext,
        event: &Test1Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_1.lock().await;
        *data = event.msg.to_owned();
        Ok(())
    }

    #[tokio::test]
    async fn test_dispatch() {
        setup_bus().await;
        let txn_ctx = ExampleBusContext::new(BusMetadata::default());

        bus::event(
            &txn_ctx,
            Test1Event {
                msg: "ok".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(TEST_1.lock().await.as_str(), "ok");
    }

    #[BusEvent]
    pub struct Test2Event {}

    #[BusEventHandler]
    async fn handle2test1(
        _ctx: &ExampleBusContext,
        _event: &Test2Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 1;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle2test2(
        _ctx: &ExampleBusContext,
        _event: &Test2Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 2;
        Ok(())
    }

    #[tokio::test]
    async fn test_dispatch_multiple_handlers() {
        setup_bus().await;
        let ctx = ExampleBusContext::new(BusMetadata::default());

        bus::event(&ctx, Test2Event {}).await.unwrap();
        assert_eq!(TEST_2.lock().await.clone(), 3);
    }

    #[BusEvent]
    pub struct Test3Event {}

    #[BusEventHandler]
    async fn test_context_handle(
        ctx: &ExampleBusContext,
        _event: &Test3Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_3.lock().await;
        *data = ctx.metadata().clone();
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

        let txn_ctx = ExampleBusContext::new(meta);

        bus::event(&txn_ctx, Test3Event {}).await.unwrap();
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
        ctx: &ExampleBusContext,
        _e: &ParentEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        bus::event(ctx, ChildEvent {}).await?;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle_child(
        _ctx: &ExampleBusContext,
        _e: &ChildEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = NESTED_COUNTER.lock().await;
        *count += 1;
        Ok(())
    }

    #[tokio::test]
    async fn test_nested_dispatch() {
        setup_bus().await;
        let ctx = ExampleBusContext::new(BusMetadata::default());

        bus::event(&ctx, ParentEvent {}).await.unwrap();

        assert_eq!(
            *NESTED_COUNTER.lock().await,
            1,
            "Child handler should be called by Parent handler"
        );
    }

    #[tokio::test]
    async fn test_event_lifetime_safety() {
        setup_bus().await;
        let ctx = ExampleBusContext::new(BusMetadata::default());

        {
            let local_event = Test1Event {
                msg: "temporary".to_string(),
            };
            bus::event(&ctx, local_event).await.unwrap();
        }

        assert_eq!(TEST_1.lock().await.as_str(), "temporary");
    }

    #[tokio::test]
    async fn test_deep_nested_dispatch() {
        setup_bus().await;
        let ctx = ExampleBusContext::new(BusMetadata::default());

        *NESTED_COUNTER.lock().await = 0;

        for _ in 0..100 {
            bus::event(&ctx, ParentEvent {}).await.unwrap();
        }

        assert_eq!(*NESTED_COUNTER.lock().await, 100);
    }

    static STOP_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEvent]
    pub struct StopChainEvent {}

    #[BusEventHandler]
    async fn handle_fail_a(
        _ctx: &ExampleBusContext,
        _e: &StopChainEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = STOP_COUNTER.lock().await;
        *count += 1;
        Err("Fail A".into())
    }

    #[BusEventHandler]
    async fn handle_fail_b(
        _ctx: &ExampleBusContext,
        _e: &StopChainEvent,
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

        let ctx = ExampleBusContext::new(BusMetadata::default());

        let result = bus::event(&ctx, StopChainEvent {}).await;
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
        _ctx: &ExampleBusContext,
        _e: &PanicEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        panic!("Boom!");
    }

    #[tokio::test]
    async fn test_handler_panic_safety() {
        setup_bus().await;

        let ctx = ExampleBusContext::new(BusMetadata::default());

        let result = bus::event(&ctx, PanicEvent {}).await;

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

    #[BusEvent]
    pub struct SimpleMutEvent {
        pub new_val: usize,
    }

    #[BusEventHandler]
    async fn handle_simple_mut(
        _ctx: &mut ExampleBusContext,
        event: &SimpleMutEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data = event.new_val;

        Ok(())
    }

    #[tokio::test]
    async fn test_basic_mutable_context_passing() {
        setup_bus().await;
        let mut ctx = ExampleBusContext::new(BusMetadata::default());

        bus::event(&mut ctx, SimpleMutEvent { new_val: 42 })
            .await
            .unwrap();

        assert_eq!(*TEST_2.lock().await, 42);
    }

    // ==============================

    // Спеціальний складний контекст для тестування лайфтаймів
    pub struct TripleLifetimeContext<'a, 'b, 'c> {
        pub metadata: BusMetadata,
        pub data_a: &'a String,
        pub data_b: &'b String,
        pub data_c: &'c String,
    }

    // Реалізація базового трейту
    impl<'a, 'b, 'c> IBusContext for TripleLifetimeContext<'a, 'b, 'c> {
        fn metadata(&self) -> &BusMetadata {
            &self.metadata
        }
    }

    #[cfg(test)]
    mod triple_lifetime_tests {
        use super::*;
        use crate::bus;
        use crate::tests::base::setup_bus;
        use once_cell::sync::Lazy;
        use tokio::sync::Mutex;

        static TRIPLE_RESULT: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

        #[BusEvent]
        pub struct TripleTestEvent {}

        #[BusEventHandler]
        async fn handle_triple<'a, 'b, 'c>(
            ctx: &mut TripleLifetimeContext<'a, 'b, 'c>,
            _e: &TripleTestEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut res = TRIPLE_RESULT.lock().await;
            // Перевіряємо, що ми можемо прочитати всі три джерела
            *res = format!("{}-{}-{}", ctx.data_a, ctx.data_b, ctx.data_c);
            Ok(())
        }

        #[tokio::test]
        async fn test_triple_lifetime_independence() {
            setup_bus().await;

            let meta = BusMetadata::default();

            // Створюємо дані в різних скоупах, щоб гарантувати різні лайфтайми
            let a = "A".to_string();
            {
                let b = "B".to_string();
                {
                    let c = "C".to_string();

                    let mut ctx = TripleLifetimeContext {
                        metadata: meta,
                        data_a: &a,
                        data_b: &b,
                        data_c: &c,
                    };

                    // Диспатчимо як мутабельний контекст
                    bus::event(&mut ctx, TripleTestEvent {}).await.unwrap();
                }
            }

            let final_res = TRIPLE_RESULT.lock().await;
            assert_eq!(*final_res, "A-B-C");
        }
    }

    #[BusEvent]
    pub struct MutExpectationEvent {}

    #[BusEventHandler]
    async fn handle_mut_requirement(
        _ctx: &mut ExampleBusContext,
        _e: &MutExpectationEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    #[tokio::test]
    async fn test_init_fails_on_immut_passing() {
        setup_bus().await;

        let ctx = ExampleBusContext::new(BusMetadata::default());
        let result = bus::event(&ctx, MutExpectationEvent {}).await;

        assert!(result.is_err(), "Мала повернутися помилка контексту");

        let err_msg = format!("{:?}", result.err().unwrap());

        assert!(
            !err_msg.contains("Handler panicked"),
            "Помилка прийшла з catch_unwind! Це означає, що FromContext не спрацював і стався прорив пам'яті."
        );

        assert!(
            err_msg.contains("Handler requires &mut context, but provided context is immutable"),
            "Очікувалася помилка валідації мутабельності, але отримано: {}",
            err_msg
        );
    }

    #[BusEvent]
    pub struct ImmutExpectationEvent {}

    #[BusEventHandler]
    async fn handle_immut_requirement(
        _ctx: &ExampleBusContext, // Хендлер очікує лише читання
        _e: &ImmutExpectationEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    #[tokio::test]
    async fn test_mut_to_immut_passing_is_ok() {
        setup_bus().await;

        let mut ctx = ExampleBusContext::new(BusMetadata::default());
        let result = bus::event(&mut ctx, ImmutExpectationEvent {}).await;

        assert!(
            result.is_ok(),
            "Передача &mut в хендлер, що очікує &, має бути успішною. Помилка: {:?}",
            result.err()
        );
    }
}

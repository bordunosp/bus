#[cfg(test)]
mod tests {
    use crate as rust_bus;
    use crate::contracts::ctx::IBusContext;
    use crate::contracts::meta::BusMetadata;
    use crate::tests::base::setup_bus;
    use crate::workers::configuration::BusQueueConfiguration;
    use crate::{ExampleBusContext, bus};
    use once_cell::sync::Lazy;
    use rust_bus::{BusEvent, BusEventHandler};
    use sea_orm::{ConnectionTrait, QueryResult, Statement, TransactionTrait};
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
        _ctx: &ExampleBusContext<'_>,
        event: &Test1Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_1.lock().await;
        *data = event.msg.to_owned();
        Ok(())
    }

    #[tokio::test]
    async fn test_dispatch() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

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
        _ctx: &ExampleBusContext<'_>,
        _event: &Test2Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 1;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle2test2(
        _ctx: &ExampleBusContext<'_>,
        _event: &Test2Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 2;
        Ok(())
    }

    #[tokio::test]
    async fn test_dispatch_multiple_handlers() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

        bus::event(&txn_ctx, Test2Event {}).await.unwrap();
        assert_eq!(TEST_2.lock().await.clone(), 3);
    }

    #[BusEvent]
    pub struct Test3Event {}

    #[BusEventHandler]
    async fn test_context_handle<'a>(
        ctx: &'a ExampleBusContext<'a>,
        _event: &Test3Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_3.lock().await;
        *data = ctx.metadata().clone();
        Ok(())
    }

    #[tokio::test]
    async fn test_context() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();

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

        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, meta);

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
    pub struct DbSuccessEvent {
        pub id: i32,
        pub data: String,
    }

    #[BusEventHandler]
    async fn db_success_handle<'a>(
        ctx: &'a ExampleBusContext<'a>,
        event: &DbSuccessEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "sea-orm-postgres")]
        let sql = "INSERT INTO temp_test (id, val) VALUES ($1, $2)";
        #[cfg(feature = "sea-orm-mysql")]
        let sql = "INSERT INTO temp_test (id, val) VALUES (?, ?)";

        ctx.txn()
            .execute_raw(Statement::from_sql_and_values(
                ctx.txn().get_database_backend(),
                sql,
                vec![event.id.into(), event.data.clone().into()],
            ))
            .await?;
        Ok(())
    }

    #[BusEvent]
    pub struct DbRollbackEvent {
        pub id: i32,
    }

    #[BusEventHandler]
    async fn db_rollback_handle<'a>(
        ctx: &'a ExampleBusContext<'a>,
        event: &DbRollbackEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "sea-orm-postgres")]
        let sql = "INSERT INTO temp_test (id, val) VALUES ($1, $2)";
        #[cfg(feature = "sea-orm-mysql")]
        let sql = "INSERT INTO temp_test (id, val) VALUES (?, ?)";

        ctx.txn()
            .execute_raw(Statement::from_sql_and_values(
                ctx.txn().get_database_backend(),
                sql,
                vec![event.id.into(), "should_be_rolled_back".into()],
            ))
            .await?;

        Err("error executing DBROLLBACK".into())
    }

    async fn setup_temp_table() {
        let pool = BusQueueConfiguration::global().unwrap().get_connection();

        pool.execute_unprepared(
            &"CREATE TABLE IF NOT EXISTS temp_test (id INT PRIMARY KEY, val TEXT)",
        )
        .await
        .unwrap();

        pool.execute_unprepared(&"truncate temp_test")
            .await
            .unwrap();

        #[cfg(feature = "sea-orm-mysql")]
        pool.execute_unprepared(&"ALTER TABLE temp_test AUTO_INCREMENT = 1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_transaction_commit_behavior() {
        setup_bus().await;
        setup_temp_table().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();

        let txn = pool.begin().await.unwrap();

        {
            let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

            bus::event(
                &txn_ctx,
                DbSuccessEvent {
                    id: 1,
                    data: "hello".into(),
                },
            )
            .await
            .unwrap();
        }

        let row: Option<QueryResult> = txn
            .query_one_raw(Statement::from_string(
                txn.get_database_backend(),
                "SELECT val FROM temp_test WHERE id = 1",
            ))
            .await
            .unwrap();

        let query_res = row.unwrap();
        let val: String = query_res.try_get("", "val").unwrap();
        assert_eq!(val, "hello".to_string());

        txn.commit().await.unwrap();

        let row: Option<QueryResult> = pool
            .query_one_raw(Statement::from_string(
                pool.get_database_backend(),
                "SELECT val FROM temp_test WHERE id = 1",
            ))
            .await
            .unwrap();

        let query_res = row.unwrap();
        let val: String = query_res.try_get("", "val").unwrap();
        assert_eq!(val, "hello".to_string());
    }

    #[tokio::test]
    async fn test_transaction_rollback_behavior() {
        setup_bus().await;
        setup_temp_table().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        {
            let mut txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());
            let result = bus::event(&mut txn_ctx, DbRollbackEvent { id: 99 }).await;

            assert!(result.is_err());
        }

        txn.rollback().await.unwrap();

        let check_txn = pool.begin().await.unwrap();

        let exists_row: Option<QueryResult> = check_txn
            .query_one_raw(Statement::from_string(
                check_txn.get_database_backend(),
                "SELECT id FROM temp_test WHERE id = 99",
            ))
            .await
            .unwrap();

        let exists = exists_row.is_some();

        assert!(!exists, "Дані не повинні були зберегтися після rollback");
    }

    #[BusEvent]
    pub struct ParentEvent {}
    #[BusEvent]
    pub struct ChildEvent {}

    static NESTED_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEventHandler]
    async fn handle_parent(
        ctx: &ExampleBusContext<'_>,
        _e: &ParentEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        bus::event(ctx, ChildEvent {}).await?;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle_child(
        _ctx: &ExampleBusContext<'_>,
        _e: &ChildEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = NESTED_COUNTER.lock().await;
        *count += 1;
        Ok(())
    }

    #[tokio::test]
    async fn test_nested_dispatch() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

        bus::event(&txn_ctx, ParentEvent {}).await.unwrap();

        assert_eq!(
            *NESTED_COUNTER.lock().await,
            1,
            "Child handler should be called by Parent handler"
        );
    }

    #[tokio::test]
    async fn test_event_lifetime_safety() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

        {
            let local_event = Test1Event {
                msg: "temporary".to_string(),
            };
            bus::event(&txn_ctx, local_event).await.unwrap();
        }

        assert_eq!(TEST_1.lock().await.as_str(), "temporary");
    }

    #[tokio::test]
    async fn test_deep_nested_dispatch() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

        *NESTED_COUNTER.lock().await = 0;

        for _ in 0..100 {
            bus::event(&txn_ctx, ParentEvent {}).await.unwrap();
        }

        assert_eq!(*NESTED_COUNTER.lock().await, 100);
    }

    static STOP_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEvent]
    pub struct StopChainEvent {}

    #[BusEventHandler]
    async fn handle_fail_a(
        _ctx: &ExampleBusContext<'_>,
        _e: &StopChainEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = STOP_COUNTER.lock().await;
        *count += 1;
        Err("Fail A".into())
    }

    #[BusEventHandler]
    async fn handle_fail_b(
        _ctx: &ExampleBusContext<'_>,
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

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

        let result = bus::event(&txn_ctx, StopChainEvent {}).await;
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
        _ctx: &ExampleBusContext<'_>,
        _e: &PanicEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        panic!("Boom!");
    }

    #[tokio::test]
    async fn test_handler_panic_safety() {
        setup_bus().await;

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let txn_ctx = ExampleBusContext::new(&txn, BusMetadata::default());

        let result = bus::event(&txn_ctx, PanicEvent {}).await;

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
    async fn handle_simple_mut<'a>(
        ctx: &'a mut ExampleBusContext<'a>,
        event: &SimpleMutEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = ctx.txn();
        let mut data = TEST_2.lock().await;
        *data = event.new_val;

        Ok(())
    }

    #[tokio::test]
    async fn test_basic_mutable_context_passing() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        bus::event(&mut txn_ctx, SimpleMutEvent { new_val: 42 })
            .await
            .unwrap();

        assert_eq!(*TEST_2.lock().await, 42);
    }

    // =================

    use sea_orm::DatabaseTransaction;

    pub struct MultiLifetimeSeaContext<'a, 'b, 'c> {
        pub txn: &'a DatabaseTransaction,
        pub cache: &'b String,
        pub request_data: &'c String,
        pub metadata: BusMetadata,
    }

    impl<'a, 'b, 'c> IBusContext<'a> for MultiLifetimeSeaContext<'a, 'b, 'c> {
        fn metadata(&self) -> &BusMetadata {
            &self.metadata
        }

        fn txn(&self) -> &'a DatabaseTransaction {
            self.txn
        }
    }

    #[cfg(test)]
    mod multi_lifetime_db_tests {
        use super::*;
        use crate::tests::base::setup_bus;
        use sea_orm::TransactionTrait;

        // Використовуємо Vec для збору маркерів виконання
        static MULTI_DB_LOG: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

        #[BusEvent]
        pub struct MultiLifetimeTestEvent {}

        #[BusEventHandler]
        async fn handle_triple_lifetime<'a, 'b, 'c>(
            ctx: &'a mut MultiLifetimeSeaContext<'a, 'b, 'c>,
            _e: &MultiLifetimeTestEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
        where
            'b: 'a,
            'c: 'a,
        {
            let mut log = MULTI_DB_LOG.lock().await;
            log.push(format!("{}-{}-TRIPLE", ctx.cache, ctx.request_data));
            Ok(())
        }

        #[BusEventHandler]
        async fn handle_single_lifetime<'a>(
            _ctx: &'a mut MultiLifetimeSeaContext<'a, 'a, 'a>,
            _e: &MultiLifetimeTestEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut log = MULTI_DB_LOG.lock().await;
            log.push("SINGLE".to_string());
            Ok(())
        }

        #[tokio::test]
        async fn test_sea_orm_multi_lifetime_independence() {
            // 1. Очищення логу перед тестом
            {
                let mut log = MULTI_DB_LOG.lock().await;
                log.clear();
            }

            setup_bus().await;
            let pool = BusQueueConfiguration::global().unwrap().get_connection();
            let txn = pool.begin().await.unwrap();

            let cache_val = "REDIS".to_string();
            let req_val = "REQ".to_string();

            {
                let mut ctx = MultiLifetimeSeaContext {
                    txn: &txn,
                    cache: &cache_val,
                    request_data: &req_val,
                    metadata: BusMetadata::default(),
                };

                bus::event(&mut ctx, MultiLifetimeTestEvent {})
                    .await
                    .unwrap();
            }

            // 2. Отримуємо результати
            let final_log = MULTI_DB_LOG.lock().await;

            // Діагностика в консоль, якщо щось піде не так
            println!("Execution log: {:?}", *final_log);

            // 3. Перевірка: нам байдуже на порядок, головне — наявність обох записів
            let has_triple = final_log.iter().any(|s| s == "REDIS-REQ-TRIPLE");
            let has_single = final_log.iter().any(|s| s == "SINGLE");

            assert!(
                has_triple,
                "Handler 'triple' was not executed. Log: {:?}",
                *final_log
            );
            assert!(
                has_single,
                "Handler 'single' was not executed. Log: {:?}",
                *final_log
            );
            assert_eq!(final_log.len(), 2, "Expected exactly 2 handlers to run");

            txn.rollback().await.unwrap();
        }
    }

    // ==========================

    #[BusEvent]
    pub struct MutExpectationEvent {}

    #[BusEventHandler]
    async fn handle_mut_requirement<'a>(
        _ctx: &'a mut ExampleBusContext<'a>,
        _e: &MutExpectationEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    #[tokio::test]
    async fn test_init_fails_on_immut_passing() {
        setup_bus().await;

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let ctx = ExampleBusContext::new(&txn, BusMetadata::default());
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
    async fn handle_immut_requirement<'a>(
        _ctx: &'a ExampleBusContext<'a>, // Хендлер очікує лише читання
        _e: &ImmutExpectationEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    #[tokio::test]
    async fn test_mut_to_immut_passing_is_ok() {
        setup_bus().await;

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();
        let mut ctx = ExampleBusContext::new(&txn, BusMetadata::default());
        let result = bus::event(&mut ctx, ImmutExpectationEvent {}).await;

        assert!(
            result.is_ok(),
            "Передача &mut в хендлер, що очікує &, має бути успішною. Помилка: {:?}",
            result.err()
        );
    }
}

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
        _ctx: &mut ExampleBusContext<'_, '_>,
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
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        bus::event(
            &mut txn_ctx,
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
        _ctx: &mut ExampleBusContext<'_, '_>,
        _event: &Test2Event,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 1;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle2test2(
        _ctx: &mut ExampleBusContext<'_, '_>,
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
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        bus::event(&mut txn_ctx, Test2Event {}).await.unwrap();
        assert_eq!(TEST_2.lock().await.clone(), 3);
    }

    #[BusEvent]
    pub struct Test3Event {}

    #[BusEventHandler]
    async fn test_context_handle<'a>(
        ctx: &'a mut ExampleBusContext<'a, 'a>,
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

        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, meta);

        bus::event(&mut txn_ctx, Test3Event {}).await.unwrap();
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
        ctx: &'a mut ExampleBusContext<'a, 'a>,
        event: &DbSuccessEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "sqlx-postgres")]
        let sql = "INSERT INTO temp_test (id, val) VALUES ($1, $2)";
        #[cfg(feature = "sqlx-mysql")]
        let sql = "INSERT INTO temp_test (id, val) VALUES (?, ?)";

        sqlx::query(sql)
            .bind(event.id)
            .bind(&event.data)
            .execute(&mut **ctx.txn())
            .await?;
        Ok(())
    }

    #[BusEvent]
    pub struct DbRollbackEvent {
        pub id: i32,
    }

    #[BusEventHandler]
    async fn db_rollback_handle<'a>(
        ctx: &'a mut ExampleBusContext<'a, 'a>,
        event: &DbRollbackEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "sqlx-postgres")]
        let sql = "INSERT INTO temp_test (id, val) VALUES ($1, $2)";
        #[cfg(feature = "sqlx-mysql")]
        let sql = "INSERT INTO temp_test (id, val) VALUES (?, ?)";

        sqlx::query(sql)
            .bind(event.id)
            .bind("should_be_rolled_back")
            .execute(&mut **ctx.txn())
            .await?;

        Err("error executing DBROLLBACK".into())
    }

    async fn setup_temp_table() {
        let pool = BusQueueConfiguration::global().unwrap().get_connection();

        sqlx::query("CREATE TABLE IF NOT EXISTS temp_test (id INT PRIMARY KEY, val TEXT)")
            .execute(pool)
            .await
            .unwrap();

        sqlx::query("truncate temp_test")
            .execute(pool)
            .await
            .unwrap();

        #[cfg(feature = "sqlx-mysql")]
        sqlx::query("ALTER TABLE temp_test AUTO_INCREMENT = 1")
            .execute(pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_transaction_commit_behavior() {
        setup_bus().await;
        setup_temp_table().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();

        let mut txn = pool.begin().await.unwrap();

        {
            let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

            bus::event(
                &mut txn_ctx,
                DbSuccessEvent {
                    id: 1,
                    data: "hello".into(),
                },
            )
            .await
            .unwrap();
        }

        let row: (String,) = sqlx::query_as("SELECT val FROM temp_test WHERE id = 1")
            .fetch_one(&mut *txn)
            .await
            .unwrap();

        assert_eq!(row.0, "hello");

        txn.commit().await.unwrap();

        let row: (String,) = sqlx::query_as("SELECT val FROM temp_test WHERE id = 1")
            .fetch_one(pool)
            .await
            .unwrap();

        assert_eq!(row.0, "hello");
    }

    #[tokio::test]
    async fn test_transaction_rollback_behavior() {
        setup_bus().await;
        setup_temp_table().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        {
            let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());
            let result = bus::event(&mut txn_ctx, DbRollbackEvent { id: 99 }).await;

            assert!(result.is_err());
        }

        txn.rollback().await.unwrap();

        let mut check_txn = pool.begin().await.unwrap();

        let exists: Option<(i32,)> = sqlx::query_as("SELECT id FROM temp_test WHERE id = 99")
            .fetch_optional(&mut *check_txn)
            .await
            .ok()
            .flatten();

        assert!(
            exists.is_none(),
            "Дані не повинні були зберегтися після rollback"
        );
    }

    #[BusEvent]
    pub struct ParentEvent {}
    #[BusEvent]
    pub struct ChildEvent {}

    static NESTED_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEventHandler]
    async fn handle_parent(
        ctx: &mut ExampleBusContext<'_, '_>,
        _e: &ParentEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        bus::event(ctx, ChildEvent {}).await?;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle_child(
        _ctx: &mut ExampleBusContext<'_, '_>,
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
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        bus::event(&mut txn_ctx, ParentEvent {}).await.unwrap();

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
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        {
            let local_event = Test1Event {
                msg: "temporary".to_string(),
            };
            bus::event(&mut txn_ctx, local_event).await.unwrap();
        }

        assert_eq!(TEST_1.lock().await.as_str(), "temporary");
    }

    #[tokio::test]
    async fn test_deep_nested_dispatch() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        *NESTED_COUNTER.lock().await = 0;

        for _ in 0..100 {
            bus::event(&mut txn_ctx, ParentEvent {}).await.unwrap();
        }

        assert_eq!(*NESTED_COUNTER.lock().await, 100);
    }

    static STOP_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));

    #[BusEvent]
    pub struct StopChainEvent {}

    #[BusEventHandler]
    async fn handle_fail_a(
        _ctx: &mut ExampleBusContext<'_, '_>,
        _e: &StopChainEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = STOP_COUNTER.lock().await;
        *count += 1;
        Err("Fail A".into())
    }

    #[BusEventHandler]
    async fn handle_fail_b(
        _ctx: &mut ExampleBusContext<'_, '_>,
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
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        let result = bus::event(&mut txn_ctx, StopChainEvent {}).await;
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
        _ctx: &mut ExampleBusContext<'_, '_>,
        _e: &PanicEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        panic!("Boom!");
    }

    #[tokio::test]
    async fn test_handler_panic_safety() {
        setup_bus().await;

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();
        let mut txn_ctx = ExampleBusContext::new(&mut txn, BusMetadata::default());

        let result = bus::event(&mut txn_ctx, PanicEvent {}).await;

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

    // ==========================

    // Спеціальний складний контекст для SQLx
    #[cfg(feature = "_db_sqlx")]
    pub struct MultiLifetimeSqlxContext<'a, 't, 'b, 'c> {
        #[cfg(feature = "sqlx-postgres")]
        pub txn: &'t mut sqlx::Transaction<'a, sqlx::Postgres>,
        #[cfg(feature = "sqlx-mysql")]
        pub txn: &'t mut sqlx::Transaction<'a, sqlx::MySql>,

        pub cache_data: &'b String,
        pub user_session: &'c String,
        pub metadata: BusMetadata,
    }

    #[cfg(feature = "_db_sqlx")]
    impl<'a, 't, 'b, 'c> IBusContext<'a> for MultiLifetimeSqlxContext<'a, 't, 'b, 'c> {
        fn metadata(&self) -> &BusMetadata {
            &self.metadata
        }

        #[cfg(feature = "sqlx-postgres")]
        fn txn<'res>(&'res mut self) -> &'res mut sqlx::Transaction<'a, sqlx::Postgres>
        where
            'a: 'res,
        {
            &mut *self.txn
        }

        #[cfg(feature = "sqlx-mysql")]
        fn txn<'res>(&'res mut self) -> &'res mut sqlx::Transaction<'a, sqlx::MySql>
        where
            'a: 'res,
        {
            &mut *self.txn
        }
    }

    #[cfg(test)]
    mod multi_lifetime_db_tests {
        use super::*;
        use crate::tests::base::setup_bus;

        static MULTI_DB_LOG: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

        #[BusEvent]
        pub struct MultiLifetimeTestEvent {}

        // Хендлер з 4 окремими лайфтаймами (специфіка SQLx транзакцій)
        #[BusEventHandler]
        async fn handle_triple_lifetime<'a, 't, 'b, 'c>(
            ctx: &'a mut MultiLifetimeSqlxContext<'a, 't, 'b, 'c>,
            _e: &MultiLifetimeTestEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
        where
            'b: 'a,
            'c: 'a,
            't: 'a,
        {
            let mut log = MULTI_DB_LOG.lock().await;
            log.push(format!("{}-{}-TRIPLE", ctx.cache_data, ctx.user_session));
            Ok(())
        }

        // Хендлер, де всі дані мають один лайфтайм
        #[BusEventHandler]
        async fn handle_single_lifetime<'a>(
            _ctx: &'a mut MultiLifetimeSqlxContext<'a, 'a, 'a, 'a>,
            _e: &MultiLifetimeTestEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut log = MULTI_DB_LOG.lock().await;
            log.push("SINGLE".to_string());
            Ok(())
        }

        #[tokio::test]
        async fn test_sqlx_multi_lifetime_independence() {
            {
                let mut log = MULTI_DB_LOG.lock().await;
                log.clear();
            }

            setup_bus().await;
            let pool = BusQueueConfiguration::global().unwrap().get_connection();
            let mut txn = pool.begin().await.unwrap();

            let cache_val = "REDIS".to_string();
            let session_val = "REQ".to_string();

            {
                let mut ctx = MultiLifetimeSqlxContext {
                    txn: &mut txn,
                    cache_data: &cache_val,
                    user_session: &session_val,
                    metadata: BusMetadata::default(),
                };

                bus::event(&mut ctx, MultiLifetimeTestEvent {})
                    .await
                    .unwrap();
            }

            let final_log = MULTI_DB_LOG.lock().await;

            let has_triple = final_log.iter().any(|s| s == "REDIS-REQ-TRIPLE");
            let has_single = final_log.iter().any(|s| s == "SINGLE");

            assert!(has_triple, "Handler 'triple' failed. Log: {:?}", *final_log);
            assert!(has_single, "Handler 'single' failed. Log: {:?}", *final_log);
            assert_eq!(final_log.len(), 2);

            txn.rollback().await.unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate as rust_bus;
    use crate::bus;
    use crate::contracts::meta::BusMetadata;
    use crate::tests::base::setup_bus;
    use crate::workers::configuration::BusQueueConfiguration;
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
        _txn: &sea_orm::DatabaseTransaction,
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
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        bus::event(
            &txn,
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
        _txn: &sea_orm::DatabaseTransaction,
        _event: &Test2Event,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data += 1;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle2test2(
        _txn: &sea_orm::DatabaseTransaction,
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
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        bus::event(&txn, Test2Event {}, &BusMetadata::default())
            .await
            .unwrap();
        assert_eq!(TEST_2.lock().await.clone(), 3);
    }

    #[BusEvent]
    pub struct Test3Event {}

    #[BusEventHandler]
    async fn test_context_handle<'a>(
        _txn: &sea_orm::DatabaseTransaction,
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

        bus::event(&txn, Test3Event {}, &meta).await.unwrap();
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
    async fn db_success_handle(
        txn: &sea_orm::DatabaseTransaction,
        event: &DbSuccessEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "sea-orm-postgres")]
        let sql = "INSERT INTO temp_test (id, val) VALUES ($1, $2)";
        #[cfg(feature = "sea-orm-mysql")]
        let sql = "INSERT INTO temp_test (id, val) VALUES (?, ?)";

        txn.execute_raw(Statement::from_sql_and_values(
            txn.get_database_backend(),
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
        txn: &sea_orm::DatabaseTransaction,
        event: &DbRollbackEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "sea-orm-postgres")]
        let sql = "INSERT INTO temp_test (id, val) VALUES ($1, $2)";
        #[cfg(feature = "sea-orm-mysql")]
        let sql = "INSERT INTO temp_test (id, val) VALUES (?, ?)";

        txn.execute_raw(Statement::from_sql_and_values(
            txn.get_database_backend(),
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
            bus::event(
                &txn,
                DbSuccessEvent {
                    id: 1,
                    data: "hello".into(),
                },
                &BusMetadata::default(),
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
            let result =
                bus::event(&txn, DbRollbackEvent { id: 99 }, &BusMetadata::default()).await;

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
        txn: &sea_orm::DatabaseTransaction,
        _e: &ParentEvent,
        meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        bus::event(txn, ChildEvent {}, meta).await?;
        Ok(())
    }

    #[BusEventHandler]
    async fn handle_child(
        _txn: &sea_orm::DatabaseTransaction,
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
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        bus::event(&txn, ParentEvent {}, &BusMetadata::default())
            .await
            .unwrap();

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

        {
            let local_event = Test1Event {
                msg: "temporary".to_string(),
            };
            bus::event(&txn, local_event, &BusMetadata::default())
                .await
                .unwrap();
        }

        assert_eq!(TEST_1.lock().await.as_str(), "temporary");
    }

    #[tokio::test]
    async fn test_deep_nested_dispatch() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        *NESTED_COUNTER.lock().await = 0;

        for _ in 0..100 {
            bus::event(&txn, ParentEvent {}, &BusMetadata::default())
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
        _txn: &sea_orm::DatabaseTransaction,
        _e: &StopChainEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut count = STOP_COUNTER.lock().await;
        *count += 1;
        Err("Fail A".into())
    }

    #[BusEventHandler]
    async fn handle_fail_b(
        _txn: &sea_orm::DatabaseTransaction,
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

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        let result = bus::event(&txn, StopChainEvent {}, &BusMetadata::default()).await;
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
        _txn: &sea_orm::DatabaseTransaction,
        _e: &PanicEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        panic!("Boom!");
    }

    #[tokio::test]
    async fn test_handler_panic_safety() {
        setup_bus().await;

        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let txn = pool.begin().await.unwrap();

        let result = bus::event(&txn, PanicEvent {}, &BusMetadata::default()).await;

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
        _txn: &sea_orm::DatabaseTransaction,
        event: &SimpleMutEvent,
        _meta: &BusMetadata,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut data = TEST_2.lock().await;
        *data = event.new_val;

        Ok(())
    }

    #[tokio::test]
    async fn test_basic_mutable_context_passing() {
        setup_bus().await;
        let pool = BusQueueConfiguration::global().unwrap().get_connection();
        let mut txn = pool.begin().await.unwrap();

        bus::event(
            &mut txn,
            SimpleMutEvent { new_val: 42 },
            &BusMetadata::default(),
        )
        .await
        .unwrap();

        assert_eq!(*TEST_2.lock().await, 42);
    }
}

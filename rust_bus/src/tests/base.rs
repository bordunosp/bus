use crate::init;
use tokio::sync::OnceCell;

static INIT_DONE: OnceCell<()> = OnceCell::const_new();

#[cfg(not(feature = "_db_any"))]
pub async fn setup_bus() {
    INIT_DONE
        .get_or_init(|| async {
            init().await.expect("Failed to init bus queue");
        })
        .await;
}

#[cfg(feature = "_db_any")]
pub async fn setup_bus() {
    INIT_DONE
        .get_or_init(|| async {
            println!("🚀 Starting global one-time initialization...");

            use crate::workers::configuration::{BusQueueConfigurationBuilder, QueueConfigBuilder};

            let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");

            #[cfg(feature = "_db_sqlx")]
            let pool = {
                #[cfg(feature = "sqlx-postgres")]
                let p = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(10)
                    .connect(&db_url)
                    .await
                    .unwrap();

                #[cfg(feature = "sqlx-mysql")]
                let p = sqlx::mysql::MySqlPoolOptions::new()
                    .max_connections(10)
                    .connect(&db_url)
                    .await
                    .unwrap();

                sqlx::query("DELETE FROM bus_jobs_peers")
                    .execute(&p)
                    .await
                    .ok();
                sqlx::query("DELETE FROM bus_jobs").execute(&p).await.ok();
                p
            };

            #[cfg(feature = "_db_sea_orm")]
            let pool = {
                let opt = sea_orm::ConnectOptions::new(db_url)
                    .max_connections(10)
                    .clone();

                use sea_orm::ConnectionTrait;
                let db = sea_orm::Database::connect(opt.clone())
                    .await
                    .expect("Database connection failed");

                db.execute_unprepared("DELETE FROM bus_jobs_peers")
                    .await
                    .ok();
                db.execute_unprepared("DELETE FROM bus_jobs").await.ok();
                opt
            };

            let config_builder = BusQueueConfigurationBuilder::new()
                .connection(pool)
                .add_queue("default", QueueConfigBuilder::new().workers(2));

            init(config_builder)
                .await
                .expect("Failed to init bus queue");

            println!("✅ Initialization finished");
            ()
        })
        .await;
}

use sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

impl Migration {
    async fn up_postgres(&self, db: &SchemaManagerConnection<'_>) -> Result<(), DbErr> {
        db.execute_unprepared(r#"
            CREATE TYPE bus_events_status_enum AS ENUM ('pending', 'processing', 'failed', 'completed');
            CREATE TYPE bus_events_archive_mode_enum AS ENUM ('none', 'all', 'failed', 'completed');
            CREATE TYPE bus_events_status_archive_enum AS ENUM ('failed', 'completed');
            
            CREATE TABLE IF NOT EXISTS bus_events
            (
                id                UUID PRIMARY KEY,
                queue_name        VARCHAR(40)                    NOT NULL,
                type_name_event   VARCHAR(1500)                  NOT NULL,
                type_name_handler VARCHAR(1500)                  NOT NULL,
                status            bus_events_status_enum         NOT NULL,
                payload_json      jsonb,
                payload_bin       BYTEA                          NOT NULL,
                retries_current   INTEGER                        NOT NULL,
                retries_max       INTEGER                        NOT NULL,
                latest_error      TEXT,
                archive_mode      bus_events_archive_mode_enum   NOT NULL,
                should_start_at   TIMESTAMP WITHOUT TIME ZONE    NOT NULL,
                expires_at        TIMESTAMP WITHOUT TIME ZONE,
                expires_interval  interval                       NOT NULL,
                created_at        TIMESTAMP WITHOUT TIME ZONE    NOT NULL,
                updated_at        TIMESTAMP WITHOUT TIME ZONE    NOT NULL
            );
            CREATE INDEX idx_bus_events_status_expires_at ON bus_events (status, expires_at ASC);
            CREATE INDEX idx_bus_events_qn_s_ssa ON bus_events (queue_name, status, should_start_at ASC, id);
            

            CREATE TABLE IF NOT EXISTS bus_events_archive
            (
                id                UUID PRIMARY KEY,
                queue_name        VARCHAR(40)                    NOT NULL,
                type_name_event   VARCHAR(1500)                  NOT NULL,
                type_name_handler VARCHAR(1500)                  NOT NULL,
                status            bus_events_status_archive_enum NOT NULL,
                payload_json      jsonb,
                payload_bin       BYTEA                          NOT NULL,
                latest_error      TEXT,
                created_at        TIMESTAMP WITHOUT TIME ZONE    NOT NULL
            );
            CREATE INDEX idx_bus_events_archive_queue_name ON bus_events_archive (queue_name);
            CREATE INDEX idx_bus_events_archive_status ON bus_events_archive (status);
        "#).await?;

        Ok(())
    }

    async fn up_mysql(&self, db: &SchemaManagerConnection<'_>) -> Result<(), DbErr> {
        db.execute_unprepared(r#"
                CREATE TABLE IF NOT EXISTS bus_events (
                    id binary(16) PRIMARY KEY,
                    queue_name VARCHAR(40) NOT NULL,
                    type_name_event VARCHAR(1500) NOT NULL,
                    type_name_handler VARCHAR(1500) NOT NULL,
                    status ENUM('pending', 'processing', 'failed', 'completed') NOT NULL,
                    payload_json JSON,
                    payload_bin MEDIUMBLOB NOT NULL,
                    retries_current INTEGER NOT NULL,
                    retries_max INTEGER NOT NULL,
                    latest_error TEXT,
                    archive_mode ENUM('none', 'all', 'failed', 'completed') NOT NULL,
                    should_start_at DATETIME NOT NULL,
                    expires_at DATETIME,
                    expires_interval INT NOT NULL COMMENT 'Timeout interval in seconds',
                    created_at DATETIME NOT NULL,
                    updated_at DATETIME NOT NULL
                );
                
                CREATE INDEX idx_bus_events_status_expires_at ON bus_events (status, expires_at ASC);
                CREATE INDEX idx_bus_events_qn_s_ssa ON bus_events (queue_name, status, should_start_at, id);
                
                CREATE TABLE IF NOT EXISTS bus_events_archive (
                    id binary(16) PRIMARY KEY,
                    queue_name VARCHAR(40) NOT NULL,
                    type_name_event VARCHAR(1500) NOT NULL,
                    type_name_handler VARCHAR(1500) NOT NULL,
                    status ENUM('failed', 'successful') NOT NULL,
                    payload_json JSON,
                    payload_bin MEDIUMBLOB NOT NULL,
                    latest_error TEXT,
                    created_at DATETIME NOT NULL
                );
                CREATE INDEX idx_bus_events_archive_queue_name ON bus_events_archive (queue_name);
                CREATE INDEX idx_bus_events_archive_status ON bus_events_archive (status);
        "#).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db_backend = manager.get_database_backend();
        let db = manager.get_connection();

        match db_backend {
            DbBackend::MySql => self.up_mysql(db).await?,
            DbBackend::Postgres => self.up_postgres(db).await?,
            DbBackend::Sqlite => return Err(DbErr::Custom("Sqlite database error".into())),
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db_backend = manager.get_database_backend();
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            DROP TABLE IF EXISTS bus_events;
            DROP TABLE IF EXISTS bus_events_archive;
        "#,
        )
        .await?;

        if DbBackend::Postgres == db_backend {
            db.execute_unprepared(
                r#"
                DROP TYPE IF EXISTS bus_events_status_enum;
                DROP TYPE IF EXISTS bus_events_archive_mode_enum;
                DROP TYPE IF EXISTS bus_events_status_archive_enum;
            "#,
            )
            .await?;
        }

        Ok(())
    }
}

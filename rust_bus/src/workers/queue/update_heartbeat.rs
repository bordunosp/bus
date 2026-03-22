use crate::BusError;
use crate::workers::configuration::BusQueueConfiguration;
use chrono::{Duration, Utc};
use uuid::Uuid;

#[cfg(feature = "_db_sea_orm")]
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

#[cfg(feature = "_db_postgres")]
const SQL_PG: &str = r#"
INSERT INTO bus_jobs_peers (name, node, started_at, expires_at)
VALUES ($1, $2, $3, $4)
    ON CONFLICT (name) DO UPDATE SET
        node = EXCLUDED.node,
        expires_at = EXCLUDED.expires_at;
"#;

#[cfg(feature = "_db_mysql")]
const SQL_MYSQL: &str = r#"
INSERT INTO bus_jobs_peers (name, node, started_at, expires_at)
VALUES (?, ?, ?, ?)
    ON DUPLICATE KEY UPDATE
        node = VALUES(node),
        expires_at = VALUES(expires_at);
"#;

pub(crate) async fn update_heartbeat(
    peer_name: &str,
    node_id: Uuid,
    timeout_sec: i64,
) -> Result<bool, BusError> {
    let config = BusQueueConfiguration::global()?;
    let conn = config.get_connection();

    let now = Utc::now();
    let expires_at = now + Duration::seconds(timeout_sec);

    #[cfg(feature = "sea-orm-postgres")]
    conn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        SQL_PG,
        vec![
            peer_name.into(),
            node_id.into(),
            now.into(),
            expires_at.into(),
        ],
    ))
    .await?;

    #[cfg(feature = "sea-orm-mysql")]
    conn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::MySql,
        SQL_MYSQL,
        vec![
            peer_name.into(),
            node_id.into(),
            now.into(),
            expires_at.into(),
        ],
    ))
    .await?;

    #[cfg(feature = "sqlx-postgres")]
    sqlx::query(SQL_PG)
        .bind(peer_name)
        .bind(node_id)
        .bind(now)
        .bind(expires_at)
        .execute(conn)
        .await
        .map_err(|e| BusError::Configuration(format!("Postgres HB execute error: {}", e)))?;

    #[cfg(feature = "sqlx-mysql")]
    sqlx::query(SQL_MYSQL)
        .bind(peer_name)
        .bind(node_id.as_bytes().as_slice())
        .bind(now.naive_utc())
        .bind(expires_at.naive_utc())
        .execute(conn)
        .await
        .map_err(|e| BusError::Configuration(format!("MySQL HB execute error: {}", e)))?;

    #[cfg(feature = "logging")]
    log::trace!(
        "Bus. Heartbeat for peer '{}' (node: {})",
        peer_name,
        node_id
    );

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::configuration::BusQueueConfiguration;
    use uuid::Uuid;

    use crate::tests::base::setup_bus;
    #[cfg(feature = "_db_sea_orm")]
    use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement};

    #[cfg(feature = "sqlx-postgres")]
    #[tokio::test]
    async fn test_hb_sqlx_postgres_conflict() {
        setup_bus().await;
        let peer = "pg_sqlx_peer";
        let node2 = Uuid::now_v7();

        println!("DEBUG: 1 ========================");

        let conn = BusQueueConfiguration::global().unwrap().get_connection();
        println!("DEBUG: 2 ========================");
        let stats = conn.size();
        let idle = conn.num_idle();
        println!("Pool Size: {}, Idle: {}", stats, idle);

        // Спробуй виконати запит з жорстким таймаутом на рівні майбутнього (Future)
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            sqlx::query("SELECT 1").execute(conn),
        )
        .await
        {
            Ok(Ok(_)) => println!("DEBUG: 3 ========================"),
            Ok(Err(e)) => println!("SQL Error: {:?}", e),
            Err(_) => println!("TIMEOUT: Пул або сокет не відповів за 2 секунди!"),
        }

        // sqlx::query("TRUNCATE TABLE bus_jobs_peers").execute(conn).await.ok();
        sqlx::query("select 1").execute(conn).await.ok();
        println!("DEBUG: 3 ========================");

        update_heartbeat(peer, Uuid::now_v7(), 30).await.unwrap();
        update_heartbeat(peer, node2, 60).await.unwrap();

        //
        let res: (Uuid, i64) = sqlx::query_as(
            "SELECT node, (SELECT COUNT(*) FROM bus_jobs_peers WHERE name = $1) FROM bus_jobs_peers WHERE name = $1"
        )
            .bind(peer)
            .fetch_one(conn).await.unwrap();

        assert_eq!(res.0, node2);
        assert_eq!(res.1, 1);
    }

    #[cfg(feature = "sqlx-mysql")]
    #[tokio::test]
    async fn test_hb_sqlx_mysql_conflict() {
        setup_bus().await;
        let peer = "mysql_sqlx_peer";
        let node2 = Uuid::now_v7();

        update_heartbeat(peer, Uuid::now_v7(), 30).await.unwrap();
        update_heartbeat(peer, node2, 60).await.unwrap();

        let conn = BusQueueConfiguration::global().unwrap().get_connection();
        let (node_bytes, count): (Vec<u8>, i64) = sqlx::query_as(
            "SELECT node, (SELECT COUNT(*) FROM bus_jobs_peers WHERE name = ?) FROM bus_jobs_peers WHERE name = ?"
        ).bind(peer).bind(peer).fetch_one(conn).await.unwrap();

        assert_eq!(node_bytes, node2.as_bytes());
        assert_eq!(count, 1);
    }

    #[cfg(feature = "sea-orm-postgres")]
    #[tokio::test]
    async fn test_hb_sea_postgres_conflict() {
        setup_bus().await;
        let peer = "pg_sea_peer";
        let node2 = Uuid::now_v7();

        update_heartbeat(peer, Uuid::now_v7(), 30).await.unwrap();
        update_heartbeat(peer, node2, 60).await.unwrap();

        let conn = BusQueueConfiguration::global().unwrap().get_connection();
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node FROM bus_jobs_peers WHERE name = $1",
            vec![peer.into()],
        );

        let query_res: Option<QueryResult> = conn.query_one_raw(stmt).await.unwrap();

        let res = query_res.expect("Row missing");
        let val: Uuid = res.try_get_by_index(0).unwrap();
        assert_eq!(val, node2);
    }

    #[cfg(feature = "sea-orm-mysql")]
    #[tokio::test]
    async fn test_hb_sea_mysql_conflict() {
        setup_bus().await;
        let peer = "mysql_sea_peer";
        let node2 = Uuid::now_v7();

        update_heartbeat(peer, Uuid::now_v7(), 30).await.unwrap();
        update_heartbeat(peer, node2, 60).await.unwrap();

        let conn = BusQueueConfiguration::global().unwrap().get_connection();
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            "SELECT node FROM bus_jobs_peers WHERE name = ?",
            vec![peer.into()],
        );

        let query_res: Option<QueryResult> = conn.query_one_raw(stmt).await.unwrap();
        let res = query_res.expect("Row missing");
        let val: Vec<u8> = res.try_get_by_index(0).unwrap();
        assert_eq!(val, node2.as_bytes());
    }
}

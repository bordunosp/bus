pub mod dispatch_in_memory;
pub mod registration;

#[cfg(feature = "_db_any")]
pub mod dispatch_db;

#[cfg(feature = "sea-orm-postgres")]
pub mod dispatch_db_sea_postgres;

#[cfg(feature = "sea-orm-mysql")]
pub mod dispatch_db_sea_mysql;

#[cfg(feature = "sqlx-mysql")]
pub mod dispatch_db_sqlx_mysql;

#[cfg(feature = "sqlx-postgres")]
pub mod dispatch_db_sqlx_postgres;

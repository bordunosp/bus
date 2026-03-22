pub(crate) mod dispatch_in_memory;
pub(crate) mod registration;

#[cfg(feature = "_db_any")]
pub(crate) mod dispatch_db;

#[cfg(feature = "sea-orm-postgres")]
pub(crate) mod dispatch_db_sea_postgres;

#[cfg(feature = "sea-orm-mysql")]
pub(crate) mod dispatch_db_sea_mysql;

#[cfg(feature = "sqlx-mysql")]
pub(crate) mod dispatch_db_sqlx_mysql;

#[cfg(feature = "sqlx-postgres")]
pub(crate) mod dispatch_db_sqlx_postgres;

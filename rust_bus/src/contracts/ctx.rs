use crate::BusError;
use crate::contracts::meta::BusMetadata;
use std::any::type_name;

pub struct RawContext {
    pub ptr: *mut (),
    pub is_mutable: bool,
}

pub trait ToRawContext: Send + Sync {
    fn to_raw(&self) -> RawContext;
}

impl<T: ?Sized + Send + Sync> ToRawContext for &T {
    fn to_raw(&self) -> RawContext {
        let ptr = (*self) as *const T as *mut ();
        RawContext {
            ptr,
            is_mutable: false,
        }
    }
}

impl<T: ?Sized + Send + Sync> ToRawContext for &mut T {
    fn to_raw(&self) -> RawContext {
        let ptr = unsafe {
            let ptr_to_ptr = self as *const &mut T as *const *mut T;
            *ptr_to_ptr
        };

        RawContext {
            ptr: ptr as *mut (),
            is_mutable: true,
        }
    }
}

pub trait FromContext<'a> {
    type Target;
    /// # Safety
    unsafe fn from_raw(raw: RawContext) -> Result<Self::Target, BusError>;
}

impl<'a, T: 'a> FromContext<'a> for &'a T {
    type Target = &'a T;
    unsafe fn from_raw(raw: RawContext) -> Result<Self::Target, BusError> {
        unsafe { Ok(&*(raw.ptr as *const T)) }
    }
}

impl<'a, T: 'a> FromContext<'a> for &'a mut T {
    type Target = &'a mut T;
    unsafe fn from_raw(raw: RawContext) -> Result<Self::Target, BusError> {
        if !raw.is_mutable {
            return Err(BusError::Context(
                "Handler requires &mut context, but provided context is immutable.".to_string(),
            ));
        }
        unsafe { Ok(&mut *(raw.ptr as *mut T)) }
    }
}

#[cfg(not(feature = "_db_any"))]
pub trait IBusContext: Send + Sync {
    fn ctx_identity(&self) -> &'static str {
        type_name::<Self>()
    }
    fn metadata(&self) -> &BusMetadata;
}

#[cfg(feature = "_db_any")]
pub trait IBusContext<'a>: Send + Sync {
    fn ctx_identity(&self) -> &'static str {
        type_name::<Self>()
    }
    fn metadata(&self) -> &BusMetadata;

    #[cfg(feature = "_db_sea_orm")]
    fn txn(&self) -> &'a sea_orm::DatabaseTransaction;

    #[cfg(feature = "sqlx-postgres")]
    fn txn<'b>(&'b mut self) -> &'b mut sqlx::Transaction<'a, sqlx::Postgres>
    where
        'a: 'b;

    #[cfg(feature = "sqlx-mysql")]
    fn txn<'b>(&'b mut self) -> &'b mut sqlx::Transaction<'a, sqlx::MySql>
    where
        'a: 'b;
}

#[cfg(feature = "_db_sea_orm")]
impl<'a, 't, T: IBusContext<'a> + ?Sized> IBusContext<'a> for &'t T
where
    't: 'a,
{
    fn metadata(&self) -> &BusMetadata {
        (**self).metadata()
    }
    fn txn(&self) -> &'a sea_orm::DatabaseTransaction {
        (**self).txn()
    }
}

#[cfg(feature = "_db_any")]
impl<'a, 't, T: IBusContext<'a> + ?Sized> IBusContext<'a> for &'t mut T
where
    't: 'a,
{
    fn metadata(&self) -> &BusMetadata {
        (**self).metadata()
    }

    #[cfg(feature = "_db_sea_orm")]
    fn txn(&self) -> &'a sea_orm::DatabaseTransaction {
        (**self).txn()
    }

    #[cfg(feature = "sqlx-postgres")]
    fn txn<'b>(&'b mut self) -> &'b mut sqlx::Transaction<'a, sqlx::Postgres>
    where
        'a: 'b,
    {
        (**self).txn()
    }

    #[cfg(feature = "sqlx-mysql")]
    fn txn<'b>(&'b mut self) -> &'b mut sqlx::Transaction<'a, sqlx::MySql>
    where
        'a: 'b,
    {
        (**self).txn()
    }
}

#[cfg(feature = "_db_sqlx")]
pub struct ExampleBusContext<'a, 't> {
    #[cfg(feature = "sqlx-postgres")]
    txn: &'t mut sqlx::Transaction<'a, sqlx::Postgres>,

    #[cfg(feature = "sqlx-mysql")]
    txn: &'t mut sqlx::Transaction<'a, sqlx::MySql>,

    metadata: BusMetadata,
}

#[cfg(feature = "_db_sqlx")]
impl<'a, 't> ExampleBusContext<'a, 't> {
    pub fn new(
        #[cfg(feature = "sqlx-postgres")] txn: &'t mut sqlx::Transaction<'a, sqlx::Postgres>,
        #[cfg(feature = "sqlx-mysql")] txn: &'t mut sqlx::Transaction<'a, sqlx::MySql>,
        metadata: BusMetadata,
    ) -> Self {
        Self { txn, metadata }
    }
}

#[cfg(feature = "_db_sqlx")]
impl<'a, 't> IBusContext<'a> for ExampleBusContext<'a, 't> {
    #[cfg(feature = "sqlx-postgres")]
    fn txn<'b>(&'b mut self) -> &'b mut sqlx::Transaction<'a, sqlx::Postgres>
    where
        'a: 'b,
    {
        &mut *self.txn
    }

    #[cfg(feature = "sqlx-mysql")]
    fn txn<'b>(&'b mut self) -> &'b mut sqlx::Transaction<'a, sqlx::MySql>
    where
        'a: 'b,
    {
        &mut *self.txn
    }

    fn metadata(&self) -> &BusMetadata {
        &self.metadata
    }
}

#[cfg(feature = "_db_sea_orm")]
pub struct ExampleBusContext<'a> {
    txn: &'a sea_orm::DatabaseTransaction,
    metadata: BusMetadata,
}

#[cfg(feature = "_db_sea_orm")]
impl<'a> ExampleBusContext<'a> {
    pub fn new(txn: &'a sea_orm::DatabaseTransaction, metadata: BusMetadata) -> Self {
        Self { txn, metadata }
    }
}

#[cfg(feature = "_db_sea_orm")]
impl<'a> IBusContext<'a> for ExampleBusContext<'a> {
    fn metadata(&self) -> &BusMetadata {
        &self.metadata
    }
    #[cfg(feature = "_db_sea_orm")]
    fn txn(&self) -> &'a sea_orm::DatabaseTransaction {
        self.txn
    }
}

// Example Context No Database
#[cfg(not(feature = "_db_any"))]
pub struct ExampleBusContext {
    metadata: BusMetadata,
}

#[cfg(not(feature = "_db_any"))]
impl ExampleBusContext {
    pub fn new(metadata: BusMetadata) -> Self {
        Self { metadata }
    }
}

#[cfg(not(feature = "_db_any"))]
impl IBusContext for ExampleBusContext {
    fn metadata(&self) -> &BusMetadata {
        &self.metadata
    }
}

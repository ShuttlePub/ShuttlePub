use crate::{transfer::{AccountDto, CreateAccountDto}, ApplicationError};

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait CreateAccountAdaptor: 'static + Send + Sync {
    async fn create(&self, account: CreateAccountDto) -> Result<AccountDto, ApplicationError>;
}

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait DeleteAccountAdaptor: 'static + Send + Sync {
    async fn delete(&self, id: i64) -> Result<(), ApplicationError>;
}
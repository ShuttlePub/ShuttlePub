use crate::entities::{Account, AccountId, AccountName};
use crate::error::KernelError;

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait AccountRepository: Send + Sync + 'static {
    async fn create(&self, create: &Account) -> Result<(), KernelError>;
    async fn delete(&self, delete: &AccountId) -> Result<(), KernelError>;

    async fn find_all(&self) -> Result<Vec<Account>, KernelError>;
    async fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, KernelError>;
    async fn find_by_name(&self, name: &AccountName) -> Result<Option<Account>, KernelError>;
}
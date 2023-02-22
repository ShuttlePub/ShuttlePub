use crate::{error::KernelError, entities::{Profile, AccountId}};

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait ProfileRepository: Send + Sync + 'static {
    async fn create(&self, create: &Profile) -> Result<(), KernelError>;
    async fn update(&self, update: &Profile) -> Result<(), KernelError>;

    async fn find_by_account_id(&self, id: &AccountId) -> Result<Option<Profile>, KernelError>;
}
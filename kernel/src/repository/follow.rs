use crate::{entities::{AccountTypes, AccountId}, error::KernelError};

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait FollowRepository: Send + Sync + 'static {
    async fn add(account: &AccountTypes) -> Result<(), KernelError>;
    async fn remove(account: &AccountTypes) -> Result<(), KernelError>;

    async fn find_all_by_src(id: &AccountId) -> Result<Vec<AccountTypes>, KernelError>;
    async fn find_all_by_local(id: &AccountId) -> Result<Vec<AccountId>, KernelError>;
}
use kernel::{
    repository::AccountRepository, 
    entities::{AccountId, Account}
};
use time::OffsetDateTime;

use crate::{
    adaptor::{CreateAccountAdaptor, DeleteAccountAdaptor},
    transfer::{AccountDto, CreateAccountDto}, ApplicationError
};

pub struct CreateAccountInteractor<T> {
    repo: T
}

impl<T> CreateAccountInteractor<T> {
    pub fn new(repo: T) -> Self {
        Self { repo }
    }
}

#[async_trait::async_trait]
impl<T> CreateAccountAdaptor for CreateAccountInteractor<T>
  where T: AccountRepository
{
    async fn create(&self, account: CreateAccountDto) -> Result<AccountDto, ApplicationError> {
        let id = AccountId::default();
        let CreateAccountDto { name, bot } = account;
        let account = Account::new(id, name, bot, OffsetDateTime::now_utc(), OffsetDateTime::now_utc());

        self.repo.create(&account).await?;

        Ok(account.into())
    }
}
pub struct DeleteAccountInteractor<T> {
    repo: T
}

impl<T> DeleteAccountInteractor<T> {
    pub fn new(repo: T) -> Self {
        Self { repo }
    }
}

#[async_trait::async_trait]
impl<T> DeleteAccountAdaptor for DeleteAccountInteractor<T>
  where T: AccountRepository
{
    async fn delete(&self, id: i64) -> Result<(), ApplicationError> {
        let id = AccountId::new(id);

        if (self.repo.find_by_id(&id).await?).is_none() {
            return Err(ApplicationError::NotFound { 
                method: "delete", 
                entity: "account", 
                id: format!("{:?}", id)
            });
        }

        self.repo.delete(&id).await?;

        Ok(())
    }
}
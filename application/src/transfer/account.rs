use kernel::entities::{Account, DestructAccount, DestructUpdateTime};
use time::OffsetDateTime;

#[derive(Debug)]
pub struct AccountDto {
    pub id: i64,
    pub name: String,
    pub bot: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime
}

impl From<Account> for AccountDto {
    fn from(internal: Account) -> Self {
        let DestructAccount { 
            id, 
            name, 
            bot ,
            date,
        } = internal.into_destruct();
        let DestructUpdateTime {
            created_at,
            updated_at,
        } = date.into_destruct();
        AccountDto { 
            id: id.into(), 
            name: name.into(), 
            bot: bot.into(),
            created_at: created_at.into(),
            updated_at: updated_at.into()
        }
    }
}

pub struct CreateAccountDto {
    pub name: String,
    pub bot: bool
}

impl CreateAccountDto {
    pub fn new(
        name: impl Into<String>, 
        bot: impl Into<bool>
    ) -> Self {
        Self { 
            name: name.into(), 
            bot: bot.into() 
        }
    }
}
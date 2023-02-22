use serde::{Deserialize, Serialize};
use destructure::Destructure;
use time::OffsetDateTime;

use super::UpdateTime;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountId(i64);

impl From<AccountId> for i64 {
    fn from(id: AccountId) -> Self {
        id.0
    }
}

impl AsRef<i64> for AccountId {
    fn as_ref(&self) -> &i64 {
        &self.0
    }
}

impl AccountId {
    pub fn new(id: impl Into<i64>) -> Self {
        Self(id.into())
    }
}

impl Default for AccountId {
    fn default() -> Self {
        use rand::Rng;
        let gen = rand::thread_rng()
            .gen_range(1000_0000_0000_0000..=9999_9999_9999_9999);
        Self(gen)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountName(String);

impl From<AccountName> for String {
    fn from(name: AccountName) -> Self {
        name.0
    }
}

impl AsRef<str> for AccountName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AccountName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsBot(bool);

impl IsBot {
    pub fn new(flag: bool) -> Self {
        Self(flag)
    }
}

impl AsRef<bool> for IsBot {
    fn as_ref(&self) -> &bool {
        &self.0
    }
}

impl From<IsBot> for bool {
    fn from(flag: IsBot) -> Self {
        flag.0
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Deserialize, Serialize, Destructure)]
pub struct Account {
    id: AccountId,
    name: AccountName,
    bot: IsBot,
    date: UpdateTime
}

impl Account {
    pub fn new(
        id: impl Into<i64>,
        name: impl Into<String>,
        bot: impl Into<bool>,
        created_at: impl Into<OffsetDateTime>,
        updated_at: impl Into<OffsetDateTime>
    ) -> Self {
        Self {
            id: AccountId::new(id),
            name: AccountName::new(name),
            bot: IsBot::new(bot.into()),
            date: UpdateTime::new(created_at.into(), updated_at.into())
        }
    }

    pub fn id(&self) -> &AccountId {
        &self.id
    }

    pub fn name(&self) -> &AccountName {
        &self.name
    }

    pub fn bot(&self) -> &IsBot {
        &self.bot
    }

    pub fn date(&self) -> &UpdateTime {
        &self.date
    }
}

#[cfg(test)]
mod test {
    use time::OffsetDateTime;

    use crate::entities::Account;

    #[test]
    fn struct_test() {
        let _account = Account::new(
            1234567890, 
            "test_man", 
            false, 
            OffsetDateTime::now_utc(),
            OffsetDateTime::now_utc()
        );
    }
}
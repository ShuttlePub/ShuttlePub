use serde::{Deserialize, Serialize};
use destructure::Destructure;
use crate::entities::{Follow, Profile};

#[derive(Debug, Copy, Clone, Hash, Serialize, Deserialize)]
pub struct AccountId(i64);

impl From<i64> for AccountId {
    fn from(prime: i64) -> Self {
        Self(prime)
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

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct AccountName(String);

impl From<String> for AccountName {
    fn from(prime: String) -> Self {
        Self(prime.into())
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

#[derive(Debug, Clone, Hash, Deserialize, Serialize, Destructure)]
pub struct Account {
    id: AccountId,
    name: AccountName,

    profile: Profile,
    follow: Follow
}

impl Account {
    pub fn new(
        id: impl Into<i64>,
        name: impl Into<String>,
        profile: Profile,
        follow: Follow
    ) -> Self {
        Self {
            id: AccountId::new(id),
            name: AccountName::new(name),
            profile,
            follow
        }
    }
}

#[cfg(test)]
mod test {
    use crate::entities::{Account, Follow, Profile};

    #[test]
    fn struct_test() {
        let _account = Account::new(
            1234567890, "test_man",
            Profile::new("Test Man", "I AM TEST MAN", "test.example.com", "test.example.com"),
            Follow::new()
        );
    }
}
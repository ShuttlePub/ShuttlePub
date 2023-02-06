use serde::{Deserialize, Serialize};
use destructure::Destructure;
use crate::entities::{Follow, Profile};

#[derive(Debug, Copy, Clone, Hash, Serialize, Deserialize)]
pub struct AccountId(i64);

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct AccountName(String);

#[derive(Debug, Clone, Hash, Deserialize, Serialize, Destructure)]
pub struct Account {
    id: AccountId,
    name: AccountName,

    profile: Profile,
    follow: Follow
}
use serde::{Deserialize, Serialize};
use crate::entities::AccountId;

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Follow(Vec<AccountTypes>);

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub enum AccountTypes {
    Local(AccountId),
    Federate(String)
}
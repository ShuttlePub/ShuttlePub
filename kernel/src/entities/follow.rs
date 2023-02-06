use serde::{Deserialize, Serialize};
use crate::entities::AccountId;

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Follow(Vec<AccountTypes>);

impl AsRef<[AccountTypes]> for Follow {
    fn as_ref(&self) -> &[AccountTypes] {
        &self.0
    }
}

impl Follow {
    pub fn new() -> Self {
        Self(Vec::new())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub enum AccountTypes {
    Local(AccountId),
    Federate(String)
}

#[cfg(test)]
mod test {
    use crate::entities::Follow;

    #[test]
    fn struct_test() {
        let _follow = Follow::new();
    }
}
use destructure::Destructure;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{AccountId, UpdateTime};

use crate::error::KernelError;

#[derive(Debug, Clone, Copy, Hash, Serialize, Deserialize)]
pub struct ProfileId(Uuid);

impl ProfileId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
}

impl AsRef<Uuid> for ProfileId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl From<ProfileId> for Uuid {
    fn from(id: ProfileId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for ProfileId {
    type Error = KernelError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let tried = Uuid::try_parse(value)
            .map_err(|e| {
                KernelError::Convert(format!("failed parse uuid from &str. `uuid`: {}", e))
            })?;
        Ok(Self(tried))
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct DisplayName(String);

impl From<DisplayName> for String {
    fn from(name: DisplayName) -> Self {
        name.0
    }
}

impl AsRef<str> for DisplayName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl DisplayName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, Default)]
pub struct Summary(String);

impl From<Summary> for String {
    fn from(text: Summary) -> Self {
        text.0
    }
}


impl AsRef<str> for Summary {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Summary {
    pub fn new(summary: impl Into<String>) -> Self {
        Self(summary.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, Default)]
pub struct Icon(String);

impl From<Icon> for String {
    fn from(url: Icon) -> Self {
        url.0
    }
}

impl AsRef<str> for Icon {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Icon {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, Default)]
pub struct Banner(String);

impl From<Banner> for String {
    fn from(url: Banner) -> Self {
        url.0
    }
}

impl AsRef<str> for Banner {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Banner {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, Destructure)]
pub struct Profile {
    id: ProfileId,
    account: AccountId,
    date: UpdateTime,
    name: DisplayName,
    summary: Summary,
    icon: Icon,
    banner: Banner
}

impl Profile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<Uuid>,
        account: impl Into<i64>,
        created_at: impl Into<OffsetDateTime>,
        updated_at: impl Into<OffsetDateTime>,
        name: impl Into<String>,
        summary: impl Into<String>,
        icon: impl Into<String>,
        banner: impl Into<String>
    ) -> Self {
        Self {
            id: ProfileId::new(id.into()),
            account: AccountId::new(account.into()),
            date: UpdateTime::new(created_at.into(), updated_at.into()),
            name: DisplayName::new(name),
            summary: Summary::new(summary),
            icon: Icon::new(icon),
            banner: Banner::new(banner)
        }
    }

    pub fn id(&self) -> &ProfileId {
        &self.id
    }

    pub fn account(&self) -> &AccountId {
        &self.account
    }

    pub fn date(&self) -> &UpdateTime {
        &self.date
    }

    pub fn name(&self) -> &DisplayName {
        &self.name
    }

    pub fn summary(&self) -> &Summary {
        &self.summary
    }

    pub fn icon(&self) -> &Icon {
        &self.icon
    }

    pub fn banner(&self) -> &Banner {
        &self.banner
    }
}

#[cfg(test)]
mod test {
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::entities::{Profile, AccountId};

    #[test]
    fn struct_test() {
        let _profile = Profile::new(
            Uuid::new_v4(),
            AccountId::default(),
            OffsetDateTime::now_utc(),
            OffsetDateTime::now_utc(),
            "Shuttle", 
            "This is Shuttle!", 
            "example.com", 
            "example.com"
        );
    }
}
use destructure::Destructure;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedAt(OffsetDateTime);

impl CreatedAt {
    pub fn new(time: OffsetDateTime) -> Self {
        Self(time)
    }
}

impl From<CreatedAt> for OffsetDateTime {
    fn from(date: CreatedAt) -> Self {
        date.0
    }
}

impl AsRef<OffsetDateTime> for CreatedAt {
    fn as_ref(&self) -> &OffsetDateTime {
        &self.0
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatedAt(OffsetDateTime);

impl UpdatedAt {
    pub fn new(time: OffsetDateTime) -> Self {
        Self(time)
    }
}

impl From<UpdatedAt> for OffsetDateTime {
    fn from(date: UpdatedAt) -> Self {
        date.0
    }
}

impl AsRef<OffsetDateTime> for UpdatedAt {
    fn as_ref(&self) -> &OffsetDateTime {
        &self.0
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, Destructure)]
pub struct UpdateTime {
    created_at: CreatedAt,
    updated_at: UpdatedAt
}

impl UpdateTime {
    pub fn new(
        created_at: impl Into<OffsetDateTime>,
        updated_at: impl Into<OffsetDateTime>
    ) -> Self {
        Self { 
            created_at: CreatedAt::new(created_at.into()), 
            updated_at: UpdatedAt::new(updated_at.into())
        }
    }

    pub fn created_at(&self) -> &CreatedAt {
        &self.created_at
    }

    pub fn updated_at(&self) -> &UpdatedAt {
        &self.updated_at
    }
}


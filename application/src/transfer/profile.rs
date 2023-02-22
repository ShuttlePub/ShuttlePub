use kernel::entities::{Profile, DestructProfile, DestructUpdateTime};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug)]
pub struct ProfileDto {
    pub id: Uuid,
    pub account: i64,
    pub create_at: OffsetDateTime,
    pub update_at: OffsetDateTime,
    pub display_name: String,
    pub summary: String,
    pub icon_url: String,
    pub banner_url: String
}

impl From<Profile> for ProfileDto {
    fn from(internal: Profile) -> Self {
        let DestructProfile {
            id,
            date,
            name,
            summary,
            icon,
            banner,
            account,
        } = internal.into_destruct();
        let DestructUpdateTime {
            created_at,
            updated_at,
        } = date.into_destruct();
        Self { 
            id: id.into(), 
            account: account.into(), 
            create_at: created_at.into(), 
            update_at: updated_at.into(), 
            display_name: name.into(), 
            summary: summary.into(), 
            icon_url: icon.into(), 
            banner_url: banner.into()
        }
    }
}

pub struct CreateProfileDto {
    pub display_name: String,
    pub summary: String,
    pub icon: String,
    pub banner: String,
}

impl CreateProfileDto {
    pub fn new(
        display_name: impl Into<String>,
        summary: impl Into<String>,
        icon: impl Into<String>,
        banner: impl Into<String>
    ) -> Self {
        Self { 
            display_name: display_name.into(), 
            summary: summary.into(), 
            icon: icon.into(), 
            banner: banner.into() 
        }
    }
}

pub struct UpdateProfileDto {
    pub id: Uuid,
    pub display_name: String,
    pub summary: String,
    pub icon: String,
    pub banner: String,
}

impl UpdateProfileDto {
    pub fn new(
        id: impl Into<Uuid>,
        display_name: impl Into<String>,
        summary: impl Into<String>,
        icon: impl Into<String>,
        banner: impl Into<String>,
    ) -> Self {
        Self { 
            id: id.into(),
            display_name: display_name.into(), 
            summary: summary.into(), 
            icon: icon.into(), 
            banner: banner.into() 
        }
    }
}
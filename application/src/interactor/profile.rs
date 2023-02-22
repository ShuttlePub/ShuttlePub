use time::OffsetDateTime;
use uuid::Uuid;

use kernel::{
    entities::{AccountId, Profile, ProfileId, DisplayName, Summary, Icon, Banner, UpdateTime, DestructUpdateTime},
    repository::ProfileRepository, 
};

use crate::{
    adaptor::{CreateProfileAdaptor, UpdateProfileAdaptor},
    transfer::{CreateProfileDto, ProfileDto, UpdateProfileDto}, 
    ApplicationError
};

pub struct CreateProfileInteractor<T> {
    repo: T
}

impl<T> CreateProfileInteractor<T> {
    pub fn new(repo: T) -> Self {
        Self { repo }
    }
}

#[async_trait::async_trait]
impl<T> CreateProfileAdaptor for CreateProfileInteractor<T>
  where T: ProfileRepository
{
    async fn create(&self, account: i64, profile: CreateProfileDto) -> Result<ProfileDto, ApplicationError> {
        let profile_id = ProfileId::new(Uuid::new_v4());
        let account_id = AccountId::new(account);

        let (create_at, updated_at) = (OffsetDateTime::now_utc(), OffsetDateTime::now_utc());

        let CreateProfileDto {
            display_name,
            summary,
            icon,
            banner
        } = profile;

        let profile = Profile::new(
            profile_id,
            account_id,
            create_at,
            updated_at,
            display_name,
            summary,
            icon,
            banner
        );

        self.repo.create(&profile).await?;

        Ok(profile.into())
    }
}

pub struct UpdateProfileInteractor<T> {
    repo: T
}

impl<T> UpdateProfileInteractor<T> {
    pub fn new(repo: T) -> Self {
        Self { repo }
    }
}

#[async_trait::async_trait]
impl<T> UpdateProfileAdaptor for UpdateProfileInteractor<T> 
  where T: ProfileRepository
{
    async fn update(&self, account: i64, profile: UpdateProfileDto) -> Result<ProfileDto, ApplicationError> {
        let id = AccountId::new(account);
        let mut destructed = match self.repo.find_by_account_id(&id).await? {
            Some(item) => item,
            None => return Err(ApplicationError::NotFound { 
                method: "update", 
                entity: "profile", 
                id: format!("{:?}", id)
            })
        }.into_destruct();

        let DestructUpdateTime { created_at, .. } = destructed.date.into_destruct();

        let updated_at = OffsetDateTime::now_utc();

        destructed.name    = DisplayName::new(profile.display_name);
        destructed.summary = Summary::new(profile.summary);
        destructed.icon    = Icon::new(profile.icon);
        destructed.banner  = Banner::new(profile.banner);
        destructed.date    = UpdateTime::new(created_at, updated_at);

        let patched = destructed.freeze();
        
        self.repo.update(&patched).await?;

        Ok(patched.into())
    }
}
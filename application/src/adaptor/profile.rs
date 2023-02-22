use crate::{
    transfer::{ProfileDto, CreateProfileDto, UpdateProfileDto}, 
    ApplicationError
};

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait CreateProfileAdaptor: 'static + Send + Sync {
    async fn create(&self, account: i64, profile: CreateProfileDto) -> Result<ProfileDto, ApplicationError>;
}

#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait UpdateProfileAdaptor: 'static + Send + Sync {
    async fn update(&self, account: i64, profile: UpdateProfileDto) -> Result<ProfileDto, ApplicationError>;
}
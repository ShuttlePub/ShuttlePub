use application::interactor::{CreateAccountInteractor, DeleteAccountInteractor, CreateProfileInteractor, UpdateProfileInteractor};
use driver::{
    postgres::DataBaseDriver, 
    database::{AccountDataBase, ProfileDataBase}
};

pub async fn inject() -> anyhow::Result<()> {
    let pool = DataBaseDriver::setup().await?;
    let account_repository = AccountDataBase::new(pool.clone());
    let profile_repository = ProfileDataBase::new(pool);

    let _account_create = CreateAccountInteractor::new(account_repository.clone());
    let _account_delete = DeleteAccountInteractor::new(account_repository);
    
    let _profile_create = CreateProfileInteractor::new(profile_repository.clone());
    let _profile_update = UpdateProfileInteractor::new(profile_repository);
    Ok(())
}
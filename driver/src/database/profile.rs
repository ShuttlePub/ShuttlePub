use kernel::{
    repository::ProfileRepository, 
    entities::{Profile, AccountId}, 
    KernelError
};
use sqlx::{Pool, Postgres, PgConnection};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::DriverError;

#[derive(Debug, Clone)]
pub struct ProfileDataBase {
    pool: Pool<Postgres>
}

#[allow(dead_code)]
impl ProfileDataBase {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ProfileRepository for ProfileDataBase {
    async fn create(&self, create: &Profile) -> Result<(), KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        Internal::create(create, &mut con).await?;
        Ok(())
    }
    
    async fn update(&self, update: &Profile) -> Result<(), KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        Internal::update(update, &mut con).await?;
        Ok(())
    }

    async fn find_by_account_id(&self, id: &AccountId) -> Result<Option<Profile>, KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        let fetched = Internal::find_by_account_id(id, &mut con).await?;
        Ok(fetched)
    }
}

#[allow(dead_code)]
#[derive(Debug, sqlx::FromRow)]
struct ProfileRow {
    pub id: Uuid,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub account: i64,
    pub display_name: String,
    pub summary: String,
    pub icon: String,
    pub banner: String
}

pub(in crate::database) struct Internal;

impl Internal {
    pub async fn create(create: &Profile, con: &mut PgConnection) -> Result<(), DriverError> {
        sqlx::query(r#"
            INSERT INTO profiles (
                id,
                created_at,
                updated_at,
                account,
                display_name,
                summary,
                icon,
                banner
            )
            VALUES (
                $1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8
            );
        "#)
        .bind(create.id().as_ref())
        .bind(create.date().created_at().as_ref())
        .bind(create.date().updated_at().as_ref())
        .bind(create.account().as_ref())
        .bind(create.name().as_ref())
        .bind(create.summary().as_ref())
        .bind(create.icon().as_ref())
        .bind(create.banner().as_ref())
        .execute(&mut *con)
        .await?;

        Ok(())
    }

    pub async fn update(update: &Profile, con: &mut PgConnection) -> Result<(), DriverError> {
        sqlx::query(r#"
            UPDATE profiles
            SET
                updated_at = $1,
                display_name = $2,
                summary = $3,
                icon = $4,
                banner = $5
            WHERE id = $6
        "#)
        .bind(update.date().updated_at().as_ref())
        .bind(update.name().as_ref())
        .bind(update.summary().as_ref())
        .bind(update.icon().as_ref())
        .bind(update.banner().as_ref())
        .bind(update.id().as_ref())
        .execute(&mut *con)
        .await?;

        Ok(())
    }

    pub async fn find_by_account_id(account: &AccountId, con: &mut PgConnection) -> Result<Option<Profile>, DriverError> {
        sqlx::query_as::<_, ProfileRow>(r#"
            SELECT * from profiles WHERE account = $1
        "#)
        .bind(account.as_ref())
        .fetch_optional(&mut *con)
        .await?
        .map(|fetched| -> Result<Profile, DriverError> {
            Ok(Profile::new(
                fetched.id, 
                fetched.account, 
                fetched.created_at, 
                fetched.updated_at, 
                fetched.display_name, 
                fetched.summary, 
                fetched.icon, 
                fetched.banner
            ))
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::entities::*;
    use sqlx::{Postgres, Pool, postgres::PgPoolOptions};
    use time::PrimitiveDateTime;
    use time_macros::{date, time};
    use crate::database::{account::Internal as AccountDataBaseInternal, profile::ProfileRow};

    use super::Internal;

    async fn test_pool() -> anyhow::Result<Pool<Postgres>> {
        dotenvy::dotenv().ok();

        let url = dotenvy::var("DATABASE_URL")
            .expect("`DATABASE_URL` is not set. This is a required environment variable.");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .idle_timeout(Duration::new(5, 0))
            .connect(&url)
            .await?;

        Ok(pool)
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_create() -> anyhow::Result<()> {
        let pool = test_pool().await?;

        let mut con = pool.begin().await?;

        let a_id = AccountId::default();

        let a_name = AccountName::new("test1");

        let created_at = PrimitiveDateTime::new(date!(2023-2-20), time!(0:00)).assume_utc();
        let updated_at = PrimitiveDateTime::new(date!(2023-2-20), time!(0:00)).assume_utc();

        let a = Account::new(a_id, a_name.clone(), false, created_at, updated_at);

        AccountDataBaseInternal::create(&a, &mut con).await?;

        let a_prof_id = ProfileId::default();

        let a_prof = Profile::new(
            a_prof_id, a_id, 
            created_at, updated_at, 
            "Test Man A", 
            "野生のテストマンAが現れた!", 
            format!("cdn.example.dev/icon/{}", a_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", a_id.as_ref())
        );

        Internal::create(&a_prof, &mut con).await?;

        let a_prof_in_db = Internal::find_by_account_id(&a_id, &mut con).await?.unwrap();
        println!("{:#?}", a_prof_in_db);

        con.rollback().await?;

        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_update() -> anyhow::Result<()> {
        let pool = test_pool().await?;

        let mut con = pool.begin().await?;

        let a_id = AccountId::default();

        let a_name = AccountName::new("test1");

        let created_at = PrimitiveDateTime::new(date!(2023-2-20), time!(0:00)).assume_utc();
        let updated_at = PrimitiveDateTime::new(date!(2023-2-20), time!(0:00)).assume_utc();

        let a = Account::new(a_id, a_name.clone(), false, created_at, updated_at);

        AccountDataBaseInternal::create(&a, &mut con).await?;

        let a_prof_id = ProfileId::default();

        let a_prof = Profile::new(
            a_prof_id, a_id, 
            created_at, updated_at, 
            "Test Man A", 
            "野生のテストマンAが現れた!", 
            format!("cdn.example.dev/icon/{}", a_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", a_id.as_ref())
        );

        Internal::create(&a_prof, &mut con).await?;

        let a_prof_in_db = Internal::find_by_account_id(&a_id, &mut con).await?.unwrap();
        println!("{:#?}", a_prof_in_db);

        let a_prof = Profile::new(
            a_prof_id, a_id, 
            created_at, updated_at, 
            "Test Man A", 
            "野生のテストマンAは逃げた!", 
            format!("cdn.example.dev/icon/{}", a_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", a_id.as_ref())
        );

        Internal::update(&a_prof, &mut con).await?;

        let a_prof_in_db = Internal::find_by_account_id(&a_id, &mut con).await?.unwrap();

        println!("{:#?}", a_prof_in_db);

        con.rollback().await?;
        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test() -> anyhow::Result<()> {
        let pool = test_pool().await?;

        let mut con = pool.begin().await?;

        let a_id = AccountId::default();
        let b_id = AccountId::default();
        let c_id = AccountId::default();

        let a_name = AccountName::new("test1");
        let b_name = AccountName::new("test2");
        let c_name = AccountName::new("test3");

        let created_at = PrimitiveDateTime::new(date!(2023-2-20), time!(0:00)).assume_utc();
        let updated_at = PrimitiveDateTime::new(date!(2023-2-20), time!(0:00)).assume_utc();

        let a = Account::new(a_id, a_name.clone(), false, created_at, updated_at);
        let b = Account::new(b_id, b_name.clone(), false, created_at, updated_at);
        let c = Account::new(c_id, c_name.clone(), true,  created_at, updated_at);

        
        AccountDataBaseInternal::create(&a, &mut con).await?;
        AccountDataBaseInternal::create(&b, &mut con).await?;
        AccountDataBaseInternal::create(&c, &mut con).await?;

        let a_prof_id = ProfileId::default();
        let b_prof_id = ProfileId::default();
        let c_prof_id = ProfileId::default();

        let a_prof = Profile::new(
            a_prof_id, a_id, 
            created_at, updated_at, 
            "Test Man A", 
            "野生のテストマンAが現れた!", 
            format!("cdn.example.dev/icon/{}", a_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", a_id.as_ref())
        );

        let b_prof = Profile::new(
            b_prof_id, b_id, 
            created_at, updated_at, 
            "Test Man B", 
            "野生のテストマンBが現れた!", 
            format!("cdn.example.dev/icon/{}", b_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", b_id.as_ref())
        );

        let c_prof = Profile::new(
            c_prof_id, c_id, 
            created_at, updated_at, 
            "Test Man C", 
            "野生のテストマンCが現れた!", 
            format!("cdn.example.dev/icon/{}", c_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", c_id.as_ref())
        );

        Internal::create(&a_prof, &mut con).await?;
        Internal::create(&b_prof, &mut con).await?;
        Internal::create(&c_prof, &mut con).await?;

        let all: Vec<ProfileRow> = sqlx::query_as(r#"SELECT * FROM profiles"#)
            .fetch_all(&mut *con)
            .await?;
        println!("{:#?}", all);

        let a_prof = Profile::new(
            a_prof_id, a_id, 
            created_at, updated_at, 
            "Test Man A", 
            "野生のテストマンAは逃げた!", 
            format!("cdn.example.dev/icon/{}", a_id.as_ref()), 
            format!("cdn.example.dev/banner/{}", a_id.as_ref())
        );

        Internal::update(&a_prof, &mut con).await?;

        let a_prof_in_db = Internal::find_by_account_id(&a_id, &mut con).await?.unwrap();

        println!("{:#?}", a_prof_in_db);

        AccountDataBaseInternal::delete(&b_id, &mut con).await?;

        let b_prof_in_db = Internal::find_by_account_id(&b_id, &mut con).await?;

        assert!(b_prof_in_db.is_none());

        con.rollback().await?;
        Ok(())
    }
}
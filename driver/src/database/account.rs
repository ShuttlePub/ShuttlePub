use futures_util::{StreamExt, TryStreamExt};
use kernel::{
    KernelError,
    repository::AccountRepository,
    entities::{Account, AccountId, AccountName}
};
use sqlx::{Pool, Postgres, PgConnection};
use time::OffsetDateTime;

use crate::DriverError;

#[derive(Debug, Clone)]
pub struct AccountDataBase {
    pool: Pool<Postgres>
}

#[allow(dead_code)]
impl AccountDataBase {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AccountRepository for AccountDataBase {
    async fn create(&self, create: &Account) -> Result<(), KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        Internal::create(create, &mut con).await?;

        Ok(())
    }

    async fn delete(&self, delete: &AccountId) -> Result<(), KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        Internal::delete(delete, &mut con).await?;

        Ok(())
    }

    async fn find_all(&self) -> Result<Vec<Account>, KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        let all = Internal::find_all(&mut con).await?;
        Ok(all)
    }

    async fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        let found = Internal::find_by_id(id, &mut con).await?;
        Ok(found)
    }

    async fn find_by_name(&self, name: &AccountName) -> Result<Option<Account>, KernelError> {
        let mut con = self.pool.acquire().await
            .map_err(DriverError::SqlX)?;
        let found = Internal::find_by_name(name, &mut con).await?;
        Ok(found)
    }
}

#[allow(dead_code)]
#[derive(sqlx::FromRow)]
struct AccountRow {
    id: i64,
    name: String,
    bot: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime
}

pub(in crate::database) struct Internal;

impl Internal {
    pub async fn create(create: &Account, con: &mut PgConnection) -> Result<(), DriverError> {
        sqlx::query(r#"
            INSERT INTO accounts (
                id,
                name,
                bot,
                created_at,
                updated_at
            )
            VALUES (
                $1,
                $2,
                $3,
                $4,
                $5
            );
        "#)
        .bind(create.id().as_ref())
        .bind(create.name().as_ref())
        .bind(create.bot().as_ref())
        .bind(create.date().created_at().as_ref())
        .bind(create.date().updated_at().as_ref())
        .execute(&mut *con)
        .await?;
        
        Ok(())
    }

    pub async fn delete(delete: &AccountId, con: &mut PgConnection) -> Result<(), DriverError> {
        sqlx::query(r#"
            DELETE FROM accounts WHERE id = $1
        "#)
        .bind(delete.as_ref())
        .execute(&mut *con)
        .await?;

        Ok(())
    }

    pub async fn find_all(con: &mut PgConnection) -> Result<Vec<Account>, DriverError> {
        let all: Vec<Account> = sqlx::query_as::<_, AccountRow>(r#"
            SELECT * from accounts
        "#)
        .fetch(&mut *con)
        .map(|fetched| -> Result<Account, DriverError> {
            let fetched = fetched?;
            Ok(Account::new(
                fetched.id,
                fetched.name,
                fetched.bot,
                fetched.created_at, 
                fetched.updated_at
            ))
        })
        .try_collect()
        .await?;

        Ok(all)
    }

    pub async fn find_by_id(id: &AccountId, con: &mut PgConnection) -> Result<Option<Account>, DriverError> {
        sqlx::query_as::<_, AccountRow>(r#"
            SELECT * from accounts WHERE id = $1
        "#)
        .bind(id.as_ref())
        .fetch_optional(&mut *con)
        .await?
        .map(|fetched| -> Result<Account, DriverError> {
            Ok(Account::new(
                fetched.id,
                fetched.name,
                fetched.bot,
                fetched.created_at, 
                fetched.updated_at
            ))
        })
        .transpose()
    }

    pub async fn find_by_name(name: &AccountName, con: &mut PgConnection) -> Result<Option<Account>, DriverError> {
        sqlx::query_as::<_, AccountRow>(r#"
            SELECT * from accounts WHERE name LIKE $1
        "#)
        .bind(name.as_ref())
        .fetch_optional(&mut *con)
        .await?
        .map(|fetched| -> Result<Account, DriverError> {
            Ok(Account::new(
                fetched.id,
                fetched.name,
                fetched.bot,
                fetched.created_at, 
                fetched.updated_at
            ))
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::entities::{Account, AccountId, AccountName};
    use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
    use time::PrimitiveDateTime;
    use time_macros::{date, time};

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
    async fn test() -> anyhow::Result<()> {
        let pool = test_pool().await?;

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

        let mut con = pool.begin().await?;

        Internal::create(&a, &mut con).await?;
        Internal::create(&b, &mut con).await?;
        Internal::create(&c, &mut con).await?;

        let fetched = Internal::find_all(&mut con).await?;
        println!("{:?}", fetched);

        let fetched = (
            Internal::find_by_id(&a_id, &mut con).await?.unwrap(),
            Internal::find_by_id(&b_id, &mut con).await?.unwrap(),
            Internal::find_by_id(&c_id, &mut con).await?.unwrap()
        );

        assert_eq!(fetched.0, a);
        assert_eq!(fetched.1, b);
        assert_eq!(fetched.2, c);

        assert_ne!(fetched.0, b);

        let fetched = (
            Internal::find_by_name(&a_name, &mut con).await?.unwrap(),
            Internal::find_by_name(&b_name, &mut con).await?.unwrap(),
            Internal::find_by_name(&c_name, &mut con).await?.unwrap()
        );

        assert_eq!(fetched.0, a);
        assert_eq!(fetched.1, b);
        assert_eq!(fetched.2, c);

        assert_ne!(fetched.0, b);

        con.rollback().await?;
        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_create() -> anyhow::Result<()> {
        let pool = test_pool().await?;

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

        let mut con = pool.begin().await?;

        Internal::create(&a, &mut con).await?;
        Internal::create(&b, &mut con).await?;
        Internal::create(&c, &mut con).await?;

        con.rollback().await?;
        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_delete() -> anyhow::Result<()> {
        let pool = test_pool().await?;

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

        let mut con = pool.begin().await?;

        Internal::create(&a, &mut con).await?;
        Internal::create(&b, &mut con).await?;
        Internal::create(&c, &mut con).await?;

        Internal::delete(&a_id, &mut con).await?;
        Internal::delete(&b_id, &mut con).await?;
        Internal::delete(&c_id, &mut con).await?;

        let a = Internal::delete(&a_id, &mut con).await;
        let b = Internal::delete(&b_id, &mut con).await;
        let c = Internal::delete(&c_id, &mut con).await;

        assert!(a.is_ok());
        assert!(b.is_ok());
        assert!(c.is_ok());

        con.rollback().await?;
        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_find_all() -> anyhow::Result<()> {
        let pool = test_pool().await?;

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

        let mut con = pool.begin().await?;

        Internal::create(&a, &mut con).await?;
        Internal::create(&b, &mut con).await?;
        Internal::create(&c, &mut con).await?;

        let fetched = Internal::find_all(&mut con).await?;
        println!("{:?}", fetched);

        con.rollback().await?;
        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_find_by_id() -> anyhow::Result<()> {
        let pool = test_pool().await?;

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

        let mut con = pool.begin().await?;

        Internal::create(&a, &mut con).await?;
        Internal::create(&b, &mut con).await?;
        Internal::create(&c, &mut con).await?;

        let fetched = (
            Internal::find_by_id(&a_id, &mut con).await?.unwrap(),
            Internal::find_by_id(&b_id, &mut con).await?.unwrap(),
            Internal::find_by_id(&c_id, &mut con).await?.unwrap()
        );

        assert_eq!(fetched.0, a);
        assert_eq!(fetched.1, b);
        assert_eq!(fetched.2, c);

        assert_ne!(fetched.0, b);

        con.rollback().await?;
        Ok(())
    }

    #[ignore = "It depends on Postgres and does not work as is."]
    #[tokio::test]
    async fn test_find_by_name() -> anyhow::Result<()> {
        let pool = test_pool().await?;

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

        let mut con = pool.begin().await?;

        Internal::create(&a, &mut con).await?;
        Internal::create(&b, &mut con).await?;
        Internal::create(&c, &mut con).await?;

        let fetched = (
            Internal::find_by_name(&a_name, &mut con).await?.unwrap(),
            Internal::find_by_name(&b_name, &mut con).await?.unwrap(),
            Internal::find_by_name(&c_name, &mut con).await?.unwrap()
        );

        assert_eq!(fetched.0, a);
        assert_eq!(fetched.1, b);
        assert_eq!(fetched.2, c);

        assert_ne!(fetched.0, b);

        con.rollback().await?;
        Ok(())
    }
}
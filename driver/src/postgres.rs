use std::time::Duration;

use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

use crate::DriverError;

pub struct DataBaseDriver;

impl DataBaseDriver {
    pub async fn setup() -> Result<Pool<Postgres>, DriverError> {
        let url = Self::env_setup();
        
        tracing::info!("setup `postgresql` connection pool.");

        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(5000))
            .max_connections(8)
            .connect(&url)
            .await?;

        tracing::info!("setup successful!");
        
        Ok(pool)
    }

    fn env_setup() -> String {
        tracing::info!("enviroment variable check.");
        dotenvy::dotenv().ok();
        let url = dotenvy::var("DATABASE_URL")
            .expect("`DATABASE_URL` does not set! This value required.");
        tracing::info!("checked.");

        url
    }
}
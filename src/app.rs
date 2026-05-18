use eyre::{Error, Context};
use std::time::Duration;
use sqlx::{
    sqlite::Sqlite,
    postgres::Postgres,
    pool::{
        Pool,
        PoolConnection,
        PoolOptions,
    },
};

#[derive(Debug)]
pub struct AppState {
    sqlite: Pool<Sqlite>,
    pg: Pool<Postgres>,
}

// XXX: Set from cmdline when migrating
const PG_URL: &'static str = "postgres:///power";

impl AppState {
    pub fn connect(db: &str) -> Result<Self, Error> {
        let sqlite = PoolOptions::<Sqlite>::new()
            .max_lifetime(Duration::from_secs(1))
            .connect_lazy(db)?;

        let pg_opts = PoolOptions::<Postgres>::new()
            .acquire_timeout(Duration::new(1, 0))
            .max_lifetime(Duration::from_secs(60));
        let pg = pg_opts.connect_lazy(PG_URL)?;

        Ok(Self {
            sqlite,
            pg,
        })
    }

    pub async fn acquire_sqlite(&self) -> Result<PoolConnection<Sqlite>, Error> {
        Ok(self.sqlite.acquire().await?)
    }

    pub async fn acquire_pg(&self) -> Result<PoolConnection<Postgres>, Error> {
        Ok(self.pg.acquire().await
            .wrap_err("Failed to connect to postgres")?
        )
    }
}

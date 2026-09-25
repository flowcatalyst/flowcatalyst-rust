//! One PostgreSQL for the whole run, in Docker (Java's harness embeds one
//! with zonky): a uniquely named container publishing 5432 on a random
//! loopback port, so concurrent runs and other agents' test containers never
//! collide. Removed on drop.

use anyhow::{bail, Context, Result};
use sqlx::{Connection, PgConnection};
use std::process::Command;
use std::time::{Duration, Instant};

const PASSWORD: &str = "parity";
const READY_BUDGET: Duration = Duration::from_secs(90);

pub struct DockerPg {
    container: String,
    port: u16,
}

impl DockerPg {
    pub async fn start(image: &str) -> Result<Self> {
        let container = format!(
            "fc-parity-pg-{}-{}",
            std::process::id(),
            crate::keys::random_token()
        );
        let out = Command::new("docker")
            .args(["run", "-d", "--rm", "--name", &container])
            .args(["-e", &format!("POSTGRES_PASSWORD={PASSWORD}")])
            .args(["-p", "127.0.0.1::5432", image])
            // Both servers plus the harness share this instance; durability is irrelevant.
            .args([
                "-c",
                "max_connections=500",
                "-c",
                "fsync=off",
                "-c",
                "synchronous_commit=off",
            ])
            .output()
            .context("run `docker run` (is Docker running?)")?;
        if !out.status.success() {
            bail!(
                "docker run {image} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        // From here on the guard owns the container, so every early return removes it.
        let mut pg = Self { container, port: 0 };
        let out = Command::new("docker")
            .args(["port", &pg.container, "5432/tcp"])
            .output()
            .context("docker port")?;
        let mapping = String::from_utf8_lossy(&out.stdout);
        pg.port = mapping
            .lines()
            .find_map(|l| l.rsplit(':').next().and_then(|p| p.trim().parse().ok()))
            .with_context(|| format!("could not read the published port from `{mapping}`"))?;
        pg.wait_ready().await?;
        tracing::info!(container = %pg.container, port = pg.port, "postgres ready");
        Ok(pg)
    }

    async fn wait_ready(&self) -> Result<()> {
        let deadline = Instant::now() + READY_BUDGET;
        loop {
            // The image's entrypoint runs a socket-only bootstrap server first;
            // a TCP connection that succeeds is the real one.
            if let Ok(mut c) = PgConnection::connect(&self.url("postgres")).await {
                if sqlx::query("SELECT 1").execute(&mut c).await.is_ok() {
                    let _ = c.close().await;
                    return Ok(());
                }
            }
            if Instant::now() > deadline {
                bail!(
                    "postgres in {} not ready after {READY_BUDGET:?}",
                    self.container
                );
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub fn container(&self) -> &str {
        &self.container
    }

    /// A libpq URL both Go's pgx and Rust's sqlx read.
    pub fn url(&self, database: &str) -> String {
        format!(
            "postgresql://postgres:{PASSWORD}@127.0.0.1:{}/{database}?sslmode=disable",
            self.port
        )
    }

    pub async fn execute(&self, database: &str, sql: &str) -> Result<()> {
        let mut c = PgConnection::connect(&self.url(database)).await?;
        sqlx::raw_sql(sql)
            .execute(&mut c)
            .await
            .with_context(|| format!("{database}: {sql}"))?;
        c.close().await?;
        Ok(())
    }

    pub async fn create_database(&self, name: &str) -> Result<()> {
        self.execute("postgres", &format!("CREATE DATABASE {}", safe(name)?))
            .await
    }

    /// Template copies are byte-identical: every seeded id and timestamp is
    /// the same on both sides.
    pub async fn create_from_template(&self, name: &str, template: &str) -> Result<()> {
        self.execute(
            "postgres",
            &format!(
                "CREATE DATABASE {} TEMPLATE {}",
                safe(name)?,
                safe(template)?
            ),
        )
        .await
    }
}

fn safe(name: &str) -> Result<&str> {
    let ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        bail!("unsafe database name: {name}");
    }
    Ok(name)
}

impl Drop for DockerPg {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container])
            .output();
    }
}

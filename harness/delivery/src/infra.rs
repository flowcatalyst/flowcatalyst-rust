//! Docker infrastructure: one Postgres (a database per side) and one
//! LocalStack SQS emulator per run. Containers get unique names and random
//! host ports so concurrent runs (and other agents' Docker tests) never
//! collide; both are removed when the `Infra` is dropped.

use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

pub const POSTGRES_IMAGE: &str = "postgres:17";
pub const LOCALSTACK_IMAGE: &str = "localstack/localstack:3.0";

pub struct Infra {
    pub run_id: String,
    pub pg_container: String,
    pub sqs_container: String,
    pub pg_port: u16,
    pub sqs_port: u16,
    keep: bool,
}

fn docker(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("docker")
        .args(args)
        .output()
        .context("running docker")?;
    if !out.status.success() {
        bail!(
            "docker {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn host_port(container: &str, port: &str) -> anyhow::Result<u16> {
    let s = docker(&["port", container, port])?;
    // "0.0.0.0:55012\n[::]:55012"
    let first = s.lines().next().unwrap_or_default();
    let p = first
        .rsplit(':')
        .next()
        .context("no port")?
        .parse::<u16>()?;
    Ok(p)
}

impl Infra {
    pub fn start(run_id: &str, keep: bool) -> anyhow::Result<Infra> {
        let pg_container = format!("fc-harness-delivery-pg-{run_id}");
        let sqs_container = format!("fc-harness-delivery-sqs-{run_id}");
        docker(&[
            "run",
            "-d",
            "--rm",
            "--name",
            &pg_container,
            "-e",
            "POSTGRES_PASSWORD=harness",
            "-e",
            "POSTGRES_USER=harness",
            "-e",
            "POSTGRES_DB=harness",
            "-p",
            "127.0.0.1::5432",
            POSTGRES_IMAGE,
            "-c",
            "max_connections=400",
            "-c",
            "fsync=off",
            "-c",
            "synchronous_commit=off",
        ])?;
        // Created before the second container so a failure there still
        // removes the first (Drop).
        let mut infra = Infra {
            run_id: run_id.to_string(),
            pg_container: pg_container.clone(),
            sqs_container: sqs_container.clone(),
            pg_port: 0,
            sqs_port: 0,
            keep,
        };
        docker(&[
            "run",
            "-d",
            "--rm",
            "--name",
            &sqs_container,
            "-e",
            "SERVICES=sqs",
            "-e",
            "DEBUG=0",
            "-e",
            "EAGER_SERVICE_LOADING=1",
            // Queue URLs in the path style with the host the harness
            // reaches (localhost), not localhost.localstack.cloud.
            "-e",
            "SQS_ENDPOINT_STRATEGY=path",
            "-p",
            "127.0.0.1::4566",
            LOCALSTACK_IMAGE,
        ])?;
        infra.pg_port = host_port(&pg_container, "5432/tcp")?;
        infra.sqs_port = host_port(&sqs_container, "4566/tcp")?;
        infra.wait_ready()?;
        Ok(infra)
    }

    fn wait_ready(&self) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let ok = Command::new("docker")
                .args([
                    "exec",
                    &self.pg_container,
                    "pg_isready",
                    "-U",
                    "harness",
                    "-d",
                    "harness",
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                break;
            }
            if Instant::now() > deadline {
                bail!("postgres not ready");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        loop {
            let ok = Command::new("curl")
                .args([
                    "-sf",
                    &format!("http://127.0.0.1:{}/_localstack/health", self.sqs_port),
                ])
                .output()
                .map(|o| {
                    o.status.success() && String::from_utf8_lossy(&o.stdout).contains("\"sqs\"")
                })
                .unwrap_or(false);
            if ok {
                break;
            }
            if Instant::now() > deadline {
                bail!("localstack not ready");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(())
    }

    pub fn create_database(&self, name: &str) -> anyhow::Result<()> {
        docker(&[
            "exec",
            &self.pg_container,
            "psql",
            "-U",
            "harness",
            "-d",
            "harness",
            "-c",
            &format!("CREATE DATABASE {name}"),
        ])?;
        Ok(())
    }

    pub fn database_url(&self, name: &str) -> String {
        format!(
            "postgres://harness:harness@127.0.0.1:{}/{name}",
            self.pg_port
        )
    }

    pub fn sqs_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.sqs_port)
    }
}

impl Drop for Infra {
    fn drop(&mut self) {
        if self.keep {
            eprintln!(
                "keeping containers {} and {}",
                self.pg_container, self.sqs_container
            );
            return;
        }
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.pg_container, &self.sqs_container])
            .output();
    }
}

/// A free TCP port on loopback (bind :0, read, release).
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(0)
}

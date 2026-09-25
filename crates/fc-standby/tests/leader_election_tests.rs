//! Leader election against a real Redis (testcontainers), through a TCP
//! proxy the test can freeze to simulate a Redis that stops answering.
//!
//! Requires Docker; ignored by default:
//!   cargo test -p fc-standby --test leader_election_tests -- --ignored

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fc_standby::{LeaderElection, LeaderElectionConfig};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A byte-copying TCP proxy. While `frozen` is set it stops forwarding in
/// both directions but keeps every socket open — a hung Redis, not a dead
/// one, which is the case a connection error alone never reports.
async fn start_proxy(upstream: String, frozen: Arc<AtomicBool>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                return;
            };
            let Ok(server) = TcpStream::connect(&upstream).await else {
                continue;
            };
            let (cr, cw) = client.into_split();
            let (sr, sw) = server.into_split();
            tokio::spawn(pump(cr, sw, frozen.clone()));
            tokio::spawn(pump(sr, cw, frozen.clone()));
        }
    });
    port
}

async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    frozen: Arc<AtomicBool>,
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        while frozen.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        while frozen.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if to.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

/// H13: a Redis that stops answering must not leave this instance calling
/// itself leader while its lock expires and another instance takes it.
/// Go's go-redis times a command out after 3s and its election demotes on
/// the error; here the command times out (response timeout) and, as a
/// second line, `is_leader()` stops answering true once the last confirmed
/// extend is older than the lock TTL minus a margin.
#[tokio::test]
#[ignore = "requires Docker"]
async fn leader_demotes_when_redis_stops_answering() {
    let container = Redis::default().start().await.expect("start redis");
    let redis_port = container.get_host_port_ipv4(6379).await.unwrap();
    let frozen = Arc::new(AtomicBool::new(false));
    let proxy_port = start_proxy(format!("127.0.0.1:{redis_port}"), frozen.clone()).await;

    let mut config = LeaderElectionConfig::new(format!("redis://127.0.0.1:{proxy_port}"))
        .with_lock_key("fc:test:h13".to_string())
        .with_instance_id("instance-a".to_string());
    config.lock_ttl_seconds = 6;
    config.heartbeat_interval_seconds = 1;

    let election = Arc::new(LeaderElection::new(config).await.expect("election"));
    election.clone().start().await.unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while !election.is_leader() {
        assert!(Instant::now() < deadline, "never acquired leadership");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Redis hangs: nothing is refused, nothing answers.
    frozen.store(true, Ordering::SeqCst);
    let frozen_at = Instant::now();
    while election.is_leader() {
        assert!(
            frozen_at.elapsed() < Duration::from_secs(6),
            "still leader {:?} after Redis stopped answering — the lock (TTL 6s) \
             may already belong to another instance",
            frozen_at.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Demoted before the lock could expire in Redis.
    assert!(frozen_at.elapsed() < Duration::from_secs(6));

    // Redis answers again: the lock is still ours (or free), so leadership
    // comes back without waiting for anything else.
    frozen.store(false, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !election.is_leader() {
        assert!(
            Instant::now() < deadline,
            "leadership must be re-acquired once Redis answers again"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    election.shutdown().await;
}

/// Go's `tryAcquire`: an instance whose own lock is still held re-takes it
/// (extend-if-mine) rather than waiting for the key to expire. A second
/// instance never becomes leader while the first keeps its lock.
#[tokio::test]
#[ignore = "requires Docker"]
async fn only_one_instance_leads_and_the_holder_keeps_its_lock() {
    let container = Redis::default().start().await.expect("start redis");
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://127.0.0.1:{port}");
    let mk = |id: &str| {
        let mut c = LeaderElectionConfig::new(url.clone())
            .with_lock_key("fc:test:single".to_string())
            .with_instance_id(id.to_string());
        c.lock_ttl_seconds = 6;
        c.heartbeat_interval_seconds = 1;
        c
    };
    let a = Arc::new(LeaderElection::new(mk("a")).await.unwrap());
    a.clone().start().await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !a.is_leader() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let b = Arc::new(LeaderElection::new(mk("b")).await.unwrap());
    b.clone().start().await.unwrap();
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(a.is_leader(), "the holder must keep extending its lock");
        assert!(!b.is_leader(), "two leaders at once");
    }
    a.shutdown().await;
    b.shutdown().await;
}

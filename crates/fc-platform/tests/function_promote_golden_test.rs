//! Promote wiring against what Java's own code answers
//! (`tests/data/function/promote-golden.json`, written by
//! `tests/java/…/api/PromoteGoldenGen.java` from Java's compiled classes,
//! whose sources are byte-identical to the pin `0118cdca`):
//!
//! - the trigger keys and pool code (`FunctionTriggerSync.fid` / `hash8`);
//! - the cron dialect: every cron a manifest may declare, stored as promote
//!   stores it and walked by the Rust scheduler's own reader, fires at the
//!   instants Java's `CronExpression.next` gives, in region and fixed-offset
//!   zones, across a daylight-saving gap;
//! - the `plan` of `manifest/check`, byte for byte
//!   (`FunctionApi.PromotePlanResponse`);
//! - delivery signing: the platform's signer gives Java `WebhookSigner`'s
//!   bytes, and what both deliveries send passes the function host's own
//!   webhook verifier.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use fc_platform::dispatch_job::delivery_credentials::Resolved;
use fc_platform::function::cron_dialect::scheduler_crons;
use fc_platform::function::operations::promote_plan::{
    Conflict, PoolAction, PromotePlan, PublicRoutesAction, RouteKey, ScheduleAction,
    SubscriptionAction, Wiring,
};
use fc_platform::function::operations::trigger_sync::{
    fid, pool_key, schedule_key, subscription_key,
};
use fc_platform::function::version_api::PromotePlanResponse;
use fc_platform::scheduled_job::scheduler::dispatcher::signed_request;
use fc_platform::scheduled_job::scheduler::poller::{latest_slot_in_window, next_slot_after};
use fc_platform::service_account::outbound_credentials::OutboundCredentials;
use fc_platform::shared::dispatch_process_api::apply_credentials;
use fc_platform::shared::webhook_signer;

fn golden() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/function/promote-golden.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn instant(v: &Value) -> DateTime<Utc> {
    v.as_str().unwrap().parse().unwrap()
}

#[test]
fn trigger_keys_are_javas() {
    let golden = golden();
    for k in golden["keys"].as_array().unwrap() {
        let function_id = k["functionId"].as_str().unwrap();
        assert_eq!(fid(function_id), k["fid"].as_str().unwrap());
        assert_eq!(pool_key(function_id), k["pool"].as_str().unwrap());
        for (event_type, key) in k["subscriptions"].as_object().unwrap() {
            assert_eq!(
                subscription_key(function_id, event_type),
                key.as_str().unwrap(),
                "{event_type}"
            );
        }
        for s in k["schedules"].as_array().unwrap() {
            assert_eq!(
                schedule_key(
                    function_id,
                    s["cron"].as_str().unwrap(),
                    s["timezone"].as_str()
                ),
                s["key"].as_str().unwrap(),
                "{s}"
            );
        }
    }
}

/// Promote stores `scheduler_crons(cron)`; the poller's reader walks it.
#[test]
fn stored_crons_fire_on_the_rust_scheduler_when_java_fires_them() {
    let golden = golden();
    let start = instant(&golden["start"]);
    let cases = golden["cron"].as_array().unwrap();
    assert!(cases.len() > 100, "{} cases", cases.len());
    for case in cases {
        let cron = case["cron"].as_str().unwrap();
        let zone = case["zone"].as_str().unwrap();
        let want: Vec<DateTime<Utc>> = case["fires"]
            .as_array()
            .unwrap()
            .iter()
            .map(instant)
            .collect();
        let stored = scheduler_crons(cron).unwrap_or_else(|e| panic!("{cron}: {e:?}"));

        let mut got = Vec::new();
        let mut t = start;
        for _ in 0..want.len() {
            let next = next_slot_after(&stored, zone, t)
                .unwrap_or_else(|e| panic!("{cron} as {stored:?} in {zone}: {e}"))
                .unwrap_or_else(|| panic!("{cron} as {stored:?} in {zone}: no next slot"));
            got.push(next);
            t = next;
        }
        assert_eq!(got, want, "{cron} (stored {stored:?}) in {zone}");

        // The poller's own question: the latest slot in (start, last].
        let last = *want.last().unwrap();
        assert_eq!(
            latest_slot_in_window(&stored, zone, start, last).unwrap(),
            Some(last),
            "{cron} in {zone}"
        );
    }
}

/// The control: stored as written, these would fire at other instants (or
/// not at all) on the Rust scheduler.
#[test]
fn stored_as_written_they_would_not() {
    let golden = golden();
    let start = instant(&golden["start"]);
    for cron in [
        "0 0 9 * * 1-5",
        "0 0 0 13 * 5",
        "0 0 12 ? * 0",
        "0 30 8 1-7 * mon",
    ] {
        let case = golden["cron"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["cron"] == cron && c["zone"] == "UTC")
            .unwrap();
        let java: Vec<DateTime<Utc>> = case["fires"]
            .as_array()
            .unwrap()
            .iter()
            .map(instant)
            .collect();
        let raw = vec![cron.to_string()];
        let mut t = start;
        let mut got = Vec::new();
        for _ in 0..java.len() {
            match next_slot_after(&raw, "UTC", t) {
                Ok(Some(next)) => {
                    got.push(next);
                    t = next;
                }
                _ => break,
            }
        }
        assert_ne!(got, java, "{cron}");
    }
}

fn route(hostname: &str, path_prefix: &str, prefixes: &[&str]) -> RouteKey {
    RouteKey {
        hostname: hostname.into(),
        path_prefix: path_prefix.into(),
        alias_prefixes: prefixes.iter().map(|p| p.to_string()).collect(),
    }
}

/// The same plans Java's generator builds, by name.
fn plans() -> Vec<(&'static str, PromotePlan)> {
    let sub = |action: &str, key: &str, et: &str| {
        let (trigger_key, event_type) = (key.to_string(), et.to_string());
        match action {
            "create" => SubscriptionAction::Create {
                trigger_key,
                event_type,
            },
            "update" => SubscriptionAction::Update {
                trigger_key,
                event_type,
                changed_fields: vec!["subscription".into()],
            },
            "unchanged" => SubscriptionAction::Unchanged {
                trigger_key,
                event_type,
            },
            _ => SubscriptionAction::Delete {
                trigger_key,
                event_type,
            },
        }
    };
    let tz = |t: Option<&str>| t.map(str::to_string);
    vec![
        (
            "live",
            PromotePlan {
                alias: "live".into(),
                from_version: None,
                to_version: 1,
                settings_missing: vec!["GREETING".into(), "API_KEY".into()],
                wiring: Wiring::Live {
                    pool: PoolAction::Create {
                        key: "fn-abc".into(),
                    },
                    subscriptions: vec![
                        sub("create", "fn-abc-11111111", "a:b:c:created"),
                        sub("update", "fn-abc-22222222", "a:b:c:updated"),
                        sub("unchanged", "fn-abc-33333333", "a:b:c:kept"),
                        sub("delete", "fn-abc-44444444", "a:b:c:dropped"),
                    ],
                    schedules: vec![
                        ScheduleAction::Create {
                            trigger_key: "fn-abc-55555555".into(),
                            cron: "0 0 * * * *".into(),
                            timezone: None,
                        },
                        ScheduleAction::Update {
                            trigger_key: "fn-abc-66666666".into(),
                            cron: "0 0 9 * * 1-5".into(),
                            timezone: tz(Some("Europe/Amsterdam")),
                            changed_fields: vec!["definition".into()],
                        },
                        ScheduleAction::Unchanged {
                            trigger_key: "fn-abc-77777777".into(),
                            cron: "0 0 1 * * *".into(),
                            timezone: None,
                        },
                        ScheduleAction::Delete {
                            trigger_key: "fn-abc-88888888".into(),
                            cron: "0 0 2 * * *".into(),
                            timezone: tz(Some("UTC")),
                        },
                    ],
                    public_routes: PublicRoutesAction::Replace {
                        added: vec![
                            route("api.acme.com", "/", &[]),
                            route("acme.com", "/v2", &["qa", "dev"]),
                        ],
                        removed: vec![route("old.acme.com", "/api", &["qa"])],
                    },
                },
                conflicts: vec![Conflict {
                    code: "PUBLIC_ROUTE_TAKEN".into(),
                    message: "route 'api.acme.com/' is already taken".into(),
                }],
            },
        ),
        (
            "liveUnchanged",
            PromotePlan {
                alias: "live".into(),
                from_version: Some(3),
                to_version: 4,
                settings_missing: vec![],
                wiring: Wiring::Live {
                    pool: PoolAction::Update {
                        key: "fn-abc".into(),
                        changed_fields: vec!["maxConcurrency".into()],
                    },
                    subscriptions: vec![],
                    schedules: vec![],
                    public_routes: PublicRoutesAction::Unchanged,
                },
                conflicts: vec![],
            },
        ),
        (
            "poolUnchanged",
            PromotePlan {
                alias: "live".into(),
                from_version: Some(4),
                to_version: 5,
                settings_missing: vec![],
                wiring: Wiring::Live {
                    pool: PoolAction::Unchanged {
                        key: "fn-abc".into(),
                    },
                    subscriptions: vec![],
                    schedules: vec![],
                    public_routes: PublicRoutesAction::Unchanged,
                },
                conflicts: vec![Conflict {
                    code: "TRIGGER_KEY_COLLISION".into(),
                    message: "two subscriptions entries collide".into(),
                }],
            },
        ),
        (
            "named",
            PromotePlan {
                alias: "qa".into(),
                from_version: Some(2),
                to_version: 3,
                settings_missing: vec!["GREETING".into()],
                wiring: Wiring::HttpOnly,
                conflicts: vec![],
            },
        ),
        (
            "namedUnset",
            PromotePlan {
                alias: "qa".into(),
                from_version: None,
                to_version: 1,
                settings_missing: vec![],
                wiring: Wiring::HttpOnly,
                conflicts: vec![],
            },
        ),
    ]
}

#[test]
fn the_plan_is_javas_bytes() {
    let golden = golden();
    let java = golden["plans"].as_object().unwrap();
    let plans = plans();
    assert_eq!(plans.len(), java.len());
    for (name, plan) in plans {
        let rust = serde_json::to_string(&PromotePlanResponse::of(&plan)).unwrap();
        assert_eq!(rust, java[name].as_str().unwrap(), "{name}");
    }
}

fn header(request: &reqwest::Request, name: &str) -> Option<String> {
    request
        .headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_string())
}

#[test]
fn signatures_are_javas_and_the_function_host_accepts_them() {
    let golden = golden();
    for s in golden["signatures"].as_array().unwrap() {
        let secret = s["secret"].as_str().unwrap();
        let body = s["body"].as_str().unwrap().as_bytes();
        let at = instant(&s["at"]);
        let timestamp = webhook_signer::timestamp(at);
        assert_eq!(timestamp, s["timestamp"].as_str().unwrap());
        let signature = webhook_signer::sign(secret, &timestamp, body);
        assert_eq!(signature, s["signature"].as_str().unwrap());
        assert_eq!(
            fc_fnhost_core::listener::webhook::verify(
                body,
                Some(&signature),
                Some(&timestamp),
                Some(secret),
                None,
                at.timestamp()
            ),
            Ok(())
        );
    }
}

/// Both deliveries, as built: a subscription's (the dispatch processing
/// endpoint) and a scheduled job's (the dispatcher). The host reads the
/// headers case-insensitively, as HTTP does.
#[test]
fn both_deliveries_pass_the_function_hosts_webhook_verifier() {
    let client = reqwest::Client::new();
    let secret = "whsec_app_MARKER";
    let now = Utc::now();

    let body = br#"{"id":"evt_1","type":"a:b:c:d"}"#;
    let credentials = Resolved {
        bearer_token: Some("tok".into()),
        signing_secret: Some(secret.into()),
        reason: String::new(),
        signed_by: Some("app-sa".into()),
    };
    let request = apply_credentials(
        client.post("http://fn-default:8080/functions/a.b.c/events"),
        &credentials,
        now,
        body,
    )
    .body(body.to_vec())
    .build()
    .unwrap();
    let sent = request.body().unwrap().as_bytes().unwrap().to_vec();
    assert_eq!(
        header(&request, "authorization").as_deref(),
        Some("Bearer tok")
    );
    assert_eq!(
        fc_fnhost_core::listener::webhook::verify(
            &sent,
            header(&request, "x-flowcatalyst-signature").as_deref(),
            header(&request, "x-flowcatalyst-timestamp").as_deref(),
            Some(secret),
            None,
            now.timestamp()
        ),
        Ok(())
    );

    let job_body = br#"{"jobCode":"fn-abc-12345678","payload":{"x":1}}"#.to_vec();
    let creds = OutboundCredentials {
        token: None,
        signing_secret: Some(secret.into()),
        signed_by: "app-sa".into(),
    };
    let request = signed_request(
        client.post("http://fn-default:8080/functions/a.b.c/jobs/tick"),
        Some(&creds),
        now,
        job_body.clone(),
    )
    .build()
    .unwrap();
    let sent = request.body().unwrap().as_bytes().unwrap().to_vec();
    assert_eq!(sent, job_body);
    assert_eq!(header(&request, "authorization"), None);
    assert_eq!(
        header(&request, "content-type").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        fc_fnhost_core::listener::webhook::verify(
            &sent,
            header(&request, "x-flowcatalyst-signature").as_deref(),
            header(&request, "x-flowcatalyst-timestamp").as_deref(),
            Some(secret),
            None,
            now.timestamp()
        ),
        Ok(())
    );

    // Unsigned when there is no secret: the host refuses it.
    let request = signed_request(client.post("http://h/"), None, now, b"{}".to_vec())
        .build()
        .unwrap();
    assert_eq!(header(&request, "x-flowcatalyst-signature"), None);
    assert_eq!(
        fc_fnhost_core::listener::webhook::verify(
            b"{}",
            None,
            None,
            Some(secret),
            None,
            now.timestamp()
        ),
        Err("MISSING_SIGNATURE")
    );
}

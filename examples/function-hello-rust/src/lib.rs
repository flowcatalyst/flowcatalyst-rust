//! An adapter function: when an order is placed, book its shipment with the
//! carrier and announce it.
//!
//! 1. The platform delivers `shop:orders:order:placed` to
//!    `POST /events/order-placed` (a subscription, so a signed webhook the
//!    host has already verified).
//! 2. The order is mapped onto the carrier's shipment request: the address
//!    reshaped, the country code normalised, the lines' prices summed into a
//!    declared value.
//! 3. The shipment is booked over HTTPS (`CARRIER_API_URL`, allowed by the
//!    manifest's `httpAllow`), authenticated with the `CARRIER_API_KEY`
//!    secret, idempotent on the order id.
//! 4. `shop:fulfilment:shipment:requested` is emitted, deduplicated on the
//!    order id, so a redelivery never announces a shipment twice.
//!
//! A carrier or platform hiccup answers `retry` (the delivery is deferred,
//! not failed); anything that retrying cannot fix fails the attempt.

use std::time::Duration;

use fc_function_pdk::prelude::*;
use serde::{Deserialize, Serialize};

/// How long the platform should wait before redelivering after a transient
/// failure.
const RETRY_AFTER: Duration = Duration::from_secs(30);

#[handler]
async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    match (req.method(), req.path()) {
        ("POST", "/events/order-placed") => order_placed(req, ctx).await,
        ("GET", "/healthz") => json(200, &serde_json::json!({"ok": true})),
        _ => Ok(Response::json(404, r#"{"error":"not found"}"#)?),
    }
}

// ── the inbound event ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderPlaced {
    order_id: String,
    customer: Customer,
    lines: Vec<OrderLine>,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct Customer {
    name: String,
    address: Address,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Address {
    line1: String,
    line2: Option<String>,
    city: String,
    postcode: String,
    country: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderLine {
    sku: String,
    quantity: u32,
    unit_price_cents: u64,
}

// ── the carrier's API ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ShipmentRequest {
    account: String,
    reference: String,
    recipient: Recipient,
    parcels: Vec<Parcel>,
    declared_value: Money,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Recipient {
    name: String,
    street: String,
    city: String,
    postal_code: String,
    country_code: String,
}

#[derive(Debug, Serialize, PartialEq)]
struct Parcel {
    sku: String,
    qty: u32,
}

#[derive(Debug, Serialize, PartialEq)]
struct Money {
    amount: String,
    currency: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Booked {
    shipment_id: String,
    tracking_number: String,
}

// ── the outbound event ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShipmentRequested<'a> {
    order_id: &'a str,
    shipment_id: &'a str,
    tracking_number: &'a str,
    carrier_account: &'a str,
}

async fn order_placed(req: Request, ctx: Context) -> Result<Response, Error> {
    let delivery = Webhook::event(&req)?;
    let data = delivery
        .data_json
        .as_deref()
        .ok_or_else(|| Error::msg("the order-placed event has no data"))?;
    let order: OrderPlaced = serde_json::from_str(data)?;
    let log = ctx.logger();
    log.info(format_args!(
        "order {} (delivery {}, attempt {})",
        order.order_id, delivery.id, delivery.attempt_number
    ));

    let account = ctx.config().require("CARRIER_ACCOUNT")?;
    let shipment = shipment_request(&order, &account);
    let call = HttpCall::post(format!(
        "{}/v1/shipments",
        ctx.config()
            .require("CARRIER_API_URL")?
            .trim_end_matches('/')
    ))
    .with_bearer(ctx.secrets().require("CARRIER_API_KEY")?)
    .with_header("idempotency-key", format!("order-{}", order.order_id))
    .with_json(&shipment)?
    .with_timeout(Duration::from_secs(5));

    let reply = match ctx.http().send(call).await {
        Ok(reply) => reply,
        Err(e @ (HttpError::Timeout | HttpError::Failed(_))) => {
            log.warn(format_args!(
                "the carrier is unreachable ({e}); asking for a redelivery"
            ));
            return Ok(Response::retry(RETRY_AFTER));
        }
        // Denied or malformed: the manifest or the config is wrong, which a
        // retry cannot fix.
        Err(other) => return Err(other.into()),
    };
    if reply.status() == 429 || reply.status() >= 500 {
        log.warn(format_args!(
            "the carrier answered {}; retrying later",
            reply.status()
        ));
        return Ok(Response::retry(RETRY_AFTER));
    }
    if !reply.is_success() {
        return Err(Error::msg(format!(
            "the carrier refused the shipment: {} {}",
            reply.status(),
            reply.text().unwrap_or("<binary>")
        )));
    }
    let booked: Booked = reply.json()?;

    let event = OutboundEvent::new(
        "shop:fulfilment:shipment:requested",
        format!("shipment-requested-{}", order.order_id),
    )?
    .with_subject(format!("order/{}", order.order_id))
    .with_message_group(format!("order-{}", order.order_id))
    .with_json(&ShipmentRequested {
        order_id: &order.order_id,
        shipment_id: &booked.shipment_id,
        tracking_number: &booked.tracking_number,
        carrier_account: &account,
    })?;
    // The correlation and causation ids default to the inbound event's.
    match ctx.events().emit(&event) {
        Ok(_event_id) => {}
        Err(e) if e.is_retryable() => return Ok(Response::retry(RETRY_AFTER)),
        Err(e) => return Err(e.into()),
    }
    log.info(format_args!(
        "order {} shipped as {}",
        order.order_id, booked.tracking_number
    ));
    Ok(Response::ack())
}

/// The carrier's view of an order.
fn shipment_request(order: &OrderPlaced, account: &str) -> ShipmentRequest {
    let address = &order.customer.address;
    let street = match address.line2.as_deref().map(str::trim) {
        Some(line2) if !line2.is_empty() => format!("{}, {line2}", address.line1.trim()),
        _ => address.line1.trim().to_owned(),
    };
    let total: u64 = order
        .lines
        .iter()
        .map(|l| l.unit_price_cents * u64::from(l.quantity))
        .sum();
    ShipmentRequest {
        account: account.to_owned(),
        reference: order.order_id.clone(),
        recipient: Recipient {
            name: order.customer.name.trim().to_owned(),
            street,
            city: address.city.trim().to_owned(),
            postal_code: address.postcode.replace(' ', "").to_uppercase(),
            country_code: address.country.trim().to_uppercase(),
        },
        parcels: order
            .lines
            .iter()
            .filter(|l| l.quantity > 0)
            .map(|l| Parcel {
                sku: l.sku.clone(),
                qty: l.quantity,
            })
            .collect(),
        declared_value: Money {
            amount: format!("{}.{:02}", total / 100, total % 100),
            currency: order.currency.to_uppercase(),
        },
    }
}

/// Native unit tests: `cargo test` (no wasm toolchain needed), with the
/// PDK's `TestHost` standing in for the FlowCatalyst host.
#[cfg(test)]
mod tests {
    use super::*;
    use fc_function_pdk::testing::TestHost;
    use fc_function_pdk::{block_on, EmitError, MultiMap};

    const DELIVERY: &str = r#"{
        "id": "evt-1", "type": "shop:orders:order:placed", "attemptNumber": 1,
        "correlationId": "flow-1",
        "data": {
            "orderId": "ord-42", "currency": "eur",
            "customer": {"name": " Ada Lovelace ", "address": {
                "line1": "1 Analytical Way", "line2": "Flat 2", "city": "London",
                "postcode": "nw1 6xe", "country": "gb"}},
            "lines": [
                {"sku": "ENGINE", "quantity": 1, "unitPriceCents": 12999},
                {"sku": "PUNCHCARD", "quantity": 3, "unitPriceCents": 250},
                {"sku": "GIFTWRAP", "quantity": 0, "unitPriceCents": 100}
            ]
        }
    }"#;

    fn host() -> TestHost {
        TestHost::new()
            .config("CARRIER_API_URL", "https://api.carrier.example/")
            .config("CARRIER_ACCOUNT", "acct-7")
            .secret("CARRIER_API_KEY", "k-123")
    }

    fn deliver(host: &TestHost) -> Result<Response, Error> {
        let req = host
            .request("POST", "/events/order-placed")
            .with_body(DELIVERY);
        block_on(handle(req, host.context()))
    }

    fn booked(_: &HttpCall) -> Result<HttpReply, HttpError> {
        Ok(HttpReply::new(
            201,
            MultiMap::new(),
            r#"{"shipmentId":"shp-9","trackingNumber":"TRK-1"}"#,
        ))
    }

    #[test]
    fn an_order_is_booked_with_the_carrier_and_announced() {
        let host = host().http(booked);
        let response = deliver(&host).unwrap();
        assert_eq!(response, Response::ack());

        let calls = host.http_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url(), "https://api.carrier.example/v1/shipments");
        assert_eq!(calls[0].header("authorization"), Some("Bearer k-123"));
        assert_eq!(calls[0].header("idempotency-key"), Some("order-ord-42"));
        let sent: serde_json::Value = serde_json::from_slice(calls[0].body()).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "account": "acct-7",
                "reference": "ord-42",
                "recipient": {
                    "name": "Ada Lovelace", "street": "1 Analytical Way, Flat 2",
                    "city": "London", "postalCode": "NW16XE", "countryCode": "GB"
                },
                "parcels": [{"sku": "ENGINE", "qty": 1}, {"sku": "PUNCHCARD", "qty": 3}],
                "declaredValue": {"amount": "137.49", "currency": "EUR"}
            })
        );

        let events = host.emitted();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type(), "shop:fulfilment:shipment:requested");
        assert_eq!(events[0].dedup_id(), "shipment-requested-ord-42");
        let data: serde_json::Value = serde_json::from_slice(events[0].data()).unwrap();
        assert_eq!(data["trackingNumber"], "TRK-1");
    }

    #[test]
    fn a_carrier_outage_asks_for_a_redelivery_and_announces_nothing() {
        let outage = host().http(|_| Ok(HttpReply::new(503, MultiMap::new(), "down")));
        let response = deliver(&outage).unwrap();
        assert_eq!(response.status(), 429);
        assert_eq!(response.headers()["Retry-After"], ["30"]);
        assert!(outage.emitted().is_empty());

        let timeout = host().http(|_| Err(HttpError::Timeout));
        assert_eq!(deliver(&timeout).unwrap().status(), 429);
    }

    #[test]
    fn a_refusal_fails_the_attempt() {
        let host = host().http(|_| Ok(HttpReply::new(422, MultiMap::new(), "bad postcode")));
        let error = deliver(&host).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the carrier refused the shipment: 422 bad postcode"
        );
    }

    #[test]
    fn a_carrier_the_manifest_does_not_allow_is_a_failure_not_a_retry() {
        // TestHost denies every call unless told otherwise, as an empty
        // httpAllow does.
        let error = deliver(&host()).unwrap_err();
        assert!(error.downcast_ref::<HttpError>().unwrap().is_denied());
    }

    #[test]
    fn a_platform_outage_on_emit_asks_for_a_redelivery() {
        let host = host()
            .http(booked)
            .emit_with(|_| Err(EmitError::Unavailable));
        assert_eq!(deliver(&host).unwrap().status(), 429);
    }
}

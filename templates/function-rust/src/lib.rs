//! {{project-name}}: a FlowCatalyst function. See README.md.

use fc_function_pdk::prelude::*;
use serde::Serialize;

#[handler]
async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    match (req.method(), req.path()) {
        ("POST", "/events/greeting-requested") => greeting_requested(req, ctx),
        ("GET", _) if req.path_param("name").is_some() => hello(req, ctx),
        _ => Ok(Response::json(404, r#"{"error":"not found"}"#)?),
    }
}

#[derive(Serialize)]
struct Greeting {
    message: String,
}

/// `GET /hello/{name}`: a greeting, from the `GREETING` config value.
fn hello(req: Request, ctx: Context) -> Result<Response, Error> {
    let greeting = ctx
        .config()
        .get("GREETING")
        .unwrap_or_else(|| "Hello".into());
    let name = req.path_param("name").unwrap_or("world");
    json(
        200,
        &Greeting {
            message: format!("{greeting}, {name}!"),
        },
    )
}

/// `POST /events/greeting-requested`: a subscription delivery. Answers
/// `ack` once handled; an `Err` fails the attempt, `Response::retry` defers it.
fn greeting_requested(req: Request, ctx: Context) -> Result<Response, Error> {
    let event = Webhook::event(&req)?;
    ctx.logger()
        .info(format_args!("event {} ({})", event.id, event.event_type));
    Ok(Response::ack())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_function_pdk::block_on;
    use fc_function_pdk::testing::TestHost;

    #[test]
    fn says_hello_with_the_configured_greeting() {
        let host = TestHost::new()
            .config("GREETING", "Hi")
            .path_param("name", "Ada");
        let response = block_on(handle(host.request("GET", "/hello/Ada"), host.context())).unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), br#"{"message":"Hi, Ada!"}"#);
    }

    #[test]
    fn acks_a_delivery() {
        let host = TestHost::new().caller(Caller::Platform);
        let req = host
            .request("POST", "/events/greeting-requested")
            .with_body(
                r#"{"id":"evt-1","type":"hello:greeting:greeting:requested","attemptNumber":1}"#,
            );
        assert_eq!(
            block_on(handle(req, host.context())).unwrap(),
            Response::ack()
        );
        assert_eq!(
            host.logs()[0].1,
            "event evt-1 (hello:greeting:greeting:requested)"
        );
    }
}

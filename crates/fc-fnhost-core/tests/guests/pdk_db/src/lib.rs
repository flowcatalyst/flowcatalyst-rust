//! `pdk_db`: database access written with `fc-function-pdk`
//! (`flowcatalyst:function/db`, 0.1.2), for `tests/it/wasm_db.rs`. The
//! manifest declares one database, `main`. Every route answers 200 with
//! JSON; a database error is `{"error": code, "message": …}`.
//!
//! - `POST /setup`: creates the `items` table (the body names it);
//! - `POST /items?table=T&id=N&name=S`: one INSERT, outside a transaction;
//! - `GET /items?table=T`: every row, as the host's row JSON;
//! - `POST /tx/commit?table=T&id=N`: INSERT in a transaction, committed;
//! - `POST /tx/drop?table=T&id=N`: the same, then the guard dropped;
//! - `POST /tx/leak?table=T&id=N`: the same, the transaction leaked
//!   (`mem::forget`) and the handler returns: the invocation's end rolls
//!   it back;
//! - `GET /sql?q=SQL`: runs `q`, the error or the rows;
//! - `GET /open?db=N`: opens `N` (`main` is declared; every route takes
//!   `db`);
//! - `GET /types`: one row of typed parameters and values.

use fc_function_pdk::prelude::*;
use fc_function_pdk::DbError;
use serde_json::{json as j, Value};

fn failed(e: DbError) -> Result<Response, Error> {
    json(200, &j!({"error": e.code(), "message": e.message()}))
}

fn table(req: &Request) -> String {
    req.query_param("table").unwrap_or("items").to_owned()
}

fn id(req: &Request) -> i64 {
    req.query_param("id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[handler]
fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    let db = match ctx.db(req.query_param("db").unwrap_or("main")) {
        Ok(db) => db,
        Err(e) => return failed(e),
    };
    let t = table(&req);
    let outcome: Result<Value, DbError> = match req.path() {
        "/open" => Ok(j!({"opened": db.name()})),
        "/setup" => db
            .execute(&format!("CREATE TABLE {t} (id int PRIMARY KEY, name text)"), &[])
            .map(|n| j!({"updated": n})),
        "/items" if req.method() == "POST" => db
            .execute(
                &format!("INSERT INTO {t} VALUES (?, ?)"),
                params![id(&req), req.query_param("name").unwrap_or("")],
            )
            .map(|n| j!({"updated": n})),
        "/items" => db
            .query(&format!("SELECT id, name FROM {t} ORDER BY id"), &[])
            .and_then(|rows| {
                Ok(j!({"rows": rows.values()?, "count": rows.len(), "truncated": rows.truncated()}))
            }),
        "/tx/commit" | "/tx/drop" | "/tx/leak" => (|| {
            let tx = db.begin()?;
            tx.execute(
                &format!("INSERT INTO {t} VALUES (?, 'tx')"),
                params![id(&req)],
            )?;
            match req.path() {
                "/tx/commit" => tx.commit()?,
                "/tx/drop" => drop(tx),
                _ => std::mem::forget(tx),
            }
            Ok(j!({"done": req.path()}))
        })(),
        "/sql" => db
            .query(req.query_param("q").unwrap_or(""), &[])
            .and_then(|rows| Ok(j!({"rows": rows.values()?}))),
        "/types" => db
            .query(
                "SELECT ? AS i, ? AS f, ? AS b, ? AS d, ? AS s, ? AS z, ?::uuid AS u, now() > ?::timestamptz AS later",
                params![
                    7,
                    1.5,
                    true,
                    Param::Decimal("12.50".into()),
                    "text",
                    None::<i32>,
                    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
                    "2000-01-01T00:00:00Z"
                ],
            )
            .and_then(|rows| Ok(j!({"rows": rows.values()?}))),
        _ => return Ok(Response::http(404, Default::default(), Vec::new())?),
    };
    match outcome {
        Ok(value) => json(200, &value),
        Err(e) => failed(e),
    }
}

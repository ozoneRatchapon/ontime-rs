mod room;
mod types;

use worker::*;

pub use room::OntimeRoom;

fn add_cors_headers(mut res: Response) -> Result<Response> {
    let headers = res.headers_mut();
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "*")?;
    Ok(res)
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if req.method() == Method::Options {
        let headers = Headers::new();
        let res = Response::empty()?.with_headers(headers);
        return add_cors_headers(res);
    }

    let router = Router::new();

    let res = router
        .get("/", |_req, _ctx| Response::from_html("<h1>🚀 Ontime RS Cloudfire Worker Online</h1><p>API State: <a href='/api/state'>/api/state</a></p><p>Health Check: <a href='/api/health'>/api/health</a></p>"))
        .get("/api/health", |_req, _ctx| Response::ok("Ontime RS Cloudfire Worker Online"))
        .on_async("/room/:room_id/ws", |req, ctx| async move {
            let room_id = ctx.param("room_id").unwrap_or(&"default_room".to_string()).to_string();
            let namespace = ctx.durable_object("ONTIME_ROOM")?;
            let stub = namespace.id_from_name(&room_id)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .on_async("/room/:room_id/api/state", |req, ctx| async move {
            let room_id = ctx.param("room_id").unwrap_or(&"default_room".to_string()).to_string();
            let namespace = ctx.durable_object("ONTIME_ROOM")?;
            let stub = namespace.id_from_name(&room_id)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .on_async("/ws", |req, ctx| async move {
            let url = req.url()?;
            let query_room = url.query_pairs().find(|(k, _)| k == "room").map(|(_, v)| v.to_string());
            let room_id = query_room.unwrap_or_else(|| "default_room".to_string());
            let namespace = ctx.durable_object("ONTIME_ROOM")?;
            let stub = namespace.id_from_name(&room_id)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .on_async("/api/state", |req, ctx| async move {
            let url = req.url()?;
            let query_room = url.query_pairs().find(|(k, _)| k == "room").map(|(_, v)| v.to_string());
            let room_id = query_room.unwrap_or_else(|| "default_room".to_string());
            let namespace = ctx.durable_object("ONTIME_ROOM")?;
            let stub = namespace.id_from_name(&room_id)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .on_async("/data/*path", |req, ctx| async move {
            let url = req.url()?;
            let query_room = url.query_pairs().find(|(k, _)| k == "room").map(|(_, v)| v.to_string());
            let room_id = query_room.unwrap_or_else(|| "default_room".to_string());
            let namespace = ctx.durable_object("ONTIME_ROOM")?;
            let stub = namespace.id_from_name(&room_id)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .on_async("/api/v1/control/*path", |req, ctx| async move {
            let url = req.url()?;
            let query_room = url.query_pairs().find(|(k, _)| k == "room").map(|(_, v)| v.to_string());
            let room_id = query_room.unwrap_or_else(|| "default_room".to_string());
            let namespace = ctx.durable_object("ONTIME_ROOM")?;
            let stub = namespace.id_from_name(&room_id)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        .on_async("/user/*path", |_req, _ctx| async move {
            Response::from_json(&serde_json::json!({}))
        })
        .run(req, env)
        .await?;

    add_cors_headers(res)
}

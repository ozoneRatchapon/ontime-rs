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

    let result = router
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
        .on_async("/data/*path", |mut req, _ctx| async move {
            let url = match req.url() {
                Ok(u) => u,
                Err(e) => return Response::error(format!("URL_ERR: {e}"), 400),
            };
            let path_string = url.path();
            match path_string.as_bytes() {
                b"/data/rundowns" => {
                    let mut title = "Ontime Production Rundown".to_string();
                    if req.method() == Method::Post {
                        if let Ok(text) = req.text().await {
                            if let Ok(json_body) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(t) = json_body.get("title").and_then(|v| v.as_str()) {
                                    title = t.to_string();
                                }
                            }
                        }
                    }
                    Response::from_json(&serde_json::json!({
                        "loaded": "default_rundown",
                        "rundowns": [
                            {
                                "id": "default_rundown",
                                "title": title,
                                "numEntries": 3,
                                "revision": 1
                            }
                        ]
                    }))
                }
                b"/data/rundowns/current" => {
                    Response::from_json(&serde_json::json!({
                        "id": "default_rundown",
                        "title": "Ontime Production Rundown",
                        "order": ["entry_1", "entry_2", "entry_3"],
                        "flatOrder": ["entry_1", "entry_2", "entry_3"],
                        "entries": {
                            "entry_1": {
                                "type": "event",
                                "id": "entry_1",
                                "cue": "1",
                                "title": "Welcome & Intro",
                                "duration": 300000,
                                "subtitle": null,
                                "presenter": null,
                                "note": null,
                                "flag": false,
                                "endAction": "none",
                                "timerType": "countDown",
                                "countToEnd": false,
                                "linkStart": false,
                                "timeStrategy": "relative",
                                "timeStart": 0,
                                "timeEnd": 300000,
                                "skip": false,
                                "colour": "",
                                "timeWarning": 120000,
                                "timeDanger": 60000,
                                "custom": {},
                                "triggers": [],
                                "parent": null,
                                "revision": 1,
                                "delay": 0,
                                "dayOffset": 0,
                                "gap": 0
                            },
                            "entry_2": {
                                "type": "event",
                                "id": "entry_2",
                                "cue": "2",
                                "title": "Keynote Presentation",
                                "duration": 1800000,
                                "subtitle": null,
                                "presenter": null,
                                "note": null,
                                "flag": false,
                                "endAction": "none",
                                "timerType": "countDown",
                                "countToEnd": false,
                                "linkStart": false,
                                "timeStrategy": "relative",
                                "timeStart": 0,
                                "timeEnd": 1800000,
                                "skip": false,
                                "colour": "",
                                "timeWarning": 120000,
                                "timeDanger": 60000,
                                "custom": {},
                                "triggers": [],
                                "parent": null,
                                "revision": 1,
                                "delay": 0,
                                "dayOffset": 0,
                                "gap": 0
                            },
                            "entry_3": {
                                "type": "event",
                                "id": "entry_3",
                                "cue": "3",
                                "title": "Q&A Session",
                                "duration": 600000,
                                "subtitle": null,
                                "presenter": null,
                                "note": null,
                                "flag": false,
                                "endAction": "none",
                                "timerType": "countDown",
                                "countToEnd": false,
                                "linkStart": false,
                                "timeStrategy": "relative",
                                "timeStart": 0,
                                "timeEnd": 600000,
                                "skip": false,
                                "colour": "",
                                "timeWarning": 120000,
                                "timeDanger": 60000,
                                "custom": {},
                                "triggers": [],
                                "parent": null,
                                "revision": 1,
                                "delay": 0,
                                "dayOffset": 0,
                                "gap": 0
                            }
                        },
                        "revision": 1
                    }))
                }
                b"/data/project" => Response::from_json(&serde_json::json!({
                    "title": "Ontime Production Rundown",
                    "description": "Live Event Schedule",
                    "publicUrl": null
                })),
                b"/data/settings" => Response::from_json(&serde_json::json!({
                    "version": "4.11.0",
                    "timeFormat": "24h",
                    "editorKey": null,
                    "operatorKey": null
                })),
                b"/data/view-settings" => Response::from_json(&serde_json::json!({
                    "theme": "dark",
                    "fontSize": 16,
                    "showSeconds": true
                })),
                b"/data/url-presets" => Response::from_json(&serde_json::json!([])),
                b"/data/custom-views" => Response::from_json(&serde_json::json!([])),
                b"/data/session" => Response::from_json(&serde_json::json!({ "activeConnections": 1 })),
                b"/data/session/info" => Response::from_json(&serde_json::json!({ "version": "v3.13.1", "isCloud": true })),
                b"/data/report" => Response::from_json(&serde_json::json!([])),
                b"/data/custom-fields" => Response::from_json(&serde_json::json!({})),
                b"/data/assets/css" => Response::ok(""),
                _ => Response::from_json(&serde_json::json!({})),
            }
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
        .await;

    match result {
        Ok(res) => add_cors_headers(res),
        Err(err) => {
            let res = Response::error(format!("ROUTER_EXEC_ERROR: {err:?}"), 500)?;
            add_cors_headers(res)
        }
    }
}

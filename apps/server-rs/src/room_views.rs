//! A room's custom views (plan 021): single-file HTML pages, stored apart from
//! the project so an ordinary edit never rewrites them, and served from
//! `/r/<room>/external/<name>/` the way the Ontime app serves `/external/`.
//!
//! The checks and the serving are `ontime_core::custom_views`; this is storage
//! and HTTP. The routes and answers are upstream's (`customViews.router.ts`),
//! except that an upload is sent as JSON `{ html }`.

use serde_json::json;
use worker::*;

use ontime_core::api::{self, ApiRequest};
use ontime_core::audit::{Action, Actor, Note, Subject};
use ontime_core::custom_views::{
    self as views, content_security_policy, is_valid_slug, ViewError, DEMO_SLUG, MAX_VIEWS,
};

use super::{now_ms, OntimeProject};

const INDEX_KEY: &str = "custom_views";

/// An error as upstream answers one, `{ message }`, which the client shows.
fn fail(message: impl Into<String>, status: u16) -> Result<Response> {
    Ok(Response::from_json(&json!({ "message": message.into() }))?.with_status(status))
}

fn key(slug: &str) -> String {
    format!("custom_view:{slug}")
}

/// `/data/custom-views…`, the list and its changes.
pub(super) fn is_views_path(path: &str) -> bool {
    path.ends_with("/data/custom-views") || path.contains("/data/custom-views/")
}

/// `/external/<name>/`, a page itself.
pub(super) fn is_page_path(path: &str) -> bool {
    path == "/external" || path.starts_with("/external/")
}

impl OntimeProject {
    async fn view_index(&self) -> Vec<String> {
        self.state
            .storage()
            .get::<Vec<String>>(INDEX_KEY)
            .await
            .unwrap_or_default()
    }

    async fn view_html(&self, slug: &str) -> Option<String> {
        self.state.storage().get::<String>(&key(slug)).await.ok()
    }

    /// Stores a page under a name not taken yet, as upstream refuses to
    /// overwrite one (409).
    async fn add_view(&mut self, slug: &str, html: &str, actor: Actor) -> Result<Response> {
        let mut index = self.view_index().await;
        if index.iter().any(|known| known == slug) {
            return fail(format!("Name \"{slug}\" already exists"), 409);
        }
        if index.len() >= MAX_VIEWS {
            return fail(ViewError::TooMany.message(), 400);
        }
        let mut storage = self.state.storage();
        storage.put(&key(slug), html).await?;
        index.push(slug.to_string());
        index.sort();
        storage.put(INDEX_KEY, &index).await?;
        let now = now_ms();
        let note = Note::new(Action::CustomViewAdd).about(Some(Subject::titled(slug)));
        let writes = self.audit(actor, note, now).await;
        self.persist(Some(writes)).await?;
        Ok(
            Response::from_json(&json!({ "message": format!("Uploaded custom view \"{slug}\"") }))?
                .with_status(201),
        )
    }

    /// `GET /data/custom-views`, `POST …/<name>/upload`, `GET …/<name>/download`,
    /// `POST …/restore-demo` and `DELETE …/<name>`.
    pub(super) async fn views_http(
        &mut self,
        req: &Request,
        access: &ApiRequest,
        actor: Actor,
    ) -> Result<Response> {
        if !api::is_authorised(&self.project, access) {
            return fail("Unauthorized", 401);
        }
        if req.method() != Method::Get && !api::may_control(&self.project, access) {
            return fail(api::NEEDS_SIGN_IN, 403);
        }
        if self.is_new {
            return fail("This room does not exist", 404);
        }
        let path = req.path();
        let tail = path.split("/data/custom-views").nth(1).unwrap_or_default();
        let parts: Vec<&str> = tail.trim_matches('/').split('/').collect();
        match (req.method(), parts.as_slice()) {
            (Method::Get, [""]) => {
                let index = self.view_index().await;
                let list: Vec<_> = index.iter().map(|slug| json!({ "slug": slug })).collect();
                Response::from_json(&json!({ "views": list }))
            }
            (Method::Post, ["restore-demo"]) => {
                // upstream replaces the demo with a fresh copy
                if self.view_index().await.iter().any(|s| s == DEMO_SLUG) {
                    self.remove_view(DEMO_SLUG).await?;
                }
                self.add_view(DEMO_SLUG, views::DEMO_HTML, actor)
                    .await
                    .map(|response| response.with_status(201))
            }
            (_, [slug, ..]) if !is_valid_slug(slug) => fail(ViewError::BadName.message(), 400),
            (Method::Post, [slug, "upload"]) => {
                // the body was read once, into the request the core would get
                let html = access.body.get("html").and_then(|h| h.as_str());
                let Some(html) = html.map(str::to_string) else {
                    return fail("File not found", 422);
                };
                if let Err(error) = views::validate(&html) {
                    return fail(error.message(), error.status());
                }
                let slug = slug.to_string();
                self.add_view(&slug, &html, actor).await
            }
            (Method::Get, [slug, "download"]) => match self.view_html(slug).await {
                Some(html) => {
                    let mut response = Response::ok(html)?;
                    let headers = response.headers_mut();
                    headers.set("Content-Type", "text/html; charset=utf-8")?;
                    headers.set(
                        "Content-Disposition",
                        &format!("attachment; filename=\"{slug}-index.html\""),
                    )?;
                    Ok(response)
                }
                None => fail(format!("Custom view \"{slug}\" not found"), 404),
            },
            (Method::Delete, [slug]) => {
                let slug = slug.to_string();
                if self.remove_view(&slug).await? {
                    let now = now_ms();
                    let note =
                        Note::new(Action::CustomViewRemove).about(Some(Subject::titled(&slug)));
                    let writes = self.audit(actor, note, now).await;
                    self.persist(Some(writes)).await?;
                }
                Ok(Response::empty()?.with_status(204))
            }
            _ => fail("Not found", 404),
        }
    }

    async fn remove_view(&mut self, slug: &str) -> Result<bool> {
        let mut index = self.view_index().await;
        let before = index.len();
        index.retain(|known| known != slug);
        if index.len() == before {
            return Ok(false);
        }
        let mut storage = self.state.storage();
        storage.delete(&key(slug)).await?;
        storage.put(INDEX_KEY, &index).await?;
        Ok(true)
    }

    /// `GET /external/<name>/`: the page, sandboxed, pointed at this room.
    /// Gated as any read of the room is, so a locked room's pages need its
    /// token (`?token=`), which the page then carries to the room.
    pub(super) async fn page_http(&self, req: &Request, access: &ApiRequest) -> Result<Response> {
        if !api::is_authorised(&self.project, access) {
            return fail("Unauthorized", 401);
        }
        let path = req.path();
        let rest = path.trim_start_matches("/external").trim_matches('/');
        let slug = rest.strip_suffix("/index.html").unwrap_or(rest);
        let slug = slug.trim_end_matches("index.html").trim_end_matches('/');
        if self.is_new || !is_valid_slug(slug) {
            return fail(format!("{path} not found"), 404);
        }
        let Some(html) = self.view_html(slug).await else {
            return fail(format!("{path} not found"), 404);
        };
        let origin = req.url()?.origin().ascii_serialization();
        let mut response = Response::ok(views::serve(&html))?;
        let headers = response.headers_mut();
        headers.set("Content-Type", "text/html; charset=utf-8")?;
        headers.set("Content-Security-Policy", &content_security_policy(&origin))?;
        headers.set("X-Content-Type-Options", "nosniff")?;
        headers.set("Referrer-Policy", "no-referrer")?;
        headers.set("Cache-Control", "no-store")?;
        Ok(response)
    }
}

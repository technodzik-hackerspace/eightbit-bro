use std::convert::Infallible;
use std::fs;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use bro_capture::FrameData;
use bro_store::{Poi, Status};
use bro_vision::{NesCanvas, Rect, optimize_poi};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::Context;
use crate::auth::error;

type Ctx = State<Arc<Context>>;

pub fn router() -> Router<Arc<Context>> {
    Router::new()
        .route("/state", get(state))
        .route("/stream", get(stream))
        .route("/events", get(events))
        .route("/preview.jpg", get(preview))
        .route("/config", get(config))
        .route("/review", get(review_list))
        .route("/review/{id}", get(review_get).patch(review_update).delete(review_delete))
        .route("/review/{id}/frame.jpg", get(review_frame))
        .route("/review/{id}/canvas.png", get(review_canvas))
        .route("/review/{id}/pois", post(poi_add))
        .route("/review/{id}/pois/{poi}", delete(poi_delete))
}

/// Internal errors as JSON 500s.
struct ApiError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::warn!(err = %self.0, "api error");
        error(StatusCode::INTERNAL_SERVER_ERROR, &format!("{:#}", self.0))
    }
}

type ApiResult<T = Response> = Result<T, ApiError>;

async fn state(State(ctx): Ctx) -> Json<serde_json::Value> {
    let (state, since) = ctx.live.state();
    let s = &ctx.live.stats;
    let load = |a: &std::sync::atomic::AtomicU64| a.load(std::sync::atomic::Ordering::Relaxed);
    let (items, max_items, skipped) = ctx.store.review_stats();
    Json(json!({
        "path": state.path(),
        "state": state,
        "since": since,
        "source": ctx.live.source,
        "counters": {
            "captured": load(&s.captured),
            "processed": load(&s.processed),
            "skipped": load(&s.skipped),
            "corrupt": load(&s.corrupt),
            "capture_errors": load(&s.capture_errors),
        },
        "rates": ctx.live.rates(),
        "review": { "items": items, "max": max_items, "skipped": skipped },
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn stream(
    State(ctx): Ctx,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let pushes = BroadcastStream::new(ctx.live.subscribe()).filter_map(|push| {
        let push = push.ok()?;
        let data = serde_json::to_value(&push).ok()?;
        let kind = data["type"].as_str()?.to_owned();
        Some(Ok(Event::default().event(kind).data(data.to_string())))
    });
    Sse::new(pushes).keep_alive(KeepAlive::default())
}

#[derive(Deserialize)]
struct Page {
    limit: Option<u32>,
    before: Option<i64>,
}

async fn events(State(ctx): Ctx, Query(page): Query<Page>) -> ApiResult<Json<serde_json::Value>> {
    let events = ctx.store.events(page.limit.unwrap_or(50).min(500), page.before)?;
    Ok(Json(json!({ "events": events })))
}

/// The newest captured frame as-is: no decoding (tech design §7).
async fn preview(State(ctx): Ctx) -> Response {
    let Some(frame) = ctx.live.ring.as_ref().and_then(|r| r.latest()) else {
        return error(StatusCode::NOT_FOUND, "no frame yet");
    };
    let (mime, bytes) = match &frame.data {
        FrameData::Jpeg(b) => ("image/jpeg", b.clone()),
        FrameData::Png(b) => ("image/png", b.clone()),
    };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-store")], bytes.to_vec())
        .into_response()
}

async fn config(State(ctx): Ctx) -> Json<serde_json::Value> {
    Json(json!({ "text": ctx.config_text, "geometry": ctx.geometry }))
}

async fn review_list(State(ctx): Ctx) -> ApiResult<Json<serde_json::Value>> {
    let (items, max_items, skipped) = ctx.store.review_stats();
    let list = ctx.store.review_items()?;
    Ok(Json(
        json!({ "items": list, "stats": { "items": items, "max": max_items, "skipped": skipped } }),
    ))
}

#[derive(Serialize)]
struct PoiView {
    #[serde(flatten)]
    poi: Poi,
    /// The optimized region in capture pixels, for drawing over the frame.
    optimized_capture: Option<Rect>,
}

fn poi_view(ctx: &Context, poi: Poi) -> PoiView {
    let optimized_capture = poi.optimized.map(|r| ctx.geometry.to_capture(r));
    PoiView { poi, optimized_capture }
}

async fn review_get(State(ctx): Ctx, Path(id): Path<i64>) -> ApiResult {
    let Some(item) = ctx.store.review_item(id)? else {
        return Ok(error(StatusCode::NOT_FOUND, "no such item"));
    };
    let pois: Vec<_> = ctx.store.pois(id)?.into_iter().map(|p| poi_view(&ctx, p)).collect();
    Ok(Json(json!({ "item": item, "pois": pois, "geometry": ctx.geometry })).into_response())
}

#[derive(Deserialize)]
struct ItemUpdate {
    status: Option<Status>,
    note: Option<String>,
}

async fn review_update(
    State(ctx): Ctx,
    Path(id): Path<i64>,
    Json(u): Json<ItemUpdate>,
) -> ApiResult {
    ctx.store.review_update(id, u.status, u.note.as_deref())?;
    Ok(Json(json!({"ok": true})).into_response())
}

async fn review_delete(State(ctx): Ctx, Path(id): Path<i64>) -> ApiResult {
    ctx.store.review_delete(id)?;
    Ok(Json(json!({"ok": true})).into_response())
}

async fn review_frame(State(ctx): Ctx, Path(id): Path<i64>) -> Response {
    file(&ctx, id, "jpg", "image/jpeg")
}

async fn review_canvas(State(ctx): Ctx, Path(id): Path<i64>) -> Response {
    file(&ctx, id, "png", "image/png")
}

fn file(ctx: &Context, id: i64, ext: &str, mime: &'static str) -> Response {
    match fs::read(ctx.store.review_dir().join(format!("{id}.{ext}"))) {
        Ok(bytes) => {
            ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "max-age=3600")], bytes)
                .into_response()
        }
        Err(_) => error(StatusCode::NOT_FOUND, "no such frame"),
    }
}

#[derive(Deserialize)]
struct NewPoi {
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
    comment: String,
}

/// Saves a POI, optimized on the item's frame (tech design §3.6).
async fn poi_add(State(ctx): Ctx, Path(id): Path<i64>, Json(p): Json<NewPoi>) -> ApiResult {
    let drawn = Rect::new(p.x1, p.y1, p.x2, p.y2);
    if drawn.is_empty() {
        return Ok(error(StatusCode::BAD_REQUEST, "draw a rectangle first"));
    }
    let path = ctx.store.review_dir().join(format!("{id}.jpg"));
    let geometry = ctx.geometry;
    let optimized = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<Rect>> {
        let img = image::open(&path)?.to_rgb8();
        if !geometry.fits(img.width(), img.height()) {
            return Ok(None);
        }
        Ok(optimize_poi(&NesCanvas::sample(&img, &geometry, None), &geometry, drawn))
    })
    .await??;
    let poi = ctx.store.poi_add(id, drawn, optimized, p.comment.trim())?;
    Ok(Json(poi_view(&ctx, poi)).into_response())
}

async fn poi_delete(State(ctx): Ctx, Path((id, poi)): Path<(i64, i64)>) -> ApiResult {
    ctx.store.poi_delete(id, poi)?;
    Ok(Json(json!({"ok": true})).into_response())
}

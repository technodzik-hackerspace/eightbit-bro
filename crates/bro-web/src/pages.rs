use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::Context;

pub fn router() -> Router<Arc<Context>> {
    Router::new()
        .route("/", get(|| async { Html(include_str!("../assets/live.html")) }))
        .route("/events", get(|| async { Html(include_str!("../assets/events.html")) }))
        .route("/review", get(|| async { Html(include_str!("../assets/review.html")) }))
        .route("/review/{id}", get(|| async { Html(include_str!("../assets/item.html")) }))
        .route("/settings", get(|| async { Html(include_str!("../assets/settings.html")) }))
}

pub async fn setup() -> Html<&'static str> {
    Html(include_str!("../assets/setup.html"))
}

pub async fn login() -> Html<&'static str> {
    Html(include_str!("../assets/login.html"))
}

pub async fn asset(Path(file): Path<String>) -> Response {
    let (mime, body) = match file.as_str() {
        "app.css" => ("text/css", include_str!("../assets/app.css")),
        "app.js" => ("text/javascript", include_str!("../assets/app.js")),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

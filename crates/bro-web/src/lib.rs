//! The web dashboard (PRD §12, tech design §7): static pages over a JSON API, with live
//! updates over Server-Sent Events. Everything the pages show comes from `/api/` (PRD W10).

mod api;
mod auth;
mod live;
mod pages;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use bro_store::Store;
use bro_vision::Geometry;
use tokio::net::TcpListener;
use tracing::info;

pub use auth::Secrets;
pub use live::{Live, Push, Stats};

pub struct Context {
    pub store: Arc<Store>,
    pub live: Arc<Live>,
    pub geometry: Geometry,
    pub data_dir: PathBuf,
    /// The declarative config, shown read-only on the settings page.
    pub config_text: String,
    /// Ask for a password (`[web] login`, default true).
    pub login: bool,
}

pub async fn serve(listen: &str, ctx: Context) -> Result<()> {
    let ctx = Arc::new(ctx);
    let auth = Arc::new(auth::Auth::load(&ctx.data_dir, ctx.login)?);
    live::spawn_rates(ctx.live.clone());

    let app = Router::new()
        .merge(pages::router())
        .nest("/api", api::router())
        .merge(auth::protected(auth.clone()))
        .layer(axum::middleware::from_fn_with_state(auth.clone(), auth::guard))
        .merge(auth::public(auth))
        .with_state(ctx);

    let listener = TcpListener::bind(listen).await?;
    info!(%listen, "dashboard listening");
    axum::serve(listener, app).await?;
    Ok(())
}

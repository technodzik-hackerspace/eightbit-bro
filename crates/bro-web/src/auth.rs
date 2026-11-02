//! First-boot setup, login and write-only secrets (PRD §12.2, W8). Secrets live in
//! `<data_dir>/secrets.json` (mode 0600), never in Nix or git; the UI only shows whether
//! each one is set.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{Context as _, Result};
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Context, pages};

const COOKIE: &str = "bro_session";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Secrets {
    pub password_hash: Option<String>,
    pub telegram_token: Option<String>,
    pub telegram_chat: Option<String>,
}

pub struct Auth {
    /// Whether the dashboard asks for a password at all (`[web] login`).
    login: bool,
    path: PathBuf,
    secrets: RwLock<Secrets>,
    sessions: Mutex<HashSet<String>>,
}

impl Auth {
    pub fn load(data_dir: &Path, login: bool) -> Result<Auth> {
        let path = data_dir.join("secrets.json");
        let secrets = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Secrets::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Auth { login, path, secrets: RwLock::new(secrets), sessions: Mutex::default() })
    }

    fn is_set_up(&self) -> bool {
        self.secrets.read().unwrap().password_hash.is_some()
    }

    fn update(&self, f: impl FnOnce(&mut Secrets)) -> Result<()> {
        let mut secrets = self.secrets.write().unwrap();
        let mut next = secrets.clone();
        f(&mut next);
        let tmp = self.path.with_extension("tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(serde_json::to_string_pretty(&next)?.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, &self.path)?;
        *secrets = next;
        Ok(())
    }

    fn new_session(&self) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.sessions.lock().unwrap().insert(token.clone());
        token
    }

    fn session(&self, headers: &HeaderMap) -> Option<String> {
        let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
        let token = cookies
            .split(';')
            .filter_map(|c| c.trim().split_once('='))
            .find(|(k, _)| *k == COOKIE)?
            .1;
        self.sessions.lock().unwrap().contains(token).then(|| token.to_owned())
    }
}

fn hash(password: &str) -> Result<String> {
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let salt = SaltString::encode_b64(&salt).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string())
}

fn verify(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// Everything except the public routes: setup first, then a valid session.
pub async fn guard(State(auth): State<Arc<Auth>>, req: Request, next: Next) -> Response {
    if !auth.login {
        return next.run(req).await;
    }
    let api = req.uri().path().starts_with("/api/");
    if !auth.is_set_up() {
        return match api {
            true => error(StatusCode::CONFLICT, "setup required"),
            false => Redirect::to("/setup").into_response(),
        };
    }
    if auth.session(req.headers()).is_none() {
        return match api {
            true => error(StatusCode::UNAUTHORIZED, "login required"),
            false => Redirect::to("/login").into_response(),
        };
    }
    next.run(req).await
}

/// Routes reachable without a session.
pub fn public(auth: Arc<Auth>) -> Router<Arc<Context>> {
    Router::new()
        .route("/setup", get(setup_page))
        .route("/login", get(login_page))
        .route("/api/auth", get(auth_mode))
        .route("/static/{file}", get(pages::asset))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .with_state(auth)
}

/// Secrets management, behind the guard.
pub fn protected(auth: Arc<Auth>) -> Router<Arc<Context>> {
    Router::new().route("/api/secrets", get(secrets_status).post(replace_secrets)).with_state(auth)
}

/// The setup and login pages, or the dashboard when there is no login.
async fn setup_page(State(auth): State<Arc<Auth>>) -> Response {
    match auth.login {
        true => pages::setup().await.into_response(),
        false => Redirect::to("/").into_response(),
    }
}

async fn login_page(State(auth): State<Arc<Auth>>) -> Response {
    match auth.login {
        true => pages::login().await.into_response(),
        false => Redirect::to("/").into_response(),
    }
}

/// Whether pages should offer login / logout.
async fn auth_mode(State(auth): State<Arc<Auth>>) -> Json<serde_json::Value> {
    Json(json!({ "login": auth.login }))
}

#[derive(Deserialize)]
struct SetupForm {
    password: String,
    telegram_token: Option<String>,
    telegram_chat: Option<String>,
}

async fn setup(State(auth): State<Arc<Auth>>, Json(form): Json<SetupForm>) -> Response {
    if auth.is_set_up() {
        return error(StatusCode::CONFLICT, "already set up");
    }
    if form.password.chars().count() < 8 {
        return error(StatusCode::BAD_REQUEST, "the password needs at least 8 characters");
    }
    let result = blocking(move || {
        let password_hash = hash(&form.password)?;
        auth.update(|s| {
            s.password_hash = Some(password_hash);
            s.telegram_token = non_empty(form.telegram_token);
            s.telegram_chat = non_empty(form.telegram_chat);
        })?;
        Ok(auth.new_session())
    })
    .await;
    match result {
        Ok(token) => with_session(&token),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login(State(auth): State<Arc<Auth>>, Json(form): Json<LoginForm>) -> Response {
    let check = auth.clone();
    let ok = blocking(move || {
        let secrets = check.secrets.read().unwrap();
        Ok(secrets.password_hash.as_deref().is_some_and(|h| verify(&form.password, h)))
    })
    .await
    .unwrap_or(false);
    if !ok {
        tokio::time::sleep(Duration::from_millis(500)).await;
        return error(StatusCode::UNAUTHORIZED, "wrong password");
    }
    with_session(&auth.new_session())
}

async fn logout(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Response {
    if let Some(token) = auth.session(&headers) {
        auth.sessions.lock().unwrap().remove(&token);
    }
    let cookie = format!("{COOKIE}=; Path=/; Max-Age=0");
    ([(header::SET_COOKIE, cookie)], Json(json!({"ok": true}))).into_response()
}

async fn secrets_status(State(auth): State<Arc<Auth>>) -> Json<serde_json::Value> {
    let s = auth.secrets.read().unwrap();
    Json(json!({
        "password": s.password_hash.is_some(),
        "telegram_token": s.telegram_token.is_some(),
        "telegram_chat": s.telegram_chat.is_some(),
    }))
}

#[derive(Deserialize)]
struct SecretsForm {
    password: Option<String>,
    telegram_token: Option<String>,
    telegram_chat: Option<String>,
}

async fn replace_secrets(State(auth): State<Arc<Auth>>, Json(form): Json<SecretsForm>) -> Response {
    if form.password.as_ref().is_some_and(|p| p.chars().count() < 8) {
        return error(StatusCode::BAD_REQUEST, "the password needs at least 8 characters");
    }
    let result = blocking(move || {
        let password_hash = form.password.as_deref().map(hash).transpose()?;
        auth.update(|s| {
            if password_hash.is_some() {
                s.password_hash = password_hash;
            }
            if let Some(t) = form.telegram_token {
                s.telegram_token = non_empty(Some(t));
            }
            if let Some(c) = form.telegram_chat {
                s.telegram_chat = non_empty(Some(c));
            }
        })
    })
    .await;
    match result {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn with_session(token: &str) -> Response {
    let cookie = format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000");
    ([(header::SET_COOKIE, cookie)], Json(json!({"ok": true}))).into_response()
}

pub(crate) fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_round_trip() {
        let h = hash("correct horse").unwrap();
        assert!(verify("correct horse", &h));
        assert!(!verify("wrong horse", &h));
    }
}

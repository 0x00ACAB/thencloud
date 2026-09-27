pub mod auth;
pub mod links;
pub mod nodes;
pub mod public;
pub mod shares;
pub mod uploads;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, header};
use axum::routing::{delete, get, patch, post, put};
use thencloud_crypto::MAX_ENCRYPTED_CHUNK;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::error::AppError;

/// Strict CSP: only same-origin scripts (plus WASM compilation), no styles,
/// no third parties. Anything that could read `location.hash` must come
/// from this server.
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; \
     img-src 'self' blob:; media-src 'self' blob:; style-src 'none'; object-src 'none'; base-uri 'none'; \
     frame-ancestors 'none'";

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/prelogin", post(auth::prelogin))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/password", post(auth::change_password))
        .route("/me", get(auth::me))
        .route("/nodes/folder", post(nodes::create_folder))
        .route(
            "/nodes/{id}",
            get(nodes::get).patch(nodes::update).delete(nodes::delete),
        )
        .route("/nodes/{id}/children", get(nodes::children))
        .route("/nodes/{id}/path", get(nodes::path))
        .route("/nodes/{id}/chunks/{idx}", get(nodes::chunk))
        .route("/uploads", post(uploads::create))
        .route("/uploads/{id}", delete(uploads::abort))
        .route(
            "/uploads/{id}/chunks/{idx}",
            put(uploads::put_chunk).layer(DefaultBodyLimit::max(MAX_ENCRYPTED_CHUNK + 1024)),
        )
        .route("/uploads/{id}/finish", post(uploads::finish))
        .route("/users/{username}/public-key", get(shares::public_key))
        .route("/shares", post(shares::create))
        .route("/shares/incoming", get(shares::incoming))
        .route("/shares/outgoing", get(shares::outgoing))
        .route("/shares/{id}", patch(shares::update).delete(shares::delete))
        .route("/links", post(links::create).get(links::list))
        .route("/links/{id}", delete(links::delete))
        .route("/public/{token}", get(public::info))
        .route("/public/{token}/unlock", post(public::unlock))
        .route("/public/{token}/nodes/{id}/children", get(public::children))
        .route(
            "/public/{token}/nodes/{id}/chunks/{idx}",
            get(public::chunk),
        )
        .fallback(|| async { AppError::NotFound });

    let web = state.config.web_dir.clone();
    Router::new()
        .nest("/api", api)
        .route_service("/s/{token}", ServeFile::new(web.join("share.html")))
        .fallback_service(ServeDir::new(web))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

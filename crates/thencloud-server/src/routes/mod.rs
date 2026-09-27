pub mod admin;
pub mod app_data;
pub mod app_passwords;
pub mod auth;
pub mod avatars;
pub mod contacts;
pub mod drafts;
pub mod drops;
pub mod links;
pub mod nodes;
pub mod passkeys;
pub mod public;
pub mod sessions;
pub mod shares;
pub mod tools;
pub mod trash;
pub mod two_factor;
pub mod uploads;
pub mod versions;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use thencloud_crypto::MAX_ENCRYPTED_CHUNK;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::error::AppError;

/// Strict CSP: only same-origin scripts (plus WASM compilation), same-origin
/// stylesheets (no inline styles), no third parties. Anything that could read
/// `location.hash` must come from this server.
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; \
     img-src 'self' blob:; media-src 'self' blob:; style-src 'self'; object-src 'none'; base-uri 'none'; \
     frame-ancestors 'none'";

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/prelogin", post(auth::prelogin))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/login/second-factor", post(two_factor::verify))
        .route("/auth/passkey/options", post(passkeys::login_options))
        .route("/auth/passkey/login", post(passkeys::login))
        .route("/auth/totp/setup", post(two_factor::totp_setup))
        .route(
            "/auth/totp",
            post(two_factor::totp_enable).delete(two_factor::totp_disable),
        )
        .route("/passkeys", get(passkeys::list).post(passkeys::register))
        .route("/passkeys/options", post(passkeys::creation_options))
        .route("/passkeys/{id}", delete(passkeys::delete))
        .route("/auth/options", get(auth::options))
        .route(
            "/auth/recovery",
            post(auth::set_recovery).delete(auth::remove_recovery),
        )
        .route("/auth/recovery/unlock", post(auth::recovery_unlock))
        .route("/auth/recovery/reset", post(auth::recovery_reset))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/app-login", post(app_passwords::login))
        .route(
            "/app-passwords",
            get(app_passwords::list).post(app_passwords::create),
        )
        .route("/app-passwords/{id}", delete(app_passwords::delete))
        .route("/auth/password", post(auth::change_password))
        .route("/me", get(auth::me))
        .route("/me/pq-key", put(auth::set_pq_key))
        .route("/me/contacts", get(contacts::get).put(contacts::put))
        .route(
            "/me/data/{name}",
            get(app_data::get)
                .put(app_data::put)
                .layer(DefaultBodyLimit::max(4 * 1024 * 1024)),
        )
        .route(
            "/me/avatar",
            get(avatars::get_mine)
                .put(avatars::set)
                .delete(avatars::remove),
        )
        .route("/users/{username}/avatar", get(avatars::get_user))
        .route("/avatar-grants/{username}", put(avatars::grant))
        .route(
            "/sessions",
            get(sessions::list).delete(sessions::revoke_others),
        )
        .route("/sessions/{id}", delete(sessions::revoke))
        .route("/admin/users", get(admin::users))
        .route(
            "/admin/users/{id}",
            patch(admin::update_user).delete(admin::delete_user),
        )
        .route(
            "/admin/settings",
            get(admin::get_settings).patch(admin::update_settings),
        )
        .route(
            "/admin/invites",
            get(admin::invites).post(admin::create_invite),
        )
        .route("/admin/invites/{id}", delete(admin::delete_invite))
        .route("/admin/stats", get(admin::stats))
        .route("/tools", get(tools::info))
        .route("/tools/video/info", post(tools::video_info))
        .route("/tools/video/download", post(tools::video_download))
        .route("/nodes/folder", post(nodes::create_folder))
        .route(
            "/nodes/{id}",
            get(nodes::get).patch(nodes::update).delete(nodes::delete),
        )
        .route("/nodes/{id}/children", get(nodes::children))
        .route("/nodes/{id}/path", get(nodes::path))
        .route("/nodes/{id}/name-tags", post(nodes::tag_names))
        .route("/nodes/{id}/chunks/{idx}", get(nodes::chunk))
        .route(
            "/nodes/{id}/draft",
            get(drafts::get)
                .put(drafts::put)
                .delete(drafts::delete)
                .layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route("/nodes/{id}/versions", get(versions::list))
        .route("/nodes/{id}/versions/{vid}", delete(versions::delete))
        .route(
            "/nodes/{id}/versions/{vid}/restore",
            post(versions::restore),
        )
        .route(
            "/nodes/{id}/versions/{vid}/chunks/{idx}",
            get(versions::chunk),
        )
        .route("/trash", get(trash::list).delete(trash::empty))
        .route("/trash/{id}", delete(trash::purge))
        .route("/trash/{id}/restore", post(trash::restore))
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
        .route("/drops", get(drops::list))
        .route("/drops/{id}", delete(drops::discard))
        .route("/drops/{id}/adopt", post(drops::adopt))
        .route("/links", post(links::create).get(links::list))
        .route("/links/{id}", delete(links::delete))
        .route("/public/{token}", get(public::info))
        .route("/public/{token}/unlock", post(public::unlock))
        .route("/public/{token}/nodes/{id}/children", get(public::children))
        .route(
            "/public/{token}/nodes/{id}/chunks/{idx}",
            get(public::chunk),
        )
        .route("/public/{token}/uploads", post(public::upload_create))
        .route("/public/{token}/uploads/{id}", delete(public::upload_abort))
        .route(
            "/public/{token}/uploads/{id}/chunks/{idx}",
            put(public::upload_chunk).layer(DefaultBodyLimit::max(MAX_ENCRYPTED_CHUNK + 1024)),
        )
        .route(
            "/public/{token}/uploads/{id}/finish",
            post(public::upload_finish),
        )
        .fallback(|| async { AppError::NotFound });

    let web = state.config.web_dir.clone();
    Router::new()
        .nest("/api", api)
        // The web build writes .br and .gz copies of static files; send one
        // of those when the browser accepts it.
        .route_service(
            "/s/{token}",
            ServeFile::new(web.join("share.html"))
                .precompressed_br()
                .precompressed_gzip(),
        )
        .fallback_service(ServeDir::new(web).precompressed_br().precompressed_gzip())
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
        .layer(middleware::from_fn(cache_control))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Caching, by path, unless a route set its own:
/// - `/assets/*` files have content hashes in their names, so they can be
///   cached for good (only when they were found);
/// - API responses carry wrapped keys and ciphertext: never stored;
/// - HTML pages are revalidated every time, so after an upgrade nobody keeps
///   an old page that points at assets that no longer exist.
async fn cache_control(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let mut res = next.run(req).await;
    let value = if path.starts_with("/api/") {
        "no-store"
    } else if path.starts_with("/assets/") && res.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static(value));
    // Static files may be sent brotli- or gzip-compressed.
    if !path.starts_with("/api/") {
        res.headers_mut()
            .entry(header::VARY)
            .or_insert(HeaderValue::from_static("accept-encoding"));
    }
    res
}

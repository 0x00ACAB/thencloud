pub mod activity;
pub mod admin;
pub mod app_data;
pub mod app_passwords;
pub mod auth;
pub mod avatars;
pub mod comments;
pub mod contacts;
pub mod drafts;
pub mod drops;
pub mod health;
pub mod links;
pub mod nodes;
pub mod passkeys;
pub mod public;
pub mod sessions;
pub mod shares;
pub mod storage;
pub mod thumbnails;
pub mod tools;
pub mod trash;
pub mod two_factor;
pub mod uploads;
pub mod versions;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use thencloud_crypto::MAX_ENCRYPTED_CHUNK;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::RequestBodyTimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::error::AppError;

/// Strict CSP: only same-origin scripts (plus WASM compilation), same-origin
/// stylesheets (no inline styles), no third parties. Anything that could read
/// `location.hash` must come from this server.
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; \
     img-src 'self' blob:; media-src 'self' blob:; style-src 'self'; object-src 'none'; base-uri 'none'; \
     form-action 'self'; frame-ancestors 'none'";

/// The CSP of `/auth`, the page that runs the Turnstile check, when it's on:
/// Cloudflare's script and frame, and nothing else from outside. That page
/// has no password field and holds no key; it only hands the token back.
/// Nothing on a thencloud server is for search engines or AI crawlers: the
/// pages are an app, and shared files are behind links meant for one person.
/// `Disallow` keeps well-behaved crawlers out; the `X-Robots-Tag` header on
/// every response (see `router`) also covers the ones that fetch anyway.
const ROBOTS_TXT: &str = "# A private, end-to-end encrypted file store. Please don't crawl, index or\n\
# train on anything here.\n\
User-agent: *\n\
Disallow: /\n";
const X_ROBOTS_TAG: &str = "noindex, nofollow, noarchive, noai, noimageai";

/// With `--hsts`: HTTPS only, for two years, subdomains included.
const HSTS: &str = "max-age=63072000; includeSubDomains";

const AUTH_CSP: &str = "default-src 'self'; script-src 'self' https://challenges.cloudflare.com; \
     frame-src https://challenges.cloudflare.com; connect-src 'self'; img-src 'self'; style-src 'self'; \
     object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Browser features the app never uses, turned off for it and anything it
/// shows. Fullscreen (the video player) and passkeys keep their defaults.
const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), usb=(), serial=(), hid=(), \
     bluetooth=(), payment=(), browsing-topics=()";

/// How long a request body may take to arrive. A 4 MiB chunk takes about
/// two minutes at 256 kbit/s; a client that stalls longer gives up its
/// connection and buffer.
const BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Origins of the desktop and Android apps (`crates/thencloud-app`), which
/// bundle the web client instead of loading it from here: `tauri://localhost`
/// on Linux, `http(s)://tauri.localhost` on Windows and Android. Sessions are
/// bearer tokens, never cookies, so letting them in exposes nothing a
/// request from outside a browser couldn't already do.
const APP_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

fn app_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(APP_ORIGINS.map(HeaderValue::from_static))
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_secs(3600))
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health::health))
        .route("/metrics", get(health::metrics))
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
        .route("/me/delete", post(auth::delete_me))
        .route("/me/transfer", get(auth::transfer))
        .route("/me/pq-key", put(auth::set_pq_key))
        .route("/me/contacts", get(contacts::get).put(contacts::put))
        .route(
            "/me/data/{name}",
            get(app_data::get)
                .put(app_data::put)
                // Base64 of the largest blob (the search index), and the JSON around it.
                .layer(DefaultBodyLimit::max(
                    app_data::MAX_SEARCH_BYTES / 3 * 4 + 64 * 1024,
                )),
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
        .route("/admin/audit", get(admin::audit_log))
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
            "/nodes/{id}/comments",
            get(comments::list).post(comments::create),
        )
        .route("/comments/{id}", delete(comments::delete))
        .route("/nodes/{id}/thumbnail", get(thumbnails::get))
        .route("/nodes/{id}/activity", get(activity::list))
        .route("/nodes/{id}/changes", get(activity::live))
        .route("/changes", get(activity::changes))
        .route(
            "/nodes/{id}/versions/{vid}/thumbnail",
            put(thumbnails::put).layer(DefaultBodyLimit::max(
                thumbnails::MAX_THUMBNAIL_BYTES + 1024,
            )),
        )
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
        .route("/shares/{id}/key", put(shares::reseal))
        .route("/storage", get(storage::info))
        .route("/storage/prefer", put(storage::set_prefer))
        .route("/storage/google", post(storage::google_start))
        .route("/storage/google/callback", get(storage::google_callback))
        .route(
            "/storage/accounts/{id}",
            patch(storage::update_account).delete(storage::unlink),
        )
        .route("/storage/accounts/{id}/move", post(storage::move_files))
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
        .route(
            "/public/{token}/nodes/{id}/thumbnail",
            get(public::thumbnail),
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
        .fallback(|| async { AppError::NotFound })
        .layer(app_cors());

    let web = state.config.web_dir.clone();
    let hsts = state.config.hsts.then(|| HeaderValue::from_static(HSTS));
    let auth_csp = if crate::turnstile::enabled(&state) {
        AUTH_CSP
    } else {
        CSP
    };
    Router::new()
        .nest("/api", api)
        .merge(
            Router::new()
                .route_service(
                    "/auth",
                    ServeFile::new(web.join("auth.html"))
                        .precompressed_br()
                        .precompressed_gzip(),
                )
                .layer(SetResponseHeaderLayer::overriding(
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static(auth_csp),
                )),
        )
        // The web build writes .br and .gz copies of static files; send one
        // of those when the browser accepts it.
        .route_service(
            "/s/{token}",
            ServeFile::new(web.join("share.html"))
                .precompressed_br()
                .precompressed_gzip(),
        )
        .route(
            "/robots.txt",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    ROBOTS_TXT,
                )
            }),
        )
        .fallback_service(ServeDir::new(web).precompressed_br().precompressed_gzip())
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static(X_ROBOTS_TAG),
        ))
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
        .layer(SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            move |_: &Response| hsts.clone(),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(PERMISSIONS_POLICY),
        ))
        .layer(middleware::from_fn(cache_control))
        .layer(RequestBodyTimeoutLayer::new(BODY_TIMEOUT))
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

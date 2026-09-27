//! Tools that need the server. For now only the video downloader, which is
//! off unless an admin turns it on (see downloader.rs for what it does and
//! doesn't keep). Converting files happens entirely in the browser.

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::Response;
use thencloud_crypto::api::{DownloaderAccess, ToolsInfo, VideoInfo, VideoKind, VideoLinkRequest};

use crate::AppState;
use crate::auth::AuthUser;
use crate::downloader::check_link;
use crate::error::{AppError, Result};
use crate::settings;

async fn may_download(state: &AppState, user: &AuthUser) -> Result<bool> {
    Ok(state.downloader.version.is_some()
        && match settings::downloader(state).await? {
            DownloaderAccess::Off => false,
            DownloaderAccess::Admins => user.is_admin,
            DownloaderAccess::Everyone => true,
        })
}

async fn require_downloader(state: &AppState, user: &AuthUser) -> Result<()> {
    if state.downloader.version.is_none() {
        return Err(AppError::Unavailable(
            "yt-dlp isn't installed on this server".into(),
        ));
    }
    if !may_download(state, user).await? {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn info(State(state): State<AppState>, user: AuthUser) -> Result<Json<ToolsInfo>> {
    Ok(Json(ToolsInfo {
        video_downloader: may_download(&state, &user).await?,
        downloader_max_bytes: state.config.downloader_max_bytes,
        downloader_can_merge: state.downloader.can_merge,
    }))
}

/// Look a link up: title, length, and what would be downloaded.
pub async fn video_info(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<VideoLinkRequest>,
) -> Result<Json<VideoInfo>> {
    require_downloader(&state, &user).await?;
    let url = req.url.trim();
    check_link(url, state.config.downloader_public_only).await?;
    // Counts against the same one-at-a-time slot as downloads.
    let _slot = state.downloader.slot(&user.id)?;
    Ok(Json(state.downloader.info(url).await?))
}

/// Stream the video (or its audio) to the browser as it downloads.
pub async fn video_download(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<VideoLinkRequest>,
) -> Result<Response> {
    require_downloader(&state, &user).await?;
    let url = req.url.trim();
    check_link(url, state.config.downloader_public_only).await?;
    let slot = state.downloader.slot(&user.id)?;
    let kind = req.kind.unwrap_or(VideoKind::Video);
    let stream = state
        .downloader
        .stream(url, kind, state.config.downloader_max_bytes, slot)
        .await?;
    let mut res = Response::new(Body::from_stream(stream));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(res)
}

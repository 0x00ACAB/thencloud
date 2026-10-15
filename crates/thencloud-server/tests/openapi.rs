//! The OpenAPI description of every `/api` route, checked in at
//! `docs/src/reference/openapi.json`. The schemas come from the wire types in
//! `thencloud_crypto::api` (their `openapi` feature); the routes are the table
//! below, which `every_route_is_described` keeps in step with the router in
//! `src/routes/mod.rs`. After changing either, rewrite the file with
//!
//!     UPDATE_OPENAPI=1 cargo test -p thencloud-server --test openapi

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value, json};
use thencloud_crypto::KdfParams;
use thencloud_crypto::api::*;
use utoipa::ToSchema;
use utoipa::openapi::RefOr;

const FILE: &str = "../../docs/src/reference/openapi.json";

#[derive(Clone, Copy)]
enum Auth {
    /// No sign-in.
    Open,
    /// A session token or app password session.
    User,
    /// A public link, with `X-Link-Token` once `info` has handed one out.
    Visitor,
    /// The `--metrics-token`.
    Metrics,
}

enum Body {
    None,
    Json(Value),
    /// A JSON body that may be left out.
    OptionalJson(Value),
    Bytes(&'static str),
}

enum Out {
    /// 204 No Content.
    Empty,
    /// 200 with JSON.
    Ok(Value),
    /// 201 with JSON.
    Created(Value),
    /// 200 with an encrypted chunk or thumbnail, and its version in `X-Version-Id`.
    Ciphertext(&'static str),
    /// 200 with another body.
    Raw(&'static str, &'static str),
}

struct Spec {
    schemas: BTreeMap<String, RefOr<utoipa::openapi::schema::Schema>>,
    paths: BTreeMap<String, Map<String, Value>>,
}

struct Op<'a> {
    spec: &'a mut Spec,
    method: &'static str,
    path: &'static str,
    tag: &'static str,
    summary: &'static str,
    auth: Auth,
    query: Vec<(&'static str, &'static str, &'static str)>,
    body: Body,
}

impl Spec {
    fn new() -> Self {
        Spec {
            schemas: BTreeMap::new(),
            paths: BTreeMap::new(),
        }
    }

    /// A reference to `T`'s schema, adding it and what it uses to the components.
    fn r<T: ToSchema>(&mut self) -> Value {
        let mut found = vec![(T::name().to_string(), T::schema())];
        T::schemas(&mut found);
        self.schemas.extend(found);
        json!({ "$ref": format!("#/components/schemas/{}", T::name()) })
    }

    fn list<T: ToSchema>(&mut self) -> Value {
        json!({ "type": "array", "items": self.r::<T>() })
    }

    fn or_null<T: ToSchema>(&mut self) -> Value {
        json!({ "oneOf": [self.r::<T>(), { "type": "null" }] })
    }

    fn op(
        &mut self,
        method: &'static str,
        path: &'static str,
        tag: &'static str,
        summary: &'static str,
        auth: Auth,
    ) -> Op<'_> {
        Op {
            spec: self,
            method,
            path,
            tag,
            summary,
            auth,
            query: Vec::new(),
            body: Body::None,
        }
    }

    fn document(self) -> Value {
        let schemas: Map<String, Value> = self
            .schemas
            .into_iter()
            .map(|(k, v)| (k, serde_json::to_value(v).unwrap()))
            .collect();
        let mut components = json!({
            "schemas": schemas,
            "securitySchemes": {
                "session": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "The token from signing in (`SessionResponse.token`), or from `/auth/app-login` with an app password. A read-only app password may only GET.",
                },
                "linkToken": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Link-Token",
                    "description": "Sent on a public link's requests after `GET /public/{token}` (a link with limited opens) or `/unlock` (a password) handed one out. The link's key and secret never leave the browser; this is a signed pass for this visit.",
                },
                "metricsToken": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "The server's `--metrics-token`.",
                },
            },
        });
        components["responses"] = json!({
            "Error": {
                "description": "Something went wrong. `error` is a stable code (`unauthorized`, `not_found`, `conflict`, `name_taken`, `quota_exceeded`, `rate_limited`, `transfer_limit`...), `message` is for people.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorBody" } } },
            },
        });
        json!({
            "openapi": "3.1.0",
            "info": {
                "title": "thencloud",
                "version": "1",
                "description": "The HTTP API of a thencloud server, under `/api`. Everything here carries ciphertext, wrapped keys and public keys: names, contents and keys are encrypted and decrypted by the client (`thencloud-crypto`, natively or as WASM), and the formats are in `docs/format/README.md`. Binary fields are unpadded base64url strings. Fields may be added to any object over time; clients should ignore what they don't know.",
                "license": { "name": "AGPL-3.0-or-later", "identifier": "AGPL-3.0-or-later" },
            },
            "servers": [{ "url": "/api" }],
            "paths": self.paths,
            "components": components,
        })
    }
}

impl Op<'_> {
    fn query(mut self, name: &'static str, ty: &'static str, about: &'static str) -> Self {
        self.query.push((name, ty, about));
        self
    }

    fn body(mut self, body: Value) -> Self {
        self.body = Body::Json(body);
        self
    }

    fn optional_body(mut self, body: Value) -> Self {
        self.body = Body::OptionalJson(body);
        self
    }

    fn bytes(mut self, about: &'static str) -> Self {
        self.body = Body::Bytes(about);
        self
    }

    fn out(self, out: Out) {
        let mut params = Vec::new();
        for seg in self.path.split('/') {
            if let Some(name) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                let ty = if name == "idx" { "integer" } else { "string" };
                params.push(json!({
                    "name": name, "in": "path", "required": true,
                    "schema": { "type": ty },
                }));
            }
        }
        for (name, ty, about) in &self.query {
            params.push(json!({
                "name": name, "in": "query", "required": false,
                "description": about, "schema": { "type": ty },
            }));
        }
        let mut op = json!({
            "tags": [self.tag],
            "summary": self.summary,
            "operationId": operation_id(self.method, self.path),
        });
        if !params.is_empty() {
            op["parameters"] = Value::Array(params);
        }
        let json_body = |schema: &Value| json!({ "application/json": { "schema": schema } });
        match &self.body {
            Body::None => {}
            Body::Json(s) => {
                op["requestBody"] = json!({ "required": true, "content": json_body(s) })
            }
            Body::OptionalJson(s) => {
                op["requestBody"] = json!({ "required": false, "content": json_body(s) })
            }
            Body::Bytes(about) => {
                op["requestBody"] = json!({
                    "required": true, "description": about,
                    "content": { "application/octet-stream": { "schema": { "type": "string", "contentMediaType": "application/octet-stream" } } },
                })
            }
        }
        let (code, response) = match out {
            Out::Empty => ("204", json!({ "description": "Done." })),
            Out::Ok(s) => (
                "200",
                json!({ "description": "OK.", "content": json_body(&s) }),
            ),
            Out::Created(s) => (
                "201",
                json!({ "description": "Created.", "content": json_body(&s) }),
            ),
            Out::Ciphertext(about) => (
                "200",
                json!({
                    "description": about,
                    "headers": { "X-Version-Id": { "description": "The version these bytes belong to.", "schema": { "type": "string" } } },
                    "content": { "application/octet-stream": { "schema": { "type": "string", "contentMediaType": "application/octet-stream" } } },
                }),
            ),
            Out::Raw(ty, about) => (
                "200",
                json!({ "description": about, "content": { ty: { "schema": { "type": "string" } } } }),
            ),
        };
        let mut responses = Map::new();
        responses.insert(code.into(), response);
        responses.insert(
            "default".into(),
            json!({ "$ref": "#/components/responses/Error" }),
        );
        op["responses"] = Value::Object(responses);
        match self.auth {
            Auth::Open => op["security"] = json!([]),
            Auth::User => op["security"] = json!([{ "session": [] }]),
            Auth::Visitor => op["security"] = json!([{}, { "linkToken": [] }]),
            Auth::Metrics => op["security"] = json!([{ "metricsToken": [] }]),
        }
        let item = self.spec.paths.entry(self.path.to_string()).or_default();
        assert!(
            item.insert(self.method.to_string(), op).is_none(),
            "{} {} is described twice",
            self.method,
            self.path
        );
    }
}

fn operation_id(method: &str, path: &str) -> String {
    let mut id = method.to_string();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let seg = seg.trim_start_matches('{').trim_end_matches('}');
        for word in seg.split(['-', '_']) {
            let mut c = word.chars();
            if let Some(first) = c.next() {
                id.extend(first.to_uppercase());
                id.push_str(c.as_str());
            }
        }
    }
    id
}

fn spec() -> Value {
    use Auth::*;
    let mut s = Spec::new();
    s.r::<ErrorBody>();

    // Server
    s.op(
        "get",
        "/health",
        "Server",
        "200 when the database answers and the data directory is there, 503 otherwise.",
        Open,
    )
    .out(Out::Raw("text/plain", "`ok`."));
    s.op("get", "/metrics", "Server", "Prometheus metrics: the admin view's counts and the data disk's space. Off unless the server has a `--metrics-token`.", Metrics)
        .out(Out::Raw("text/plain", "Prometheus text format."));

    // Signing in
    let (b, o) = (s.r::<PreloginRequest>(), s.r::<PreloginResponse>());
    s.op("post", "/auth/prelogin", "Signing in", "The KDF salt and parameters to derive a user's keys with. Unknown users get a stable fake salt.", Open)
        .body(b).out(Out::Ok(o));
    s.r::<KdfParams>();
    let (b, o) = (s.r::<RegisterRequest>(), s.r::<SessionResponse>());
    s.op(
        "post",
        "/auth/register",
        "Signing in",
        "Make an account. The first one on a new server needs the setup code.",
        Open,
    )
    .body(b)
    .out(Out::Created(o));
    let (b, o) = (s.r::<LoginRequest>(), s.r::<LoginResponse>());
    s.op("post", "/auth/login", "Signing in", "Sign in with the auth key derived from the password: a session, or a ticket for the second step.", Open)
        .body(b).out(Out::Ok(o));
    let (b, o) = (s.r::<SecondFactorRequest>(), s.r::<SessionResponse>());
    s.op(
        "post",
        "/auth/login/second-factor",
        "Signing in",
        "Finish a password sign-in with a TOTP code or a passkey.",
        Open,
    )
    .body(b)
    .out(Out::Ok(o));
    let o = s.r::<PasskeyRequest>();
    s.op(
        "post",
        "/auth/passkey/options",
        "Signing in",
        "A challenge for signing in with a passkey alone.",
        Open,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<PasskeyLoginRequest>(), s.r::<PasskeyLoginResponse>());
    s.op("post", "/auth/passkey/login", "Signing in", "Sign in with a passkey that has PRF: a session and the master key wrapped under the PRF output.", Open)
        .body(b).out(Out::Ok(o));
    let o = s.r::<AuthOptions>();
    s.op(
        "get",
        "/auth/options",
        "Signing in",
        "What the sign-in screen can offer (registration, setup, passkeys, bot check).",
        Open,
    )
    .out(Out::Ok(o));
    let (b, o) = (
        s.r::<RecoveryUnlockRequest>(),
        s.r::<RecoveryUnlockResponse>(),
    );
    s.op(
        "post",
        "/auth/recovery/unlock",
        "Signing in",
        "Fetch the master key wrapped under the recovery key.",
        Open,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<RecoveryResetRequest>(), s.r::<SessionResponse>());
    s.op(
        "post",
        "/auth/recovery/reset",
        "Signing in",
        "Set a new password with the recovery key. Every other session is signed out.",
        Open,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<AppLoginRequest>(), s.r::<AppLoginResponse>());
    s.op(
        "post",
        "/auth/app-login",
        "Signing in",
        "Sign in with an app password alone.",
        Open,
    )
    .body(b)
    .out(Out::Ok(o));
    s.op(
        "post",
        "/auth/logout",
        "Signing in",
        "End this session.",
        User,
    )
    .out(Out::Empty);

    // Account
    let o = s.r::<Me>();
    s.op(
        "get",
        "/me",
        "Account",
        "The signed-in account: keys (wrapped), quota and settings.",
        User,
    )
    .out(Out::Ok(o));
    let b = s.r::<DeleteAccountRequest>();
    s.op(
        "post",
        "/me/delete",
        "Account",
        "Delete your own account and everything in it. Needs the password again.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    let o = s.r::<TransferInfo>();
    s.op(
        "get",
        "/me/transfer",
        "Account",
        "Your daily transfer limits and what you've used today.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<SetPqKeyRequest>(), s.r::<Me>());
    s.op(
        "put",
        "/me/pq-key",
        "Account",
        "Add the ML-KEM key to an account made before them. Once only.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let b = s.r::<ChangePasswordRequest>();
    s.op(
        "post",
        "/auth/password",
        "Account",
        "Re-wrap the master key under a new password. Other sessions are signed out.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    let (b, o) = (s.r::<SetRecoveryRequest>(), s.r::<Me>());
    s.op(
        "post",
        "/auth/recovery",
        "Account",
        "Set the recovery key (the master key wrapped under it).",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<RemoveRecoveryRequest>(), s.r::<Me>());
    s.op(
        "delete",
        "/auth/recovery",
        "Account",
        "Remove the recovery key.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<CurrentPassword>(), s.r::<TotpSetup>());
    s.op(
        "post",
        "/auth/totp/setup",
        "Account",
        "Start setting up an authenticator app: a new secret, kept aside until a code confirms it.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<EnableTotpRequest>(), s.r::<Me>());
    s.op(
        "post",
        "/auth/totp",
        "Account",
        "Turn the authenticator app on with a code from it.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<CurrentPassword>(), s.r::<Me>());
    s.op(
        "delete",
        "/auth/totp",
        "Account",
        "Turn the authenticator app off.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let o = s.list::<Passkey>();
    s.op("get", "/passkeys", "Account", "Your passkeys.", User)
        .out(Out::Ok(o));
    let o = s.r::<PasskeyCreationOptions>();
    s.op(
        "post",
        "/passkeys/options",
        "Account",
        "Step one of adding a passkey: a challenge and the credentials to exclude.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<RegisterPasskeyRequest>(), s.r::<Passkey>());
    s.op(
        "post",
        "/passkeys",
        "Account",
        "Step two: the authenticator's response. Needs the account password.",
        User,
    )
    .body(b)
    .out(Out::Created(o));
    let b = s.r::<CurrentPassword>();
    s.op(
        "delete",
        "/passkeys/{id}",
        "Account",
        "Remove a passkey.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    let o = s.list::<AppPassword>();
    s.op(
        "get",
        "/app-passwords",
        "Account",
        "Your app passwords.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<CreateAppPasswordRequest>(), s.r::<AppPassword>());
    s.op(
        "post",
        "/app-passwords",
        "Account",
        "Make an app password (needs the account password).",
        User,
    )
    .body(b)
    .out(Out::Created(o));
    s.op(
        "delete",
        "/app-passwords/{id}",
        "Account",
        "Revoke an app password and sign out its sessions.",
        User,
    )
    .out(Out::Empty);
    let o = s.list::<DeviceSession>();
    s.op(
        "get",
        "/sessions",
        "Account",
        "Signed-in sessions: device name, last seen and how each signed in. No addresses are kept.",
        User,
    )
    .out(Out::Ok(o));
    s.op(
        "delete",
        "/sessions",
        "Account",
        "Sign out every session except this one.",
        User,
    )
    .out(Out::Empty);
    s.op(
        "delete",
        "/sessions/{id}",
        "Account",
        "Sign out one session (this one too, like logging out).",
        User,
    )
    .out(Out::Empty);
    let o = s.r::<PrivateData>();
    s.op(
        "get",
        "/me/contacts",
        "Account",
        "Verified contacts, encrypted under the master key.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<PutPrivateData>(), s.r::<PrivateData>());
    s.op(
        "put",
        "/me/contacts",
        "Account",
        "Save the contacts. With `if_revision`, a concurrent change is a 409 `conflict`.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let o = s.r::<PrivateData>();
    s.op("get", "/me/data/{name}", "Account", "One app's data (`music`, `videos`, `files`, `notes`, `books`, `search`, `health` or `prefs`), encrypted under the master key.", User)
        .out(Out::Ok(o));
    let (b, o) = (s.r::<PutPrivateData>(), s.r::<PrivateData>());
    s.op(
        "put",
        "/me/data/{name}",
        "Account",
        "Save one app's data. With `if_revision`, a concurrent change is a 409 `conflict`.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let o = s.r::<MyAvatar>();
    s.op(
        "get",
        "/me/avatar",
        "Account",
        "Your encrypted picture and display name.",
        User,
    )
    .out(Out::Ok(o));
    let b = s.r::<SetAvatar>();
    s.op(
        "put",
        "/me/avatar",
        "Account",
        "Set your picture and display name, under a new avatar key.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    s.op(
        "delete",
        "/me/avatar",
        "Account",
        "Remove the picture and display name, and every copy of their key handed out.",
        User,
    )
    .out(Out::Empty);
    let o = s.or_null::<UserAvatar>();
    s.op(
        "get",
        "/users/{username}/avatar",
        "Account",
        "Someone's picture and display name, if they gave you their avatar key.",
        User,
    )
    .out(Out::Ok(o));
    let b = s.r::<AvatarGrant>();
    s.op(
        "put",
        "/avatar-grants/{username}",
        "Account",
        "Give someone your avatar key. Only between people who share something.",
        User,
    )
    .body(b)
    .out(Out::Empty);

    // Admin
    let o = s.list::<AdminUser>();
    s.op(
        "get",
        "/admin/users",
        "Admin",
        "Every account, with usage and limits. Never content.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<UpdateUserRequest>(), s.r::<AdminUser>());
    s.op(
        "patch",
        "/admin/users/{id}",
        "Admin",
        "Change an account's quota, limits, admin flag, or disable it.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    s.op(
        "delete",
        "/admin/users/{id}",
        "Admin",
        "Delete an account and everything in its tree.",
        User,
    )
    .out(Out::Empty);
    let o = s.r::<AdminSettings>();
    s.op("get", "/admin/settings", "Admin", "Runtime settings.", User)
        .out(Out::Ok(o));
    let (b, o) = (s.r::<UpdateSettingsRequest>(), s.r::<AdminSettings>());
    s.op(
        "patch",
        "/admin/settings",
        "Admin",
        "Change runtime settings (registration, the downloader, linked storage).",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let o = s.list::<Invite>();
    s.op("get", "/admin/invites", "Admin", "Open invites.", User)
        .out(Out::Ok(o));
    let (b, o) = (s.r::<CreateInviteRequest>(), s.r::<CreatedInvite>());
    s.op("post", "/admin/invites", "Admin", "Make an invite.", User)
        .body(b)
        .out(Out::Created(o));
    s.op(
        "delete",
        "/admin/invites/{id}",
        "Admin",
        "Delete an invite.",
        User,
    )
    .out(Out::Empty);
    let o = s.r::<ServerStats>();
    s.op(
        "get",
        "/admin/stats",
        "Admin",
        "Counts: accounts, files, bytes.",
        User,
    )
    .out(Out::Ok(o));
    let o = s.r::<AuditPage>();
    s.op(
        "get",
        "/admin/audit",
        "Admin",
        "The audit log of admin actions, newest first, a page at a time.",
        User,
    )
    .query(
        "before",
        "integer",
        "Entries older than this id (the next page).",
    )
    .out(Out::Ok(o));
    let o = s.r::<AdminAlerts>();
    s.op(
        "get",
        "/admin/alerts",
        "Admin",
        "Disks or a bucket nearly full, and accounts near their quota.",
        User,
    )
    .out(Out::Ok(o));

    // Tools
    let o = s.r::<ToolsInfo>();
    s.op(
        "get",
        "/tools",
        "Tools",
        "Which server-side tools are on.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<VideoLinkRequest>(), s.r::<VideoInfo>());
    s.op("post", "/tools/video/info", "Tools", "The video downloader (off by default; the server sees the link and the video): look a link up.", User)
        .body(b).out(Out::Ok(o));
    let b = s.r::<VideoLinkRequest>();
    s.op(
        "post",
        "/tools/video/download",
        "Tools",
        "Stream the video (or its audio) as it downloads, for the client to encrypt and upload.",
        User,
    )
    .body(b)
    .out(Out::Raw("application/octet-stream", "The media, streamed."));

    // Files
    let (b, o) = (s.r::<CreateFolderRequest>(), s.r::<Node>());
    s.op("post", "/nodes/folder", "Files", "Make a folder. Its name and key arrive encrypted; a duplicate name tag is a 409 `name_taken`.", User)
        .body(b).out(Out::Created(o));
    let o = s.r::<Node>();
    s.op("get", "/nodes/{id}", "Files", "A file or folder.", User)
        .out(Out::Ok(o));
    let (b, o) = (s.r::<UpdateNodeRequest>(), s.r::<Node>());
    s.op(
        "patch",
        "/nodes/{id}",
        "Files",
        "Rename or move: new encrypted metadata, a re-wrapped key, a new name tag.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    s.op("delete", "/nodes/{id}", "Files", "Move to the trash.", User)
        .out(Out::Empty);
    let (list, page) = (s.list::<Node>(), s.r::<NodePage>());
    s.op(
        "get",
        "/nodes/{id}/children",
        "Files",
        "A folder's children: all of them, or with `limit` a page continued by `after`.",
        User,
    )
    .query(
        "limit",
        "integer",
        "Page size; without it the whole array comes back.",
    )
    .query(
        "after",
        "string",
        "The `after` cursor of the previous page.",
    )
    .out(Out::Ok(json!({ "oneOf": [list, page] })));
    let o = s.r::<NodePath>();
    s.op("get", "/nodes/{id}/path", "Files", "The chain of nodes needed to derive this node's key: from your root, or from the top-most node shared with you.", User)
        .out(Out::Ok(o));
    let b = s.r::<NameTags>();
    s.op(
        "post",
        "/nodes/{id}/name-tags",
        "Files",
        "Tag children made before name tags existed. A tag that clashes is skipped.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    s.op(
        "get",
        "/nodes/{id}/chunks/{idx}",
        "Files",
        "One encrypted chunk of the current version.",
        User,
    )
    .out(Out::Ciphertext("The chunk."));
    let node = s.r::<Node>();
    s.op("get", "/nodes/{id}/cache-manifest", "Files", "SHA-256 of each ciphertext chunk of the current version, for clients that cache chunks.", User)
        .out(Out::Ok(json!({
            "type": "object",
            "required": ["node", "chunk_hashes"],
            "properties": {
                "node": node,
                "chunk_hashes": { "type": "array", "items": { "type": "string" }, "description": "Hex SHA-256 of each chunk, in order." },
            },
        })));
    let b = s.r::<SetTrashAtRequest>();
    s.op("put", "/nodes/{id}/trash-at", "Files", "Set or clear the date a node goes to the trash on its own (owner only, rounded up to the hour).", User)
        .body(b).out(Out::Empty);
    s.op(
        "get",
        "/nodes/{id}/thumbnail",
        "Files",
        "The current version's encrypted thumbnail.",
        User,
    )
    .out(Out::Ciphertext("The encrypted thumbnail."));
    s.op(
        "put",
        "/nodes/{id}/versions/{vid}/thumbnail",
        "Files",
        "Store a version's encrypted thumbnail (64 KiB at most).",
        User,
    )
    .bytes("The encrypted thumbnail.")
    .out(Out::Empty);
    let o = s.or_null::<Draft>();
    s.op(
        "get",
        "/nodes/{id}/draft",
        "Files",
        "Your unsaved draft of a file, encrypted under the master key.",
        User,
    )
    .out(Out::Ok(o));
    let b = s.r::<Draft>();
    s.op("put", "/nodes/{id}/draft", "Files", "Keep a draft.", User)
        .body(b)
        .out(Out::Empty);
    s.op(
        "delete",
        "/nodes/{id}/draft",
        "Files",
        "Drop the draft.",
        User,
    )
    .out(Out::Empty);
    let o = s.list::<Comment>();
    s.op(
        "get",
        "/nodes/{id}/comments",
        "Files",
        "Comments, encrypted under the node key.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<CreateCommentRequest>(), s.r::<Comment>());
    s.op(
        "post",
        "/nodes/{id}/comments",
        "Files",
        "Add a comment.",
        User,
    )
    .body(b)
    .out(Out::Created(o));
    s.op(
        "delete",
        "/comments/{id}",
        "Files",
        "Delete a comment (its author or the node's owner).",
        User,
    )
    .out(Out::Empty);
    let o = s.list::<ActivityEvent>();
    s.op("get", "/nodes/{id}/activity", "Files", "Who added, changed, renamed, moved, trashed or restored what under a folder, by node id, to the hour. Newest first.", User)
        .query("before", "integer", "Events older than this id (the next page).")
        .out(Out::Ok(o));
    s.op("get", "/nodes/{id}/changes", "Files", "Live changes under a folder as Server-Sent Events (`event: change`, `data: {\"node_id\": ...}`). Ids only.", User)
        .out(Out::Raw("text/event-stream", "A stream of change events."));
    let o = s.r::<ChangeFeed>();
    s.op("get", "/changes", "Files", "Every change in your tree and under what's shared with you, in order, after a cursor. An expired cursor gets `resync`.", User)
        .query("since", "integer", "The cursor from the last page; without it, just the current cursor.")
        .out(Out::Ok(o));

    // Versions
    let o = s.list::<FileVersion>();
    s.op(
        "get",
        "/nodes/{id}/versions",
        "Versions",
        "A file's versions.",
        User,
    )
    .out(Out::Ok(o));
    s.op(
        "delete",
        "/nodes/{id}/versions/{vid}",
        "Versions",
        "Delete an old version.",
        User,
    )
    .out(Out::Empty);
    let b = s.r::<PinVersionRequest>();
    s.op(
        "put",
        "/nodes/{id}/versions/{vid}/pinned",
        "Versions",
        "Pin or unpin a version, so automatic thinning passes it over.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    let (b, o) = (s.r::<RestoreVersionRequest>(), s.r::<Node>());
    s.op(
        "post",
        "/nodes/{id}/versions/{vid}/restore",
        "Versions",
        "Make an older version current again. The current one stays in the history.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    s.op(
        "get",
        "/nodes/{id}/versions/{vid}/chunks/{idx}",
        "Versions",
        "One encrypted chunk of a version.",
        User,
    )
    .out(Out::Ciphertext("The chunk."));

    // Trash
    let o = s.list::<TrashItem>();
    s.op("get", "/trash", "Trash", "What's in your trash.", User)
        .out(Out::Ok(o));
    s.op(
        "delete",
        "/trash",
        "Trash",
        "Empty the trash for good.",
        User,
    )
    .out(Out::Empty);
    s.op(
        "delete",
        "/trash/{id}",
        "Trash",
        "Delete one item for good.",
        User,
    )
    .out(Out::Empty);
    let (b, o) = (s.r::<RestoreTrashRequest>(), s.r::<Node>());
    s.op("post", "/trash/{id}/restore", "Trash", "Restore into the original folder or, if it's gone (409 `parent_unavailable`), into `parent_id` with a re-wrapped key.", User)
        .body(b).out(Out::Ok(o));

    // Uploads
    let (b, o) = (s.r::<CreateUploadRequest>(), s.r::<UploadResponse>());
    s.op(
        "post",
        "/uploads",
        "Uploads",
        "Start an upload: a new file, or a new version of one (with `if_revision`).",
        User,
    )
    .body(b)
    .out(Out::Created(o));
    s.op(
        "delete",
        "/uploads/{id}",
        "Uploads",
        "Give up on an upload and free its space.",
        User,
    )
    .out(Out::Empty);
    s.op(
        "put",
        "/uploads/{id}/chunks/{idx}",
        "Uploads",
        "Send one encrypted chunk.",
        User,
    )
    .bytes("The encrypted chunk.")
    .out(Out::Empty);
    let (b, o) = (s.r::<FinishUploadRequest>(), s.r::<Node>());
    s.op(
        "post",
        "/uploads/{id}/finish",
        "Uploads",
        "Finish an upload once every chunk is in.",
        User,
    )
    .optional_body(b)
    .out(Out::Ok(o));

    // Sharing
    let o = s.r::<UserPublicKey>();
    s.op(
        "get",
        "/users/{username}/public-key",
        "Sharing",
        "Someone's public keys, to check their fingerprint and seal a share to.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (s.r::<CreateShareRequest>(), s.r::<OutgoingShare>());
    s.op("post", "/shares", "Sharing", "Share a node: its key sealed to the recipient. Updates an existing share's key or permission.", User)
        .body(b).out(Out::Created(o));
    let o = s.list::<IncomingShare>();
    s.op(
        "get",
        "/shares/incoming",
        "Sharing",
        "What's shared with you.",
        User,
    )
    .out(Out::Ok(o));
    let o = s.list::<OutgoingShare>();
    s.op(
        "get",
        "/shares/outgoing",
        "Sharing",
        "What you've shared.",
        User,
    )
    .query("node_id", "string", "Only the shares of this node.")
    .out(Out::Ok(o));
    let b = s.r::<UpdateShareRequest>();
    s.op(
        "patch",
        "/shares/{id}",
        "Sharing",
        "Change a share's permission or expiry.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    s.op(
        "delete",
        "/shares/{id}",
        "Sharing",
        "Revoke a share (owner) or leave it (recipient).",
        User,
    )
    .out(Out::Empty);
    let b = s.r::<ResealShareRequest>();
    s.op(
        "put",
        "/shares/{id}/key",
        "Sharing",
        "The recipient seals a share's key again to their hybrid X25519 + ML-KEM-768 key.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    let o = s.list::<DroppedFile>();
    s.op(
        "get",
        "/drops",
        "Sharing",
        "Files dropped through your upload-only links, waiting to be taken in.",
        User,
    )
    .out(Out::Ok(o));
    s.op(
        "delete",
        "/drops/{id}",
        "Sharing",
        "Throw a dropped file away.",
        User,
    )
    .out(Out::Empty);
    let (b, o) = (s.r::<AdoptDropRequest>(), s.r::<Node>());
    s.op(
        "post",
        "/drops/{id}/adopt",
        "Sharing",
        "Take a dropped file in: its key wrapped under its folder's, instead of sealed to you.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    let (b, o) = (s.r::<CreateLinkRequest>(), s.r::<Link>());
    s.op(
        "post",
        "/links",
        "Sharing",
        "Make a public link. The link's key stays after the `#` and never reaches the server.",
        User,
    )
    .body(b)
    .out(Out::Created(o));
    let o = s.list::<Link>();
    s.op("get", "/links", "Sharing", "Your public links.", User)
        .query("node_id", "string", "Only the links of this node.")
        .out(Out::Ok(o));
    s.op(
        "delete",
        "/links/{id}",
        "Sharing",
        "Delete a public link.",
        User,
    )
    .out(Out::Empty);

    // Public links
    let o = s.r::<PublicLinkInfo>();
    s.op("get", "/public/{token}", "Public links", "Open a public link: its node, wrapped key, and for a link with limited opens a visit token (each call counts as an open).", Visitor)
        .out(Out::Ok(o));
    let (b, o) = (s.r::<UnlockLinkRequest>(), s.r::<UnlockLinkResponse>());
    s.op(
        "post",
        "/public/{token}/unlock",
        "Public links",
        "Unlock a password-protected link with an auth key derived from the password.",
        Visitor,
    )
    .body(b)
    .out(Out::Ok(o));
    let (list, page) = (s.list::<Node>(), s.r::<NodePage>());
    s.op(
        "get",
        "/public/{token}/nodes/{id}/children",
        "Public links",
        "A shared folder's children.",
        Visitor,
    )
    .query(
        "limit",
        "integer",
        "Page size; without it the whole array comes back.",
    )
    .query(
        "after",
        "string",
        "The `after` cursor of the previous page.",
    )
    .out(Out::Ok(json!({ "oneOf": [list, page] })));
    s.op(
        "get",
        "/public/{token}/nodes/{id}/chunks/{idx}",
        "Public links",
        "One encrypted chunk of a shared file.",
        Visitor,
    )
    .out(Out::Ciphertext("The chunk."));
    s.op(
        "get",
        "/public/{token}/nodes/{id}/thumbnail",
        "Public links",
        "A shared file's encrypted thumbnail.",
        Visitor,
    )
    .out(Out::Ciphertext("The encrypted thumbnail."));
    let (b, o) = (s.r::<CreateUploadRequest>(), s.r::<UploadResponse>());
    s.op(
        "post",
        "/public/{token}/uploads",
        "Public links",
        "Start an upload into a file drop. Its key is sealed to the owner.",
        Visitor,
    )
    .body(b)
    .out(Out::Created(o));
    s.op(
        "delete",
        "/public/{token}/uploads/{id}",
        "Public links",
        "Give up on a dropped upload.",
        Visitor,
    )
    .out(Out::Empty);
    s.op(
        "put",
        "/public/{token}/uploads/{id}/chunks/{idx}",
        "Public links",
        "Send one encrypted chunk of a dropped upload.",
        Visitor,
    )
    .bytes("The encrypted chunk.")
    .out(Out::Empty);
    s.op(
        "post",
        "/public/{token}/uploads/{id}/finish",
        "Public links",
        "Finish a dropped upload.",
        Visitor,
    )
    .out(Out::Empty);

    // Linked storage
    let o = s.r::<StorageInfo>();
    s.op(
        "get",
        "/storage",
        "Linked storage",
        "Your linked storage accounts, what each holds, and what's allowed.",
        User,
    )
    .out(Out::Ok(o));
    let b = s.r::<StoragePreferRequest>();
    s.op(
        "put",
        "/storage/prefer",
        "Linked storage",
        "Where new files go first: this server or a linked account.",
        User,
    )
    .body(b)
    .out(Out::Empty);
    let (b, o) = (s.r::<LinkStorageRequest>(), s.r::<LinkStorageResponse>());
    s.op(
        "post",
        "/storage/google",
        "Linked storage",
        "Start linking Google Drive: the consent page to open.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));
    s.op("get", "/storage/google/callback", "Linked storage", "Google's redirect back; answers 303 to `/storage-linked.html` with the outcome after the `#`.", Open)
        .query("code", "string", "From Google.")
        .query("state", "string", "From Google.")
        .query("error", "string", "From Google.")
        .out(Out::Raw("text/plain", "Never 200: always a 303 redirect."));
    let b = s.r::<UpdateStorageAccountRequest>();
    s.op(
        "patch",
        "/storage/accounts/{id}",
        "Linked storage",
        "Change a linked account's role (mirror or extra space).",
        User,
    )
    .body(b)
    .out(Out::Empty);
    s.op(
        "delete",
        "/storage/accounts/{id}",
        "Linked storage",
        "Unlink an account. What's kept only there must be moved home first.",
        User,
    )
    .out(Out::Empty);
    let (b, o) = (s.r::<MoveStorageRequest>(), s.r::<MoveStorageResponse>());
    s.op("post", "/storage/accounts/{id}/move", "Linked storage", "Move a batch of files between this server and the account. Call again until it says it's done.", User)
        .body(b).out(Out::Ok(o));
    let o = s.r::<NodeStorage>();
    s.op(
        "get",
        "/nodes/{id}/storage",
        "Linked storage",
        "Where a file's (or a folder's files') versions are kept.",
        User,
    )
    .out(Out::Ok(o));
    let (b, o) = (
        s.r::<MoveNodeStorageRequest>(),
        s.r::<MoveStorageResponse>(),
    );
    s.op(
        "post",
        "/nodes/{id}/storage",
        "Linked storage",
        "Move a batch of one file's (or folder's) versions. Owner only.",
        User,
    )
    .body(b)
    .out(Out::Ok(o));

    s.document()
}

fn generated() -> String {
    serde_json::to_string_pretty(&spec()).unwrap() + "\n"
}

#[test]
fn spec_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE);
    let fresh = generated();
    if std::env::var_os("UPDATE_OPENAPI").is_some() {
        std::fs::write(&path, &fresh).unwrap();
        return;
    }
    let kept = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        kept == fresh,
        "{} is out of date; rewrite it with\n\n    UPDATE_OPENAPI=1 cargo test -p thencloud-server --test openapi\n",
        path.display()
    );
}

/// Method and path of each `.route(...)` in the API router, read from its source.
fn routes_in_router() -> BTreeSet<(String, String)> {
    let src =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes/mod.rs"))
            .unwrap();
    // Only the `api` router: up to where the outer router nests it.
    let start = src.find("let api = Router::new()").expect("api router");
    let end = src[start..].find(".fallback(").expect("api fallback") + start;
    let api = &src[start..end];
    let mut found = BTreeSet::new();
    for call in api.split(".route(").skip(1) {
        let path = call.split('"').nth(1).expect("route path");
        // Up to the call's closing parenthesis.
        let mut depth = 1;
        let mut len = 0;
        for (i, c) in call.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                len = i;
                break;
            }
        }
        let body = &call[..len];
        for method in ["get", "post", "put", "patch", "delete"] {
            // `get(handler)` or `.post(handler)`, where a handler is `module::fn`.
            let pat = format!("{method}(");
            let mut rest = body;
            while let Some(at) = rest.find(&pat) {
                let before = rest[..at].chars().last();
                let after = &rest[at + pat.len()..];
                let handler = after.trim_start();
                let is_method = matches!(
                    before,
                    None | Some('.') | Some('(') | Some(' ') | Some('\n')
                ) && handler
                    .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                    .next()
                    .is_some_and(|h| h.contains("::"));
                if is_method {
                    found.insert((method.to_string(), path.to_string()));
                }
                rest = after;
            }
        }
    }
    found
}

#[test]
fn every_route_is_described() {
    let routes = routes_in_router();
    assert!(routes.len() > 100, "found only {} routes", routes.len());
    let spec = spec();
    let described: BTreeSet<(String, String)> = spec["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(path, item)| {
            item.as_object()
                .unwrap()
                .keys()
                .map(move |m| (m.clone(), path.clone()))
        })
        .collect();
    let missing: Vec<_> = routes.difference(&described).collect();
    let extra: Vec<_> = described.difference(&routes).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "routes not in the spec: {missing:?}\nin the spec but not routed: {extra:?}"
    );
}

/// Every `$ref` points at something in the components.
#[test]
fn references_resolve() {
    fn walk(v: &Value, doc: &Value, bad: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                if let Some(Value::String(r)) = m.get("$ref") {
                    let ptr = r.trim_start_matches('#');
                    if doc.pointer(ptr).is_none() {
                        bad.push(r.clone());
                    }
                }
                m.values().for_each(|v| walk(v, doc, bad));
            }
            Value::Array(a) => a.iter().for_each(|v| walk(v, doc, bad)),
            _ => {}
        }
    }
    let doc = spec();
    let mut bad = Vec::new();
    walk(&doc, &doc, &mut bad);
    assert!(bad.is_empty(), "unresolved: {bad:?}");
}

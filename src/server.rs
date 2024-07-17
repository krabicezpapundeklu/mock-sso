use std::{
    ffi::{OsStr, OsString},
    process::Stdio,
};

use anyhow::{bail, Context, Error, Result};

use axum::{
    extract::{Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        StatusCode, Uri,
    },
    response::{Html, IntoResponse, Response},
    routing::get,
    serve, Form, Router,
};

use base64::{prelude::BASE64_STANDARD, Engine};
use chrono::Utc;
use handlebars::Handlebars;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{io::AsyncWriteExt, net::TcpListener, process::Command};
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use url::Url;

const ISSUER_URL: &str = "https://mock-sso.mgspdtesting.com";

#[derive(Clone)]
struct AppContext {
    handlebars: Handlebars<'static>,
    key: OsString,
}

impl AppContext {
    fn new(key: OsString) -> Result<Self> {
        #[derive(RustEmbed)]
        #[folder = "templates"]
        #[include = "*.hbs"]
        struct Templates;

        let mut handlebars = Handlebars::new();

        #[cfg(debug_assertions)]
        handlebars.set_dev_mode(true);
        handlebars.set_strict_mode(true);

        handlebars.register_embed_templates_with_extension::<Templates>(".hbs")?;
        handlebars.register_template_string("VERSION", env!("CARGO_PKG_VERSION"))?;

        Ok(Self { handlebars, key })
    }
}

struct AppError(Error);

impl<E> From<E> for AppError
where
    E: Into<Error>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
    }
}

#[derive(Deserialize, Serialize)]
struct IndexData {
    #[serde(default)]
    environment: String,

    #[serde(default = "default_custom_target")]
    custom_target: String,

    #[serde(default = "default_user_id")]
    user_id: String,

    #[serde(default = "default_use_environment")]
    use_environment: bool,

    #[serde(default)]
    errors: Vec<String>,
}

fn default_custom_target() -> String {
    "http://localhost:8080/combined-app/home/saml.hms".to_string()
}

fn default_user_id() -> String {
    "sysdba".to_string()
}

const fn default_use_environment() -> bool {
    true
}

async fn get_asset(uri: Uri) -> Response {
    #[derive(RustEmbed)]
    #[folder = "dist"]
    #[include = "*.css"]
    #[include = "*.js"]
    #[cfg_attr(debug_assertions, include = "*.map")]
    struct Dist;

    #[derive(RustEmbed)]
    #[folder = "static"]
    struct Static;

    let path = uri.path().trim_start_matches('/');

    if let Some(asset) = Dist::get(path) {
        (
            [
                (CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (CONTENT_TYPE, asset.metadata.mimetype()),
            ],
            asset.data,
        )
            .into_response()
    } else if let Some(asset) = Static::get(path) {
        ([(CONTENT_TYPE, asset.metadata.mimetype())], asset.data).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn get_index(
    State(app_context): State<AppContext>,
    Query(query): Query<IndexData>,
) -> Result<Html<String>, AppError> {
    app_context
        .handlebars
        .render("index", &query)
        .map(Html)
        .map_err(Into::into)
}

async fn sign(data: &[u8], key: impl AsRef<OsStr> + Send) -> Result<Vec<u8>> {
    let mut child = Command::new("xmlsec1")
        .arg("--sign")
        .arg("--privkey-pem")
        .arg(key)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot spawn child")?;

    let mut stdin = child
        .stdin
        .take()
        .context("child did not have a handle to stdin")?;

    stdin
        .write(data)
        .await
        .context("could not write to stdin")?;

    drop(stdin);

    let output = child.wait_with_output().await?;

    if !output.stderr.is_empty() {
        bail!(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(output.stdout)
}

pub async fn start(host: &str, port: u16, key: OsString) -> Result<()> {
    let app_context = AppContext::new(key)?;

    let router = Router::new()
        .route("/", get(get_index).post(submit))
        .fallback(get(get_asset))
        .with_state(app_context)
        .layer(ServiceBuilder::new().layer(CompressionLayer::new()));

    let listener = TcpListener::bind((host, port)).await?;

    serve(listener, router).await.map_err(Into::into)
}

async fn submit(
    State(app_context): State<AppContext>,
    Form(mut form): Form<IndexData>,
) -> Result<Html<String>, AppError> {
    let target;
    let environment_target;

    if form.use_environment {
        let environment = form.environment.trim();

        if environment.is_empty() {
            target = "";

            form.errors
                .push("'Environment' is required field.".to_string());
        } else {
            environment_target =
                format!("https://{environment}-ats.mgspdtesting.com/{environment}/home/saml.hms");

            target = &environment_target;
        }
    } else {
        target = form.custom_target.trim();

        if target.is_empty() {
            form.errors
                .push("'Custom Target' is required field.".to_string());
        }
    }

    let user_id = form.user_id.trim();

    if user_id.is_empty() {
        form.errors.push("'User ID' is required field.".to_string());
    }

    if !form.errors.is_empty() {
        return Ok(Html(app_context.handlebars.render("index", &form)?));
    }

    let saml_response = app_context.handlebars.render(
        "saml-response",
        &json!({
            "issuer_url": ISSUER_URL,
            "timestamp": Utc::now().to_rfc3339(),
            "user_id": user_id,
        }),
    )?;

    let signed_saml_response = sign(saml_response.as_bytes(), &app_context.key).await?;
    let target_url = Url::parse(target)?;

    let relay_state = target_url
        .path_segments()
        .map_or("", |mut segments| segments.next().unwrap_or_default());

    app_context
        .handlebars
        .render(
            "saml-redirect",
            &json!({
                "relay_state": relay_state,
                "saml_response": BASE64_STANDARD.encode(signed_saml_response),
                "target": &target,
            }),
        )
        .map(Html)
        .map_err(Into::into)
}

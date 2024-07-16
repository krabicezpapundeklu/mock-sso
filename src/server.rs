use std::{
    ffi::{OsStr, OsString},
    process::Stdio,
};

use anyhow::{Context, Error, Result};

use axum::{
    extract::State,
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
use serde::Deserialize;
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
    cert: OsString,
}

impl AppContext {
    fn new(key: OsString, cert: OsString) -> Result<Self> {
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

        Ok(Self {
            handlebars,
            key,
            cert,
        })
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

#[derive(Deserialize)]
struct SubmitForm {
    target: String,
    user_id: String,
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

async fn get_index(State(app_context): State<AppContext>) -> Result<Html<String>, AppError> {
    app_context
        .handlebars
        .render("index", &json!({}))
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

    Ok(output.stdout)
}

pub async fn start(host: &str, port: u16, key: OsString, cert: OsString) -> Result<()> {
    let app_context = AppContext::new(key, cert)?;

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
    Form(form): Form<SubmitForm>,
) -> Result<Html<String>, AppError> {
    let saml_response = app_context.handlebars.render(
        "saml-response",
        &json!({
            "issuerUrl": ISSUER_URL,
            "SAMLConsumerUrl": form.target,
            "SAMLNameId": form.user_id,
            "timestamp": Utc::now().to_rfc3339(),
        }),
    )?;

    let signed_saml_response = sign(saml_response.as_bytes(), &app_context.key).await?;
    let target_url = Url::parse(&form.target)?;

    let relay_state = target_url
        .path_segments()
        .map_or("", |mut segments| segments.next().unwrap_or_default());

    let response = app_context.handlebars.render(
        "saml-redirect",
        &json!({
            "target": &form.target,
            "samlResponse": BASE64_STANDARD.encode(signed_saml_response),
            "relayState": relay_state
        }),
    )?;

    Ok(Html(response))
}

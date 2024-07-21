use std::{
    ffi::{OsStr, OsString},
    process::Stdio,
};

use anyhow::{bail, Context, Error, Result};

use axum::{
    extract::{Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        uri::PathAndQuery,
        StatusCode, Uri,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    serve, Form, Router,
};

use axum_extra::extract::{cookie::Cookie, CookieJar};
use base64::{prelude::BASE64_STANDARD, Engine};
use chrono::Utc;
use cookie::time::Duration;
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
struct IndexInputData {
    environment: Option<String>,
    custom_target: Option<String>,
    user_id: Option<String>,
    use_environment: Option<bool>,
}

#[derive(Deserialize, Serialize)]
struct IndexSubmitData {
    environment: String,
    custom_target: String,
    user_id: String,
    use_environment: bool,
}

#[derive(Serialize)]
struct IndexOutputData<'a> {
    #[serde(flatten)]
    input_data: &'a IndexSubmitData,

    target: &'a str,
    saml_response: &'a str,
    relay_state: &'a str,

    errors: &'a Vec<&'a str>,
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

    let path = uri.path();

    if !path.starts_with("/mock-sso/") {
        return Redirect::permanent(&format!(
            "/mock-sso{}",
            uri.path_and_query()
                .map(PathAndQuery::as_str)
                .unwrap_or_default()
        ))
        .into_response();
    }

    let path = uri.path().trim_start_matches("/mock-sso/");

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
    Query(query): Query<IndexInputData>,
    jar: CookieJar,
) -> Result<Html<String>, AppError> {
    let environment = query
        .environment
        .as_deref()
        .unwrap_or_else(|| jar.get("environment").map_or("", |cookie| cookie.value()));

    let custom_target = query.custom_target.as_deref().unwrap_or_else(|| {
        jar.get("custom_target").map_or(
            "http://localhost:8080/combined-app/home/saml.hms",
            |cookie| cookie.value(),
        )
    });

    let user_id = query
        .user_id
        .as_deref()
        .unwrap_or_else(|| jar.get("user_id").map_or("sysdba", |cookie| cookie.value()));

    let use_environment = query.use_environment.unwrap_or_else(|| {
        jar.get("use_environment")
            .map_or("true", |cookie| cookie.value())
            == "true"
    });

    app_context
        .handlebars
        .render(
            "index",
            &json!({
                "environment": environment,
                "custom_target": custom_target,
                "user_id": user_id,
                "use_environment": use_environment
            }),
        )
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
        .route("/mock-sso/", get(get_index).post(submit))
        .fallback(get(get_asset))
        .with_state(app_context)
        .layer(ServiceBuilder::new().layer(CompressionLayer::new()));

    let listener = TcpListener::bind((host, port)).await?;

    serve(listener, router).await.map_err(Into::into)
}

async fn submit(
    State(app_context): State<AppContext>,
    jar: CookieJar,
    Form(form): Form<IndexSubmitData>,
) -> Result<impl IntoResponse, AppError> {
    let mut errors = Vec::new();

    let target;
    let environment_target;

    if form.use_environment {
        let environment = form.environment.trim();

        if environment.is_empty() {
            target = "";
            errors.push("'Environment' is required field.");
        } else {
            environment_target =
                format!("https://{environment}-ats.mgspdtesting.com/{environment}/home/saml.hms");

            target = &environment_target;
        }
    } else {
        target = form.custom_target.trim();

        if target.is_empty() {
            errors.push("'Custom Target' is required field.");
        }
    }

    let target_url = Url::parse(target);

    let relay_state = if let Ok(target_url) = &target_url {
        target_url
            .path_segments()
            .map_or("", |mut segments| segments.next().unwrap_or_default())
    } else {
        if !target.is_empty() {
            errors.push(if form.use_environment {
                "'Environment' has invalid format."
            } else {
                "'Custom Target' has invalid format."
            });
        }

        ""
    };

    let user_id = form.user_id.trim();

    if user_id.is_empty() {
        errors.push("'User ID' is required field.");
    }

    let saml_response = if errors.is_empty() {
        let saml_response = app_context.handlebars.render(
            "saml-response",
            &json!({
                "issuer_url": ISSUER_URL,
                "timestamp": Utc::now().to_rfc3339(),
                "user_id": user_id,
            }),
        )?;

        let signed_saml_response = sign(saml_response.as_bytes(), &app_context.key).await?;

        Some(BASE64_STANDARD.encode(signed_saml_response))
    } else {
        None
    };

    let data = IndexOutputData {
        input_data: &form,
        target,
        saml_response: saml_response.as_deref().unwrap_or_default(),
        relay_state,
        errors: &errors,
    };

    let output = app_context.handlebars.render("index", &data).map(Html)?;

    let jar = jar
        .add(Cookie::build(("environment", form.environment)).max_age(Duration::MAX))
        .add(Cookie::build(("custom_target", form.custom_target)).max_age(Duration::MAX))
        .add(Cookie::build(("user_id", form.user_id)).max_age(Duration::MAX))
        .add(
            Cookie::build((
                "use_environment",
                if form.use_environment {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ))
            .max_age(Duration::MAX),
        );

    Ok((jar, output))
}

use std::{
    ffi::{OsStr, OsString},
    process::Stdio,
    str::FromStr,
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
    serve, Router,
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

trait Cookies {
    fn add_cookie(self, name: &'static str, value: Option<String>) -> Self;

    fn fill_option_if_none<T: FromStr>(
        &self,
        option: &mut Option<T>,
        name: &str,
        default_value: &str,
    ) -> Result<(), T::Err>;
}

impl Cookies for CookieJar {
    fn add_cookie(self, name: &'static str, value: Option<String>) -> Self {
        self.add(Cookie::build((name, value.unwrap_or_default())).max_age(Duration::WEEK))
    }

    fn fill_option_if_none<T: FromStr>(
        &self,
        option: &mut Option<T>,
        name: &str,
        default_value: &str,
    ) -> Result<(), T::Err> {
        if option.is_none() {
            let value = self
                .get(name)
                .map_or(default_value, |cookie| cookie.value());

            (*option).replace(T::from_str(value)?);
        }

        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct IndexInputData {
    environment: Option<String>,
    custom_target: Option<String>,
    user_id: Option<String>,
    use_environment: Option<bool>,
    login: Option<bool>,
}

impl IndexInputData {
    const ENVIRONMENT: &'static str = "environment";
    const CUSTOM_TARGET: &'static str = "custom_target";
    const USER_ID: &'static str = "user_id";
    const USE_ENVIRONMENT: &'static str = "use_environment";

    fn save_to_cookies(self, cookies: CookieJar) -> CookieJar {
        cookies
            .add_cookie(Self::ENVIRONMENT, self.environment)
            .add_cookie(Self::CUSTOM_TARGET, self.custom_target)
            .add_cookie(Self::USER_ID, self.user_id)
            .add_cookie(
                Self::USE_ENVIRONMENT,
                Some(self.use_environment.unwrap_or(true).to_string()),
            )
    }

    fn use_saved_or_default_values(&mut self, cookies: &CookieJar) -> Result<()> {
        cookies.fill_option_if_none(&mut self.environment, Self::ENVIRONMENT, "")?;

        cookies.fill_option_if_none(
            &mut self.custom_target,
            Self::CUSTOM_TARGET,
            "http://localhost:8080/combined-app/home/saml.hms",
        )?;

        cookies.fill_option_if_none(&mut self.user_id, Self::USER_ID, "sysdba")?;
        cookies.fill_option_if_none(&mut self.use_environment, Self::USE_ENVIRONMENT, "true")?;

        self.login = Some(self.login.unwrap_or_default());

        Ok(())
    }
}

#[derive(Serialize)]
struct IndexOutputData<'a> {
    #[serde(flatten)]
    input_data: &'a IndexInputData,

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
    Query(mut query): Query<IndexInputData>,
    mut cookies: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let login = query.login.unwrap_or_default();

    let mut errors = Vec::new();
    let mut target = "";
    let mut saml_response = None;
    let mut relay_state = "";

    let environment_target;
    let target_url;

    if login {
        if query.use_environment.unwrap_or(true) {
            let environment = query.environment.as_deref().unwrap_or_default().trim();

            if environment.is_empty() {
                target = "";
                errors.push("'Environment' is required field.");
            } else {
                environment_target = format!(
                    "https://{environment}-ats.mgspdtesting.com/{environment}/home/saml.hms"
                );

                target = &environment_target;
            }
        } else {
            target = query.custom_target.as_deref().unwrap_or_default().trim();

            if target.is_empty() {
                errors.push("'Custom Target' is required field.");
            }
        }

        relay_state = if !target.is_empty() {
            target_url = Url::parse(target);

            if let Ok(target_url) = &target_url {
                target_url
                    .path_segments()
                    .map_or("", |mut segments| segments.next().unwrap_or_default())
            } else {
                errors.push(if query.use_environment.unwrap_or(true) {
                    "'Environment' has invalid format."
                } else {
                    "'Custom Target' has invalid format."
                });

                ""
            }
        } else {
            ""
        };

        let user_id = query.user_id.as_deref().unwrap_or_default().trim();

        if user_id.is_empty() {
            errors.push("'User ID' is required field.");
        }

        saml_response = if errors.is_empty() {
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
    } else {
        query.use_saved_or_default_values(&cookies)?;
    }

    let data = IndexOutputData {
        input_data: &query,
        target,
        saml_response: saml_response.as_deref().unwrap_or_default(),
        relay_state,
        errors: &errors,
    };

    let output = Html(app_context.handlebars.render("index", &data)?);

    cookies = query.save_to_cookies(cookies);

    Ok((cookies, output))
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
        .route("/mock-sso/", get(get_index))
        .fallback(get(get_asset))
        .with_state(app_context)
        .layer(ServiceBuilder::new().layer(CompressionLayer::new()));

    let listener = TcpListener::bind((host, port)).await?;

    serve(listener, router).await.map_err(Into::into)
}

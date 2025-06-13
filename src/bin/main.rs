use axum::{
    Json, RequestExt, Router,
    body::Body,
    extract::{FromRequest, Query, Request, State},
    http::{
        StatusCode,
        header::{self, HeaderValue},
    },
    response::{Html, IntoResponse, Response},
    routing::{get, post, post_service},
};
use clap::{Arg, ArgAction, Command, value_parser};
use clap_complete::aot::{Generator, Shell, generate};
use futures_util::stream::unfold;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use tokio::{net::TcpListener, sync::mpsc};
use tower::service_fn;
use utsuru::{mirrors::DiscordLiveBuilder, sources::WHIP};

const INDEX_HTML: &str = include_str!("../../web_dist/index.html");
const INDEX_CSS: &str = include_str!("../../web_dist/bundle.css");
const INDEX_JS: &str = include_str!("../../web_dist/bundle.js");
const FAVICON_PNG: &[u8] = include_bytes!("../../web_dist/favicon.png");

pub fn main() {
    let result = start();

    if let Some(err) = result.err() {
        println!("Error: {err}");
    }
}

#[tokio::main]
async fn start() -> Result<(), Box<dyn std::error::Error>> {
    let matches = build_cli().get_matches();

    if let Some(generator) = matches.get_one::<Shell>("completions") {
        let mut cmd = build_cli();
        print_completions(*generator, &mut cmd);
        return Ok(());
    }

    let log: tracing::level_filters::LevelFilter = *matches.get_one("verbosity").unwrap();
    tracing_subscriber::fmt()
        .compact()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(
                    format!("{}={}", env!("CARGO_CRATE_NAME"), log)
                        .parse()
                        .unwrap(),
                )
                .from_env_lossy(),
        )
        .init();

    println!();
    println!("  +---------------------+");
    println!("  |                     |");
    println!(
        "  |{:^21}|",
        format!(
            "{} v{}",
            env!("CARGO_CRATE_NAME"),
            env!("CARGO_PKG_VERSION")
        )
    );
    println!("  |                     |");
    println!("  +---------------------+");
    println!();
    println!("  - Thank you for using {}.", env!("CARGO_CRATE_NAME"));
    println!("    We are currently conducting internal preparations. Please wait...");
    println!();

    let ip: IpAddr = *matches.get_one("host").unwrap();
    let port: u16 = *matches.get_one("port").unwrap();
    let addr = SocketAddr::from((ip, port));
    let listener = match TcpListener::bind(&addr).await {
        Ok(sock) => sock,
        Err(e) => {
            println!("  - An error has occured:");
            println!("    {e}");
            println!();
            return Ok(());
        }
    };

    let whip = WHIP::new(addr.ip());
    let whip_service = service_fn(whip.into_closure());

    let app = Router::new()
        .route("/", get(Html(INDEX_HTML)))
        .route(
            "/bundle.css",
            get(|| assets_get("text/css; charset=utf-8", INDEX_CSS)),
        )
        .route(
            "/bundle.js",
            get(|| assets_get("application/javascript; charset=utf-8", INDEX_JS)),
        )
        .route("/favicon.png", get(|| assets_get("image/png", FAVICON_PNG)))
        .route("/api/mirrors", get(mirrors_get))
        .route("/api/mirrors", post(mirrors_post))
        .route("/whip", post_service(whip_service))
        .with_state(whip);

    println!("  - {} is ready! Listening on:", env!("CARGO_CRATE_NAME"));
    println!("    Web UI:      http://{}", listener.local_addr().unwrap());
    println!(
        "    WHIP Server: http://{}/whip",
        listener.local_addr().unwrap()
    );
    println!("    WHIP Token:  {}", env!("CARGO_CRATE_NAME"));
    println!();

    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn assets_get(header: &'static str, body: impl IntoResponse) -> Response {
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static(header))],
        body,
    )
        .into_response()
}

async fn mirrors_get(State(whip): State<WHIP>) -> Result<Json<Vec<bool>>, StatusCode> {
    let Ok(mirrors) = whip.view_mirrors().await else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    Ok(Json(mirrors))
}

async fn mirrors_post(State(whip): State<WHIP>, action: Action) -> Result<Response, StatusCode> {
    match action {
        Action::Create(payload) => create_mirror(whip, payload).await,
        Action::Delete(payload) => delete_mirror(whip, payload).await,
    }
}

enum Action {
    Create(CreatePayload),
    Delete(DeletePayload),
}

impl<S> FromRequest<S> for Action
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(mut req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(params): Query<Params> = req
            .extract_parts()
            .await
            .map_err(IntoResponse::into_response)?;
        match params {
            Params::Create => {
                let Json(payload): Json<CreatePayload> =
                    req.extract().await.map_err(IntoResponse::into_response)?;
                Ok(Self::Create(payload))
            }
            Params::Delete => {
                let Json(payload): Json<DeletePayload> =
                    req.extract().await.map_err(IntoResponse::into_response)?;
                Ok(Self::Delete(payload))
            }
            _ => Err(StatusCode::BAD_REQUEST.into_response()),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum Params {
    Create,
    Delete,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct CreatePayload {
    token: String,
    guild_id: u64,
    channel_id: u64,
}

#[derive(Deserialize)]
struct DeletePayload {
    id: usize,
}

async fn create_mirror(whip: WHIP, payload: CreatePayload) -> Result<Response, StatusCode> {
    let (trace_tx, trace_rx) = mpsc::unbounded_channel();
    let client = DiscordLiveBuilder::new(payload.token, payload.guild_id, payload.channel_id)
        .connect(Some(trace_tx));
    let client = Box::pin(client);

    let stream = unfold(Some((trace_rx, client, whip)), async move |state| {
        let (mut trace_rx, mut client, whip) = state?;
        tokio::select! {
            res = trace_rx.recv() => {
                let trace = res?;
                let body = format!("{trace}");
                Some((Ok::<_, Box<dyn std::error::Error + Send + Sync>>(body), Some((trace_rx, client, whip))))
            },
            mir = (&mut client) => {
                let body = match mir {
                    Ok(client) => {
                        match whip.add_mirror(client).await {
                            Ok(_) => "success".into(),
                            Err(e) => format!("error: {e}")
                        }
                    },
                    Err(e) => format!("error: {e}")
                };
                Some((Ok::<_, Box<dyn std::error::Error + Send + Sync>>(body), None))
            },
        }
    });

    let resp = Response::builder().body(Body::from_stream(stream));
    let Ok(resp) = resp else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    Ok(resp)
}

async fn delete_mirror(whip: WHIP, payload: DeletePayload) -> Result<Response, StatusCode> {
    let Ok(_) = whip.remove_mirror(payload.id).await else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    Ok(().into_response())
}

fn build_cli() -> Command {
    Command::new(env!("CARGO_CRATE_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .disable_help_flag(true)
        .arg(
            Arg::new("host")
                .short('h')
                .long("host")
                .value_parser(value_parser!(IpAddr))
                .default_value("127.0.0.1")
                .help("Specify bind address"),
        )
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_parser(value_parser!(u16))
                .default_value("3000")
                .help("Specify port to listen on"),
        )
        .arg(
            Arg::new("verbosity")
                .short('v')
                .long("verbosity")
                .value_parser(value_parser!(tracing::level_filters::LevelFilter))
                .default_value("off")
                .help("Log verbosity"),
        )
        .arg(
            Arg::new("completions")
                .long("completions")
                .value_parser(value_parser!(Shell))
                .help("Print shell completion script for <shell>"),
        )
        .arg(
            Arg::new("help")
                .long("help")
                .global(true)
                .action(ArgAction::Help)
                .help("Print help"),
        )
}

fn print_completions<G: Generator>(generator: G, cmd: &mut Command) {
    generate(
        generator,
        cmd,
        cmd.get_name().to_string(),
        &mut std::io::stdout(),
    );
}

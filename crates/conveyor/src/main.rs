use std::{fmt::Display, path::PathBuf};

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use database::{Database, Ring, RingEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod database;

type Result<T = (), E = Box<dyn std::error::Error>> = std::result::Result<T, E>;
type ResponsePair<T = Value> = (StatusCode, Json<T>);

#[tokio::main]
async fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let db_path = std::env::var("RING_DATA_VIEWER_DATA_PATH")
        .map(|s| PathBuf::from(&s))
        .unwrap_or_else(|_| PathBuf::from("./data.db"));
    let database = Database::new(&db_path).unwrap();
    // build our application with a route
    let app = Router::new()
        .nest_service("/", tower_http::services::ServeDir::new("assets"))
        .nest_service(
            "/api",
            Router::new()
                .route("/rings", get(get_rings))
                .route("/ring", post(add_ring).put(update_ring))
                .route("/ring/:id", get(get_ring))
                .route("/events/:id", post(add_events).get(get_events_for_ring))
                .with_state(database),
        )
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(65535));

    let port = std::env::var("RING_VIEWER_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(4000);
    let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

fn into_response(value: impl Serialize, status: StatusCode, context: impl Display) -> ResponsePair {
    let v = match serde_json::to_value(&value) {
        Ok(v) => v,
        Err(e) => return err(e, context, None),
    };

    (status, Json(v))
}

fn err(
    e: impl Display,
    context: impl Display,
    status: impl Into<Option<StatusCode>>,
) -> ResponsePair {
    let status = status.into().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    into_response(
        ApiError {
            context: context.to_string(),
            error: e.to_string(),
        },
        status,
        "error ctor",
    )
}

async fn get_rings(db: State<Database>) -> ResponsePair {
    into_response(db.get_rings(), StatusCode::OK, "get_rings")
}

async fn get_ring(db: State<Database>, mac: Path<String>) -> ResponsePair {
    match db.get_ring(&mac.0) {
        Ok(ring) => into_response(ring, StatusCode::OK, "get_rings"),
        Err(e) => err(e, "get ring by mac", None),
    }
}

async fn add_ring(db: State<Database>, ring: Json<Ring>) -> ResponsePair {
    match db.add_ring(&ring.0) {
        Ok(()) => into_response(serde_json::Map::new(), StatusCode::OK, "add_ring"),
        Err(e) => err(e, "add_ring", None),
    }
}

async fn update_ring(db: State<Database>, ring: Json<Ring>) -> ResponsePair {
    match db.update_ring(&ring.0) {
        Ok(()) => into_response(serde_json::Map::new(), StatusCode::OK, "add_ring"),
        Err(e) => err(e, "add_ring", None),
    }
}

async fn add_events(db: State<Database>, events: Json<Vec<RingEvent>>) -> ResponsePair {
    match db.add_events(&events) {
        Ok(()) => into_response(serde_json::Map::new(), StatusCode::OK, "add_events"),
        Err(e) => err(e, "add_events", None),
    }
}

#[derive(Debug, Deserialize)]
struct EventsArgs {
    date: time::OffsetDateTime,
}

async fn get_events_for_ring(
    db: State<Database>,
    mac: Path<String>,
    args: Query<EventsArgs>,
) -> ResponsePair {
    match db.get_events_for_ring(&mac.0, args.0.date) {
        Ok(list) => into_response(list, StatusCode::OK, "get_events_for_ring"),
        Err(e) => err(e, "add_events", None),
    }
}

// fn get_utc_date_parts(date: OffsetDateTime) -> Result<(u16, u8, u8)> {
//     let date = date.replace_offset(time::UtcOffset::UTC);
//     let year = u16::try_from(date.year())?;
//     let month = u8::try_from(date.month())?;
//     let day = date.day();
//     Ok((year, month, day))
// }

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub context: String,
}

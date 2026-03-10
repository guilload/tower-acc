mod sim;

use axum::Json;
use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};

use sim::{SimConfig, TracePoint, run};

static INDEX_HTML: &str = include_str!("../static/index.html");

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn defaults() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&SimConfig::default()).unwrap(),
    )
}

async fn simulate(Json(config): Json<SimConfig>) -> Result<Json<Vec<TracePoint>>, StatusCode> {
    let trace = run(&config);
    Ok(Json(trace))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/defaults", get(defaults))
        .route("/api/simulate", post(simulate));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("tower-acc-sim running at http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}

use crate::AppState;
use axum::extract::State;
use axum::response::IntoResponse;

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4")],
        state.metrics.render(),
    )
}

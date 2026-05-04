use axum::{http::StatusCode, response::IntoResponse};

pub enum ApiError {
    InternalServerError(Option<String>),
    NotFound(Option<String>),
    BadRequest(Option<String>),
    InvalidInput(Option<String>),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let err = match self {
            ApiError::InternalServerError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                msg.unwrap_or("Something went wrong!".into()),
            ),
            ApiError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, msg.unwrap_or("Try again!".into()))
            }
            ApiError::InvalidInput(msg) => (
                StatusCode::BAD_REQUEST,
                msg.unwrap_or("Invalid Inputs!".into()),
            ),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.unwrap_or("Not Found!".into())),
        };

        err.into_response()
    }
}

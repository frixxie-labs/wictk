use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationError {
    /// The request was invalid.
    BadRequest(String),
    /// The requested resource does not exist.
    NotFound(String),
    /// An upstream provider failed or returned invalid data.
    BadGateway(String),
}

impl ApplicationError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadGateway(_) => StatusCode::BAD_GATEWAY,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::BadRequest(message) | Self::NotFound(message) | Self::BadGateway(message) => {
                message
            }
        }
    }
}

impl std::fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ApplicationError {}

impl IntoResponse for ApplicationError {
    fn into_response(self) -> Response {
        (self.status_code(), self.message().to_owned()).into_response()
    }
}

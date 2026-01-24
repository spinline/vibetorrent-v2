use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("rTorrent connection error: {0}")]
    RtorrentConnection(String),
    
    #[error("rTorrent SCGI error: {0}")]
    ScgiError(String),
    
    #[error("XML-RPC error: {0}")]
    XmlRpcError(String),

    #[error("XML build error: {0}")]
    XmlBuildError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Template error: {0}")]
    TemplateError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::RtorrentConnection(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            AppError::ScgiError(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            AppError::XmlRpcError(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            AppError::XmlBuildError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::IoError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::TemplateError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
        };
        
        tracing::error!("Error: {}", message);
        
        (status, message).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

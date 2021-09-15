use thiserror::Error;

/// Describes the various errors that can be returned from the client
#[derive(Error, Debug)]
pub enum ClientError {
    /// Indicates that the given URL is invalid, contains the underlying parsing error
    #[error("Invalid URL given")]
    InvalidUrl(#[from] url::ParseError),
    /// Invalid configuration was given to the client
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    /// IO errors from interacting with the file system
    #[error("Error while performing IO operation")]
    Io(#[from] std::io::Error),
    /// Invalid TOML parsing that can occur when loading an invoice or label from disk
    #[error("Invalid TOML")]
    InvalidToml(#[from] toml::de::Error),
    /// Invalid TOML serialization that can occur when serializing an object to a request
    #[error("Failed serializing TOML")]
    TomlSerializationError(#[from] toml::ser::Error),
    #[error("Failed serializing JSON")]
    JsonSerializationError(#[from] serde_json::Error),
    /// There was a problem with the http client. This is likely not a user issue. Contains the
    /// underlying error
    #[error("Error creating request")]
    HttpClientError(#[from] reqwest::Error),
    /// An invalid ID was given. Returns the underlying parse error
    #[error("Invalid id")]
    InvalidId(#[from] crate::id::ParseError),

    /// An error occurred with the authentication token as described by the contained message. These
    /// errors can vary widely depending on the authentication method used.
    #[error("Token error: {0}")]
    TokenError(String),

    // API errors
    /// The invoice was not found. Note that this does not necessarily mean it doesn't exist. It
    /// could also be hidden because it is yanked or due to user permissions
    #[error("Invoice was not found")]
    InvoiceNotFound,
    /// The parcel was not found.
    #[error("Parcel was not found")]
    ParcelNotFound,

    #[error("Requested resource or endpoint is not found")]
    ResourceNotFound,
    /// The invoice already exists
    #[error("Invoice already exists")]
    InvoiceAlreadyExists,
    /// The parcel already exists.
    #[error("Parcel already exists")]
    ParcelAlreadyExists,
    /// The error returned when the request is invalid. Contains the underlying HTTP status code and
    /// any message returned from the API
    #[error("Invalid request (status code {status_code:?}): {}", .message.clone().unwrap_or_else(|| "unknown error".to_owned()))]
    InvalidRequest {
        status_code: reqwest::StatusCode,
        message: Option<String>,
    },
    /// A server error was encountered. Contains an optional message from the server
    #[error("Error contacting server: {}", .0.clone().unwrap_or_else(||"Protocol error. Verify the Bindle URL".to_owned()))]
    ServerError(Option<String>),
    /// Invalid credentials were used or user does not have access to the requested resource. This
    /// is only valid if the server supports authentication and/or permissions
    #[error("User has invalid credentials or is not authorized to access the requested resource")]
    Unauthorized,

    /// There was an error with the signature on an invoice
    #[error("Signature error")]
    SignatureError(#[from] crate::invoice::signature::SignatureError),

    /// A catch-all for uncategorized errors. Contains an error message describing the underlying
    /// issue
    #[error("{0}")]
    Other(String),
}

impl From<std::convert::Infallible> for ClientError {
    fn from(_: std::convert::Infallible) -> Self {
        // Doesn't matter what we return as Infallible cannot happen
        ClientError::Other("Shouldn't happen".to_string())
    }
}

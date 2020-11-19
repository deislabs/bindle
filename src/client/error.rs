use thiserror::Error;

/// Describes the various errors that can be returned from the client
#[derive(Error, Debug)]
pub enum ClientError {
    /// Indicates that the given URL is invalid, contains the underlying parsing error
    #[error("Invalid URL given: {0:?}")]
    InvalidURL(#[from] url::ParseError),
    /// Invalid configuration was given to the client
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    /// IO errors when reading data from a file
    #[error("Error while reading file from disk: {0:?}")]
    ReadIo(#[from] std::io::Error),
    /// Invalid TOML parsing that can occur when loading an invoice or label from disk
    #[error("Invalid toml: {0:?}")]
    InvalidToml(#[from] toml::de::Error),

    // API errors
    /// The invoice was not found. Contains the name of the requested invoice. Note that this does
    /// not necessarily mean it doesn't exist. It could also be hidden because it is yanked or due
    /// to user permissions
    #[error("Invoice {0} was not found")]
    InvoiceNotFound(String),
    /// The parcel was not found. Contains the ID of the requested parcel.
    #[error("Parcel {0} was not found")]
    ParcelNotFound(String),
    /// The invoice already exists. Contains the name of the invoice the request attempted to create.
    #[error("Invoice {0} already exists")]
    InvoiceAlreadyExists(String),
    /// The parcel already exists. Contains the ID of the parcel the request attempted to create.
    #[error("Parcel {0} already exists")]
    ParcelAlreadyExists(String),
    /// The error returned when the request is invalid. Contains the underlying HTTP status code and
    /// any message returned from the API
    #[error("Invalid request (status code {status_code:?}): {message:?}")]
    InvalidRequest {
        status_code: reqwest::StatusCode,
        message: String,
    },
    /// A server error was encountered. Contains an optional message from the server
    #[error("Server has encountered an error: {0:?}")]
    ServerError(Option<String>),
    /// Invalid credentials were used or user does not have access to the requested resource. This
    /// is only valid if the server supports authentication and/or permissions
    #[error("User has invalid credentials or is not authorized to access the requested resource")]
    Unauthorized,

    /// A catch-all for uncategorized errors. Contains an error message describing the underlying
    /// issue
    #[error("{0}")]
    Other(String),
}

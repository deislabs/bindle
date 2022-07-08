//! Contains various type definitions for API request and response types that leverage the Bindle
//! objects

use serde::{Deserialize, Serialize};

use crate::invoice::{Invoice, Label};
use crate::search::SearchOptions;
use crate::SignatureRole;

/// A custom type for responding to invoice creation requests. Because invoices can be created
/// before parcels are uploaded, this allows the API to inform the user if there are missing parcels
/// in the bindle spec
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InvoiceCreateResponse {
    pub invoice: Invoice,
    pub missing: Option<Vec<Label>>,
}

/// A response to a missing parcels request. TOML doesn't support top level arrays, so they
/// must be embedded in a table
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingParcelsResponse {
    pub missing: Vec<Label>,
}

/// A string error message returned from the server
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Available options for the query API
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct QueryOptions {
    #[serde(alias = "q")]
    pub query: Option<String>,
    #[serde(alias = "v")]
    pub version: Option<String>,
    #[serde(alias = "o")]
    pub offset: Option<u64>,
    #[serde(alias = "l")]
    pub limit: Option<u8>,
    pub strict: Option<bool>,
    pub yanked: Option<bool>,
}

impl From<QueryOptions> for SearchOptions {
    fn from(qo: QueryOptions) -> Self {
        let defaults = SearchOptions::default();
        SearchOptions {
            limit: qo.limit.unwrap_or(defaults.limit),
            offset: qo.offset.unwrap_or(defaults.offset),
            strict: qo.strict.unwrap_or(defaults.strict),
            yanked: qo.yanked.unwrap_or(defaults.yanked),
        }
    }
}

/// Available query string options for the keyring API
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyOptions {
    #[serde(default)]
    #[serde(deserialize_with = "parse_role_list")]
    pub roles: Vec<SignatureRole>,
}

struct RoleVisitor(std::marker::PhantomData<fn() -> Vec<SignatureRole>>);

impl<'de> serde::de::Visitor<'de> for RoleVisitor {
    type Value = Vec<SignatureRole>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a comma delimited list of SignatureRoles")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let roles = v
            .split(',')
            .map(|s| {
                s.parse::<SignatureRole>()
                    .map_err(|e| serde::de::Error::custom(e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(roles)
    }
}

fn parse_role_list<'de, D>(deserializer: D) -> Result<Vec<SignatureRole>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let visitor = RoleVisitor(std::marker::PhantomData);
    deserializer.deserialize_str(visitor)
}

// Keeping these types private for now until we stabilize exactly how we want to handle it

#[derive(Deserialize, Serialize, Debug)]
pub(crate) struct LoginParams {
    pub provider: String,
}

/// Adds extra fields onto the device authorization response so we can pass back the client id that
/// should be used
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct DeviceAuthorizationExtraFields {
    pub client_id: String,
    pub token_url: String,
}

#[cfg(any(feature = "client", feature = "server"))]
impl oauth2::devicecode::ExtraDeviceAuthorizationFields for DeviceAuthorizationExtraFields {}

//! Common API types and utilities

use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};

mod string_or_number {
    use serde::{Deserialize, Deserializer, de};

    pub fn deserialize_u32_opt<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrNum {
            Num(u32),
            Str(String),
        }

        match Option::<StringOrNum>::deserialize(deserializer)? {
            Some(StringOrNum::Num(n)) => Ok(Some(n)),
            Some(StringOrNum::Str(s)) => s.parse().map(Some).map_err(de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Standard API error response
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Pagination parameters (matches Java: page, size)
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct PaginationParams {
    #[serde(default, deserialize_with = "string_or_number::deserialize_u32_opt")]
    page: Option<u32>,
    #[serde(default, alias = "limit", deserialize_with = "string_or_number::deserialize_u32_opt")]
    size: Option<u32>,
}

impl PaginationParams {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(0)
    }

    pub fn size(&self) -> u32 {
        self.size.unwrap_or(20)
    }
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: Some(0),
            size: Some(20),
        }
    }
}

impl PaginationParams {
    pub fn offset(&self) -> u64 {
        (self.page() as u64) * (self.size() as u64)
    }

    pub fn limit(&self) -> i64 {
        self.size() as i64
    }
}

/// Paginated response wrapper
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    pub total_pages: u32,
}

impl<T> PaginatedResponse<T> {
    pub fn new(data: Vec<T>, page: u32, size: u32, total: u64) -> Self {
        let total_pages = ((total as f64) / (size as f64)).ceil() as u32;
        Self {
            data,
            page,
            size,
            total,
            total_pages,
        }
    }
}

/// Success response with optional message
#[derive(Debug, Serialize, ToSchema)]
pub struct SuccessResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuccessResponse {
    pub fn ok() -> Self {
        Self {
            success: true,
            message: None,
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
        }
    }
}

/// Created response with ID
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedResponse {
    pub id: String,
}

impl CreatedResponse {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

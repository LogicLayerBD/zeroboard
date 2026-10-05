pub mod jwt;
pub mod middleware;
pub mod rate_limit;
pub mod service;

pub use middleware::{require_auth, AuthUser};

/// bcrypt work factor for passwords and refresh tokens (security rules require ≥ 12).
pub const BCRYPT_COST: u32 = 12;

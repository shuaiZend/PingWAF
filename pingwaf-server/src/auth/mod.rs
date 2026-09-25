//! Authentication and authorisation primitives: JWT minting/verification,
//! bcrypt password hashing and the Axum extractors that enforce both.

pub mod jwt;
pub mod middleware;
pub mod password;

pub use jwt::{
    AGENT_ROLE, Claims, ISSUER, JwtError, TOKEN_TYPE_AGENT, TOKEN_TYPE_DASHBOARD,
    TOKEN_TYPE_REFRESH, create_agent_token, create_refresh_token, create_token, verify_agent_token,
    verify_refresh_token, verify_token, verify_user_token,
};
pub use middleware::{AdminUser, AuthError, AuthUser, bearer_token};
pub use password::{
    MAX_PASSWORD_BYTES, MIN_PASSWORD_LENGTH, PasswordError, hash_password, hash_password_with_cost,
    validate_password, verify_password,
};

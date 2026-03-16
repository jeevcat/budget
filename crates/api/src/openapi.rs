// utoipa 5.4.0's `#[derive(OpenApi)]` macro expands to code that uses `.for_each()`
// instead of a `for` loop — nothing we can fix on our side.
#![allow(clippy::needless_for_each)]

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    info(title = "Budget API", version = "0.1.0"),
    modifiers(&SecurityAddon),
    tags(
        (name = "accounts"),
        (name = "transactions"),
        (name = "categories"),
        (name = "rules"),
        (name = "budgets"),
        (name = "jobs"),
        (name = "connections"),
        (name = "import"),
        (name = "amazon"),
        (name = "auth"),
    ),
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_token",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("token")
                        .build(),
                ),
            );
        }
    }
}

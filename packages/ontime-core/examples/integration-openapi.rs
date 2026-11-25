//! Prints the integration API's OpenAPI document, generated from the catalogue:
//!
//!     cargo run -q -p ontime-core --example integration-openapi > .docs/integration_api.openapi.json

fn main() {
    let document = ontime_core::integration::catalog::openapi();
    println!(
        "{}",
        serde_json::to_string_pretty(&document).expect("a JSON value serialises")
    );
}

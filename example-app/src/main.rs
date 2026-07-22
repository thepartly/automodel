#[allow(dead_code)]
mod generated;
mod models;

use std::env;

fn main() {
    let database_url = env::var("AUTOMODEL_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:password@automodel-testdb:5432/postgres".to_string()
    });

    println!("example-app compiled successfully");
    println!("All generated modules are valid.");
    println!();
    println!("Run tests with: cargo test -p example-app");
    println!("Database URL: {}", database_url);
}

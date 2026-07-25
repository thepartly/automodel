use example_app_sharded::{build_router, generated};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("AUTOMODEL_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost:55432/postgres".to_string());

    let router = build_router(&database_url, 4).await?;

    let user_id = uuid::Uuid::new_v4();
    let account =
        generated::accounts::insert_account(&router, user_id, "Ada".to_string(), 100).await?;
    println!("inserted: {:?}", account);

    let fetched = generated::accounts::get_account(&router, user_id).await?;
    println!("fetched: {:?}", fetched);

    Ok(())
}

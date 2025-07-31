use argus::state::{SqliteStateRepository, StateRepository};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().expect(".env file not found");

    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let repo = SqliteStateRepository::new(&db_url).await?;
    repo.run_migrations().await?;

    let last_block = repo.get_last_processed_block("mainnet").await?;
    println!("Last processed block for mainnet: {last_block:?}");

    Ok(())
}

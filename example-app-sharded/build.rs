#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = automodel::AutoModelConfig::from_file("automodel.yml")?;

    automodel::AutoModel::generate(
        || {
            if std::env::var("CI").is_err() {
                std::env::var("AUTOMODEL_DATABASE_URL").map_err(|_| {
                    "AUTOMODEL_DATABASE_URL environment variable must be set for code generation"
                        .to_string()
                })
            } else {
                Err(
                    "Detecting not up to date AutoModel generated code in CI environment"
                        .to_string(),
                )
            }
        },
        &config.queries_dir,
        &config.output_dir,
        config.defaults(),
    )
    .await
}

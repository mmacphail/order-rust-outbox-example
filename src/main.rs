use dotenvy::dotenv;
use order_service::{build_server, create_pool, run_migrations};
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .expect("PORT must be a valid number");

    let pool = create_pool(&database_url);
    run_migrations(&pool);

    log::info!("Starting server at http://{}:{}", host, port);

    build_server(pool, &host, port)?.await
}

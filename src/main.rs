use actix_web::{middleware::Logger, web, App, HttpServer};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use dotenvy::dotenv;
use std::env;

mod db;
mod errors;
mod handlers;
mod models;
mod schema;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

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

    let pool = db::create_pool(&database_url);

    // Run pending migrations on startup
    {
        let mut conn = pool.get().expect("Failed to get DB connection for migrations");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run database migrations");
    }

    log::info!("Starting server at http://{}:{}", host, port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(Logger::default())
            .service(
                web::scope("/orders")
                    .route("", web::post().to(handlers::orders::create_order))
                    .route("/{id}", web::get().to(handlers::orders::get_order)),
            )
    })
    .bind((host, port))?
    .run()
    .await
}

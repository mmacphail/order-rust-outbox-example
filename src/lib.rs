pub mod db;
pub mod errors;
pub mod handlers;
pub mod models;
pub mod schema;

use actix_web::{middleware::Logger, web, App, HttpServer};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub use db::{create_pool, DbPool};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::orders::create_order,
        handlers::orders::get_order,
    ),
    components(schemas(
        handlers::orders::CreateOrderRequest,
        handlers::orders::CreateOrderLineRequest,
        handlers::orders::CreateOrderResponse,
        handlers::orders::OrderResponse,
        handlers::orders::OrderLineResponse,
    )),
    tags(
        (name = "orders", description = "Order management endpoints")
    ),
    info(
        title = "Order Service API",
        version = "0.1.0",
        description = "REST API for the order service with transactional outbox pattern"
    )
)]
pub struct ApiDoc;

/// Run any pending Diesel migrations against the pool's database.
pub fn run_migrations(pool: &DbPool) {
    let mut conn = pool
        .get()
        .expect("Failed to get DB connection for migrations");
    conn.run_pending_migrations(MIGRATIONS)
        .expect("Failed to run database migrations");
}

/// Build and return an actix-web `Server` bound to `host:port`.
///
/// The caller is responsible for `.await`-ing (or `tokio::spawn`-ing) the
/// returned server.
pub fn build_server(
    pool: DbPool,
    host: &str,
    port: u16,
) -> std::io::Result<actix_web::dev::Server> {
    let openapi = ApiDoc::openapi();
    Ok(HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(Logger::default())
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-docs/openapi.json", openapi.clone()),
            )
            .service(
                web::scope("/orders")
                    .route("", web::post().to(handlers::orders::create_order))
                    .route("/{id}", web::get().to(handlers::orders::get_order)),
            )
    })
    .bind((host.to_string(), port))?
    .run())
}

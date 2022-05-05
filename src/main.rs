#![warn(rust_2018_idioms)]

use opentelemetry::global::shutdown_tracer_provider;
use opentelemetry::sdk::Resource;
use opentelemetry::trace::TraceError;
use opentelemetry::{global, sdk::trace as sdktrace};
use opentelemetry::{trace::Tracer};
use opentelemetry_otlp::WithExportConfig;
use std::error::Error;

use hyper::{body::Body, Method, Request, Response, Server, StatusCode};

use hyper::service::{make_service_fn, service_fn};

use opentelemetry_http::HeaderExtractor;
use std::collections::HashMap;
use url::form_urlencoded;

static INDEX: &[u8] = b"<html><body><form action=\"post\" method=\"post\">Name: <input type=\"text\" name=\"name\"><br>Number: <input type=\"text\" name=\"number\"><br><input type=\"submit\"></body></html>";
static MISSING: &[u8] = b"Missing field";
static NOTNUMERIC: &[u8] = b"Number field is not numeric";

fn fibonacci(n: u8) -> u64 {
    match n {
        0 => 1,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

// Using service_fn, we can turn this function into a `Service`.
async fn handle(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let tracer = global::tracer("global_tracer");

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") | (&Method::GET, "/post") => Ok(Response::new(INDEX.into())),
        (&Method::POST, "/post") => {
            // Extract the incoming context
            let parent_cx = global::get_text_map_propagator(|propagator| {
                propagator.extract(&HeaderExtractor(req.headers()))
            });
            tracer.start_with_context("fibonacci", &parent_cx);

            // Concatenate the body...
            let b = hyper::body::to_bytes(req).await?;
            // Parse the request body. form_urlencoded::parse
            // always succeeds, but in general parsing may
            // fail (for example, an invalid post of json), so
            // returning early with BadRequest may be
            // necessary.
            //
            // Warning: this is a simplified use case. In
            // principle names can appear multiple times in a
            // form, and the values should be rolled up into a
            // HashMap<String, Vec<String>>. However in this
            // example the simpler approach is sufficient.
            let params = form_urlencoded::parse(b.as_ref())
                .into_owned()
                .collect::<HashMap<String, String>>();

            // Validate the request parameters, returning
            // early if an invalid input is detected.
            let name = if let Some(n) = params.get("name") {
                n
            } else {
                return Ok(Response::builder()
                    .status(StatusCode::UNPROCESSABLE_ENTITY)
                    .body(MISSING.into())
                    .unwrap());
            };
            let number = if let Some(n) = params.get("number") {
                if let Ok(v) = n.parse::<u8>() {
                    v
                } else {
                    return Ok(Response::builder()
                        .status(StatusCode::UNPROCESSABLE_ENTITY)
                        .body(NOTNUMERIC.into())
                        .unwrap());
                }
            } else {
                return Ok(Response::builder()
                    .status(StatusCode::UNPROCESSABLE_ENTITY)
                    .body(MISSING.into())
                    .unwrap());
            };

            let nth_fib = fibonacci(number);

            // Render the response. This will often involve
            // calls to a database or web service, which will
            // require creating a new stream for the response
            // body. Since those may fail, other error
            // responses such as InternalServiceError may be
            // needed here, too.

            let body = format!(
                "Hello {}, {}th fibonacci number is {}",
                name, number, nth_fib
            );
            Ok(Response::new(body.into()))
        }
        (&Method::GET, "/get") => {
            let query = if let Some(q) = req.uri().query() {
                q
            } else {
                return Ok(Response::builder()
                    .status(StatusCode::UNPROCESSABLE_ENTITY)
                    .body(MISSING.into())
                    .unwrap());
            };
            let params = form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect::<HashMap<String, String>>();
            let page = if let Some(p) = params.get("page") {
                p
            } else {
                return Ok(Response::builder()
                    .status(StatusCode::UNPROCESSABLE_ENTITY)
                    .body(MISSING.into())
                    .unwrap());
            };
            let body = format!("You requested {}", page);
            Ok(Response::new(body.into()))
        }
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()),
    }
}

fn init_tracer() -> Result<sdktrace::Tracer, TraceError> {
    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_env())
        .with_trace_config(
            sdktrace::config().with_resource(Resource::default()),
        )
        .install_batch(opentelemetry::runtime::Tokio)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let _ = init_tracer()?;

    let addr = ([127, 0, 0, 1], 1337).into();

    let server = Server::bind(&addr).serve(make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(handle))
    }));

    println!("Listening on {}", addr);
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }

    shutdown_tracer_provider();

    Ok(())
}

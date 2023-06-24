mod json;
mod trace;

use opentelemetry::{
    global,
    sdk::{self, resource::Resource, trace::Tracer},
    trace::TracerProvider,
    KeyValue,
};
use opentelemetry_otlp::{SpanExporterBuilder, WithExportConfig};
use opentelemetry_semantic_conventions::resource;
use tracing::{info, info_span};
use tracing_subscriber::{fmt, prelude::*};

use crate::trace::{remote_trace_span, RemoteTraceContext, TraceInfo};

#[tokio::main]
async fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(otel_tracer()))
            .with(fmt::layer().json().event_format(json::Json)),
    )
    .unwrap();

    let remote_trace_context = RemoteTraceContext {
        info: TraceInfo {
            trace_id: "9d96f6d506048d33796d850a09797e55".into(),
            span_id: "0db1818f6e5514ee".into(),
        },
        trace_flags: 0,
    };

    let span = remote_trace_span(info_span!("main one"), &remote_trace_context);
    let current_span = span.enter();

    info!("main one");
    nested();
    nested_async().await;

    drop(current_span);

    global::shutdown_tracer_provider();
}

#[tracing::instrument]
fn nested() {
    info!("nested function");
}

#[tracing::instrument]
async fn nested_async() {
    info!("nested async");
}

fn otel_tracer() -> Tracer {
    let exporter = opentelemetry_otlp::new_exporter()
        .http()
        .with_endpoint("http://localhost:4318/v1/traces");

    let span_exporter = SpanExporterBuilder::from(exporter)
        .build_span_exporter()
        .unwrap();

    let batch_processor =
        sdk::trace::BatchSpanProcessor::builder(span_exporter, opentelemetry::runtime::Tokio)
            .build();

    let trace_config = sdk::trace::config().with_resource(Resource::new(vec![KeyValue::new(
        resource::SERVICE_NAME,
        "tracing-stable-trace-id-example",
    )]));

    let provider = sdk::trace::TracerProvider::builder()
        .with_span_processor(batch_processor)
        .with_config(trace_config)
        .build();

    let tracer = provider.tracer("opentelemetry-otlp");

    global::set_tracer_provider(provider);
    tracer
}

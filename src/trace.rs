use opentelemetry::trace::{SpanId, SpanRef, TraceContextExt, TraceFlags, TraceId};
use serde::{Deserialize, Serialize};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTraceContext {
    #[serde(flatten)]
    pub info: TraceInfo,
    pub trace_flags: u8,
}

// Set parent context and return reference
pub fn remote_trace_span(span: Span, trace_context: &RemoteTraceContext) -> Span {
    span.set_parent(opentelemetry::Context::new().with_remote_span_context(
        opentelemetry::trace::SpanContext::new(
            TraceId::from_hex(&trace_context.info.trace_id).unwrap(),
            SpanId::from_hex(&trace_context.info.span_id).unwrap(),
            TraceFlags::new(trace_context.trace_flags),
            true,
            Default::default(),
        ),
    ));
    span
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfo {
    pub trace_id: String,
    pub span_id: String,
}

pub(crate) fn trace_info_from_ref(span_ref: SpanRef<'_>) -> Option<TraceInfo> {
    let span_context = span_ref.span_context();
    let trace_id = span_context.trace_id();

    if trace_id == TraceId::INVALID {
        return None;
    }

    Some(TraceInfo {
        trace_id: trace_id.to_string(),
        span_id: span_context.span_id().to_string(),
    })
}

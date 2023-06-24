use std::{io, marker::PhantomData};

use chrono::Utc;
use opentelemetry::trace::TraceContextExt;
use serde::ser::{Serialize, SerializeMap, Serializer};
use tracing::{Event, Subscriber};
use tracing_opentelemetry::OtelData;
use tracing_serde::{fields::AsMap, AsSerde};
use tracing_subscriber::{
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields, FormattedFields},
    registry::{LookupSpan, SpanRef},
};

use crate::trace::trace_info_from_ref;

// https://github.com/tokio-rs/tracing/blob/4e65750b13721fee7a7ac05b053e1b9c3d21244f/tracing-subscriber/src/fmt/format/json.rs
pub struct Json;

impl<S, N> FormatEvent<S, N> for Json
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let meta = event.metadata();

        let mut visit = || {
            let mut serializer = serde_json::Serializer::new(WriteAdaptor::new(&mut writer));
            let mut serializer = serializer.serialize_map(None)?;
            serializer.serialize_entry("timestamp", &Utc::now().to_rfc3339())?;
            serializer.serialize_entry("level", &meta.level().as_serde())?;
            serializer.serialize_entry("fields", &event.field_map())?;
            serializer.serialize_entry("target", meta.target())?;

            let format_field_marker: PhantomData<N> = PhantomData;

            if let Some(span_ref) = ctx.lookup_current() {
                serializer
                    .serialize_entry("span", &SerializableSpan(&span_ref, format_field_marker))
                    .unwrap_or(());

                let trace_info = span_ref.extensions().get::<OtelData>().and_then(|o| {
                    trace_info_from_ref(o.parent_cx.span()).map(|mut info| {
                        // if the SpanBuilder contains a valid span_id we use its span_id instead
                        // of the extracted one, because it refers to the more accurate span.
                        if let Some(span_id) = o.builder.span_id {
                            info.span_id = span_id.to_string();
                        }
                        info
                    })
                });

                if let Some(trace_info) = trace_info {
                    serializer.serialize_entry("span_id", &trace_info.span_id)?;
                    serializer.serialize_entry("trace_id", &trace_info.trace_id)?;
                }
            }

            serializer.end()
        };

        visit().map_err(|_| std::fmt::Error)?;
        writeln!(writer)
    }
}

pub struct WriteAdaptor<'a> {
    fmt_write: &'a mut dyn std::fmt::Write,
}

impl<'a> WriteAdaptor<'a> {
    pub fn new(fmt_write: &'a mut dyn std::fmt::Write) -> Self {
        Self { fmt_write }
    }
}

impl<'a> io::Write for WriteAdaptor<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s =
            std::str::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        self.fmt_write
            .write_str(s)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(s.as_bytes().len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// https://github.com/tokio-rs/tracing/blob/4e65750b13721fee7a7ac05b053e1b9c3d21244f/tracing-subscriber/src/fmt/format/json.rs#L110
struct SerializableSpan<'a, 'b, Span, N>(&'b SpanRef<'a, Span>, PhantomData<N>)
where
    Span: for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static;

impl<'a, 'b, Span, N> Serialize for SerializableSpan<'a, 'b, Span, N>
where
    Span: for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
    where
        Ser: serde::ser::Serializer,
    {
        let mut serializer = serializer.serialize_map(None)?;

        let ext = self.0.extensions();
        let data = ext
            .get::<FormattedFields<N>>()
            .expect("Unable to find FormattedFields in extensions; this is a bug");

        // TODO: let's _not_ do this, but this resolves
        // https://github.com/tokio-rs/tracing/issues/391.
        // We should probably rework this to use a `serde_json::Value` or something
        // similar in a JSON-specific layer, but I'd (david)
        // rather have a uglier fix now rather than shipping broken JSON.
        match serde_json::from_str::<serde_json::Value>(data) {
            Ok(serde_json::Value::Object(fields)) => {
                for field in fields {
                    serializer.serialize_entry(&field.0, &field.1)?;
                }
            }
            // We have fields for this span which are valid JSON but not an object.
            // This is probably a bug, so panic if we're in debug mode
            Ok(_) if cfg!(debug_assertions) => panic!(
                "span '{}' had malformed fields! this is a bug.\n  error: invalid JSON object\n  fields: {:?}",
                self.0.metadata().name(),
                data
            ),
            // If we *aren't* in debug mode, it's probably best not to
            // crash the program, let's log the field found but also an
            // message saying it's type  is invalid
            Ok(value) => {
                serializer.serialize_entry("field", &value)?;
                serializer.serialize_entry("field_error", "field was no a valid object")?
            }
            // We have previously recorded fields for this span
            // should be valid JSON. However, they appear to *not*
            // be valid JSON. This is almost certainly a bug, so
            // panic if we're in debug mode
            Err(e) if cfg!(debug_assertions) => panic!(
                "span '{}' had malformed fields! this is a bug.\n  error: {}\n  fields: {:?}",
                self.0.metadata().name(),
                e,
                data
            ),
            // If we *aren't* in debug mode, it's probably best not
            // crash the program, but let's at least make sure it's clear
            // that the fields are not supposed to be missing.
            Err(e) => serializer.serialize_entry("field_error", &format!("{e}"))?,
        };
        serializer.serialize_entry("name", self.0.metadata().name())?;
        serializer.end()
    }
}

use std::{fmt, io};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    EnvFilter,
    fmt::{FmtContext, FormatEvent, FormatFields, format},
    registry::LookupSpan,
};

struct CompactFormat;

impl<S, N> FormatEvent<S, N> for CompactFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: format::Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let is_error = *event.metadata().level() == Level::ERROR;
        format()
            .compact()
            .with_target(false)
            .with_file(is_error)
            .with_line_number(is_error)
            .format_event(ctx, writer, event)
    }
}

pub fn init() {
    tracing_subscriber::fmt()
        .with_ansi(true)
        .event_format(CompactFormat)
        .with_writer(io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    std::panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        tracing::error!(location = %info.location().map(|location| location.to_string()).unwrap_or_default(), "panic: {message}");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn compact_logs_are_colored_and_only_errors_have_source_locations() {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(true)
            .event_format(CompactFormat)
            .with_max_level(Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::trace!("trace message");
            tracing::debug!("debug message");
            tracing::info!(services = 3, "info message");
            tracing::warn!("warning message");
            tracing::error!("error message");
        });

        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), 5);
        for (line, color) in lines.iter().zip([35, 34, 32, 33, 31]) {
            assert!(line.contains(&format!("\x1b[{color}m")), "{line:?}");
        }
        for line in &lines[..4] {
            assert!(!line.contains(file!()), "{line:?}");
        }
        assert!(lines[2].contains("info message"));
        assert!(lines[2].contains("services"));
        assert!(lines[4].contains(file!()));
    }
}

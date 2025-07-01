use std::fmt;

#[derive(Debug)]
pub enum EngineError {
    /// Error during engine initialization
    InitializationError(String),

    /// Spider-related errors
    SpiderError { spider_name: String, error: String },

    /// Scheduler-related errors
    SchedulerError(String),

    /// Pipeline-related errors
    PipelineError {
        pipeline_name: String,
        error: String,
    },

    /// Downloader/HTTP-related errors
    DownloaderError(String),

    /// Configuration errors
    ConfigurationError(String),

    /// Thread/concurrency errors
    ThreadError(String),

    /// Channel communication errors
    ChannelError(String),

    /// Timeout errors
    TimeoutError {
        operation: String,
        timeout: std::time::Duration,
    },

    /// Engine shutdown errors
    ShutdownError(String),

    /// IO errors (file operations, etc.)
    IoError(std::io::Error),

    /// Network/HTTP errors
    HttpError(reqwest::Error),

    /// Serialization/deserialization errors
    SerializationError(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::InitializationError(msg) => {
                write!(f, "Engine initialization failed: {}", msg)
            }

            EngineError::SpiderError { spider_name, error } => {
                write!(f, "Spider '{}' error: {}", spider_name, error)
            }

            EngineError::SchedulerError(msg) => write!(f, "Scheduler error: {}", msg),

            EngineError::PipelineError {
                pipeline_name,
                error,
            } => write!(f, "Pipeline '{}' error: {}", pipeline_name, error),

            EngineError::DownloaderError(msg) => write!(f, "Downloader error: {}", msg),

            EngineError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),

            EngineError::ThreadError(msg) => write!(f, "Thread error: {}", msg),

            EngineError::ChannelError(msg) => write!(f, "Channel communication error: {}", msg),

            EngineError::TimeoutError { operation, timeout } => {
                write!(f, "Operation '{}' timed out after {:?}", operation, timeout)
            }

            EngineError::ShutdownError(msg) => write!(f, "Engine shutdown error: {}", msg),

            EngineError::IoError(err) => write!(f, "IO error: {}", err),

            EngineError::HttpError(err) => write!(f, "HTTP error: {}", err),

            EngineError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::IoError(err) => Some(err),
            EngineError::HttpError(err) => Some(err),
            _ => None,
        }
    }
}

// Convenient From implementations for common error types
impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        EngineError::IoError(err)
    }
}

impl From<reqwest::Error> for EngineError {
    fn from(err: reqwest::Error) -> Self {
        EngineError::HttpError(err)
    }
}

// Helper methods for creating specific errors
impl EngineError {
    pub fn spider_error<S: Into<String>>(spider_name: S, error: S) -> Self {
        EngineError::SpiderError {
            spider_name: spider_name.into(),
            error: error.into(),
        }
    }

    pub fn pipeline_error<S: Into<String>>(pipeline_name: S, error: S) -> Self {
        EngineError::PipelineError {
            pipeline_name: pipeline_name.into(),
            error: error.into(),
        }
    }

    pub fn timeout_error<S: Into<String>>(operation: S, timeout: std::time::Duration) -> Self {
        EngineError::TimeoutError {
            operation: operation.into(),
            timeout,
        }
    }

    pub fn initialization_error<S: Into<String>>(msg: S) -> Self {
        EngineError::InitializationError(msg.into())
    }

    pub fn scheduler_error<S: Into<String>>(msg: S) -> Self {
        EngineError::SchedulerError(msg.into())
    }

    pub fn downloader_error<S: Into<String>>(msg: S) -> Self {
        EngineError::DownloaderError(msg.into())
    }

    pub fn configuration_error<S: Into<String>>(msg: S) -> Self {
        EngineError::ConfigurationError(msg.into())
    }

    pub fn thread_error<S: Into<String>>(msg: S) -> Self {
        EngineError::ThreadError(msg.into())
    }

    pub fn channel_error<S: Into<String>>(msg: S) -> Self {
        EngineError::ChannelError(msg.into())
    }

    pub fn shutdown_error<S: Into<String>>(msg: S) -> Self {
        EngineError::ShutdownError(msg.into())
    }

    pub fn serialization_error<S: Into<String>>(msg: S) -> Self {
        EngineError::SerializationError(msg.into())
    }
}

// Result type alias for convenience
pub type EngineResult<T> = Result<T, EngineError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn test_error_display() {
        let error = EngineError::spider_error("test_spider", "parsing failed");
        assert_eq!(
            error.to_string(),
            "Spider 'test_spider' error: parsing failed"
        );

        let timeout_error =
            EngineError::timeout_error("download", std::time::Duration::from_secs(30));
        assert_eq!(
            timeout_error.to_string(),
            "Operation 'download' timed out after 30s"
        );
    }

    #[test]
    fn test_error_conversion() {
        let io_error = Error::new(ErrorKind::NotFound, "file not found");
        let engine_error: EngineError = io_error.into();

        match engine_error {
            EngineError::IoError(_) => (),
            _ => panic!("Expected IoError"),
        }
    }

    #[test]
    fn test_helper_methods() {
        let error = EngineError::initialization_error("failed to start");
        match error {
            EngineError::InitializationError(msg) => assert_eq!(msg, "failed to start"),
            _ => panic!("Expected InitializationError"),
        }
    }
}

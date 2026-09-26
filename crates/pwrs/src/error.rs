//! Cmdlet errors as the engine wants them: an `ErrorRecord` with a
//! category, a stable id, an optional target, and a
//! terminating/non-terminating flag. Non-terminating is the default.

use crate::PsObject;

/// `System.Management.Automation.ErrorCategory`, numeric values as
/// defined by the engine.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCategory {
    NotSpecified = 0,
    OpenError = 1,
    CloseError = 2,
    DeviceError = 3,
    DeadlockDetected = 4,
    InvalidArgument = 5,
    InvalidData = 6,
    InvalidOperation = 7,
    InvalidResult = 8,
    InvalidType = 9,
    MetadataError = 10,
    NotImplemented = 11,
    NotInstalled = 12,
    ObjectNotFound = 13,
    OperationStopped = 14,
    OperationTimeout = 15,
    SyntaxError = 16,
    ParserError = 17,
    PermissionDenied = 18,
    ResourceBusy = 19,
    ResourceExists = 20,
    ResourceUnavailable = 21,
    ReadError = 22,
    WriteError = 23,
    FromStdErr = 24,
    SecurityError = 25,
    ProtocolError = 26,
    ConnectionError = 27,
    AuthenticationError = 28,
    LimitsExceeded = 29,
    QuotaExceeded = 30,
    NotEnabled = 31,
}

impl ErrorCategory {
    /// The category with this `System.Management.Automation.ErrorCategory`
    /// value, or `None` for a value outside the enum.
    pub fn from_code(code: u32) -> Option<ErrorCategory> {
        Some(match code {
            0 => ErrorCategory::NotSpecified,
            1 => ErrorCategory::OpenError,
            2 => ErrorCategory::CloseError,
            3 => ErrorCategory::DeviceError,
            4 => ErrorCategory::DeadlockDetected,
            5 => ErrorCategory::InvalidArgument,
            6 => ErrorCategory::InvalidData,
            7 => ErrorCategory::InvalidOperation,
            8 => ErrorCategory::InvalidResult,
            9 => ErrorCategory::InvalidType,
            10 => ErrorCategory::MetadataError,
            11 => ErrorCategory::NotImplemented,
            12 => ErrorCategory::NotInstalled,
            13 => ErrorCategory::ObjectNotFound,
            14 => ErrorCategory::OperationStopped,
            15 => ErrorCategory::OperationTimeout,
            16 => ErrorCategory::SyntaxError,
            17 => ErrorCategory::ParserError,
            18 => ErrorCategory::PermissionDenied,
            19 => ErrorCategory::ResourceBusy,
            20 => ErrorCategory::ResourceExists,
            21 => ErrorCategory::ResourceUnavailable,
            22 => ErrorCategory::ReadError,
            23 => ErrorCategory::WriteError,
            24 => ErrorCategory::FromStdErr,
            25 => ErrorCategory::SecurityError,
            26 => ErrorCategory::ProtocolError,
            27 => ErrorCategory::ConnectionError,
            28 => ErrorCategory::AuthenticationError,
            29 => ErrorCategory::LimitsExceeded,
            30 => ErrorCategory::QuotaExceeded,
            31 => ErrorCategory::NotEnabled,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub struct PsError {
    pub message: String,
    pub error_id: String,
    pub category: ErrorCategory,
    pub target: Option<PsObject>,
    pub terminating: bool,
}

pub type PsResult<T> = Result<T, PsError>;

impl PsError {
    pub fn new(category: ErrorCategory, error_id: impl Into<String>, message: impl Into<String>) -> Self {
        PsError { message: message.into(), error_id: error_id.into(), category, target: None, terminating: false }
    }

    /// Raise through `ThrowTerminatingError` instead of `WriteError`.
    pub fn terminating(mut self) -> Self {
        self.terminating = true;
        self
    }

    pub fn with_target(mut self, target: PsObject) -> Self {
        self.target = Some(target);
        self
    }
}

impl From<std::io::Error> for PsError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        let category = match e.kind() {
            K::NotFound => ErrorCategory::ObjectNotFound,
            K::PermissionDenied => ErrorCategory::PermissionDenied,
            K::AlreadyExists => ErrorCategory::ResourceExists,
            K::TimedOut => ErrorCategory::OperationTimeout,
            K::InvalidInput | K::InvalidData => ErrorCategory::InvalidData,
            K::ConnectionRefused | K::ConnectionReset | K::ConnectionAborted | K::NotConnected => ErrorCategory::ConnectionError,
            _ => ErrorCategory::NotSpecified,
        };
        PsError::new(category, "IoError", e.to_string())
    }
}

/// A reservation the allocator could not meet, as the error record
/// `PsMemory::zeroed` raises for the same failure. This is what makes
/// `buf.try_reserve(n)?` in a cmdlet body end in an error record: the
/// infallible allocations (`Vec::with_capacity`, `vec![0; n]`, a push
/// that grows) abort the host process when the allocator refuses them,
/// and no boundary can catch an abort.
impl From<std::collections::TryReserveError> for PsError {
    fn from(e: std::collections::TryReserveError) -> Self {
        PsError::new(ErrorCategory::ResourceUnavailable, "PwrsOutOfMemory", format!("cannot reserve the memory asked for: {e}"))
    }
}

impl core::fmt::Display for PsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{}] {}", self.error_id, self.message)
    }
}

impl std::error::Error for PsError {}

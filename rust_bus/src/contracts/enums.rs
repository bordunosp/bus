#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Timestamp {
    InsertedAt,
    ScheduledAt,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Infinite,
    Duration {
        field: Timestamp,
        duration: std::time::Duration,
    },
}

pub enum ScheduleIn {
    Immediately,
    Duration(std::time::Duration),
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Available,
    Scheduled,
    Executing,
    Retryable,
    Completed,
    Cancelled,
    Discarded,
}

impl State {
    pub fn to_str(&self) -> &'static str {
        match self {
            State::Available => "available",
            State::Scheduled => "scheduled",
            State::Executing => "executing",
            State::Retryable => "retryable",
            State::Completed => "completed",
            State::Cancelled => "cancelled",
            State::Discarded => "discarded",
        }
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Event,
    Queue,
    Priority,
    MaxAttempts,
    ExecutionTimeout,
    Tags,
    Meta,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailableField {
    Event,
    Queue,
    Priority,
    MaxAttempts,
    ExecutionTimeout,
    Tags,
    Meta,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledField {
    Event,
    Queue,
    Priority,
    MaxAttempts,
    ExecutionTimeout,
    Tags,
    Meta,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutingField {
    MaxAttempts,
    Tags,
    Meta,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryableField {
    Event,
    Priority,
    MaxAttempts,
    ExecutionTimeout,
    Tags,
    Meta,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalField {
    Tags,
    Meta,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Replace {
    Available(&'static [AvailableField]),
    Scheduled(&'static [ScheduledField]),
    Executing(&'static [ExecutingField]),
    Retryable(&'static [RetryableField]),
    Completed(&'static [TerminalField]),
    Cancelled(&'static [TerminalField]),
    Discarded(&'static [TerminalField]),
}

#[allow(dead_code)]
pub(crate) trait ToColumn {
    fn to_column(&self) -> &'static str;
}

#[allow(dead_code)]
impl ToColumn for AvailableField {
    fn to_column(&self) -> &'static str {
        match self {
            AvailableField::Event => "payload",
            AvailableField::Queue => "queue",
            AvailableField::Priority => "priority",
            AvailableField::MaxAttempts => "max_attempts",
            AvailableField::ExecutionTimeout => "execution_timeout_sec",
            AvailableField::Tags => "tags",
            AvailableField::Meta => "meta",
        }
    }
}

impl ToColumn for ScheduledField {
    fn to_column(&self) -> &'static str {
        match self {
            ScheduledField::Event => "payload",
            ScheduledField::Queue => "queue",
            ScheduledField::Priority => "priority",
            ScheduledField::MaxAttempts => "max_attempts",
            ScheduledField::ExecutionTimeout => "execution_timeout_sec",
            ScheduledField::Tags => "tags",
            ScheduledField::Meta => "meta",
        }
    }
}

impl ToColumn for ExecutingField {
    fn to_column(&self) -> &'static str {
        match self {
            ExecutingField::MaxAttempts => "max_attempts",
            ExecutingField::Tags => "tags",
            ExecutingField::Meta => "meta",
        }
    }
}

impl ToColumn for RetryableField {
    fn to_column(&self) -> &'static str {
        match self {
            RetryableField::Event => "payload",
            RetryableField::Priority => "priority",
            RetryableField::MaxAttempts => "max_attempts",
            RetryableField::ExecutionTimeout => "execution_timeout_sec",
            RetryableField::Tags => "tags",
            RetryableField::Meta => "meta",
        }
    }
}

impl ToColumn for TerminalField {
    fn to_column(&self) -> &'static str {
        match self {
            TerminalField::Tags => "tags",
            TerminalField::Meta => "meta",
        }
    }
}

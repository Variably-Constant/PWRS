//! A progress record with every field `Write-Progress` sets.

/// A progress record for [`crate::Pipeline::write_progress`] and
/// [`crate::Worker::write_progress`], carrying every field a
/// `ProgressRecord` has.
///
/// The engine refuses a record whose activity or status is empty, whose
/// activity id is negative, or whose parent activity id is its own
/// activity id, and the write returns its error.
///
/// ```ignore
/// ps.write_progress(
///     &Progress::new(2, "Copying", "3 of 40 files")
///         .with_parent(1)
///         .with_current_operation("notes.txt")
///         .with_percent(7)
///         .with_seconds_remaining(95),
/// )?;
/// ps.write_progress(&Progress::new(2, "Copying", "done").completed())?;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// The activity the record updates: records with one id drive one
    /// progress bar.
    pub activity_id: i32,
    /// The activity this one is nested under; negative for none.
    pub parent_activity_id: i32,
    /// What is being done, shown as the bar's title.
    pub activity: String,
    /// Where it stands, shown under the title.
    pub status: String,
    /// The step under way, shown below the bar; empty for none.
    pub current_operation: String,
    /// How far along, 0 to 100. Negative shows no percentage, and a
    /// value above 100 is written as 100.
    pub percent_complete: i32,
    /// An estimate of the seconds left; negative shows none.
    pub seconds_remaining: i32,
    /// True for the record that ends the activity and removes its bar.
    /// Otherwise the record is processing, whatever its percentage.
    pub completed: bool,
}

impl Progress {
    /// A processing record for `activity_id` with no parent, no current
    /// operation, no percentage and no estimate of the time left.
    pub fn new(activity_id: i32, activity: impl Into<String>, status: impl Into<String>) -> Self {
        Progress {
            activity_id,
            parent_activity_id: -1,
            activity: activity.into(),
            status: status.into(),
            current_operation: String::new(),
            percent_complete: -1,
            seconds_remaining: -1,
            completed: false,
        }
    }

    /// Nests the activity under the activity `parent`.
    pub fn with_parent(mut self, parent: i32) -> Self {
        self.parent_activity_id = parent;
        self
    }

    /// Names the step under way.
    pub fn with_current_operation(mut self, operation: impl Into<String>) -> Self {
        self.current_operation = operation.into();
        self
    }

    /// Sets how far along the activity is, 0 to 100.
    pub fn with_percent(mut self, percent: i32) -> Self {
        self.percent_complete = percent;
        self
    }

    /// Sets the estimate of the seconds left.
    pub fn with_seconds_remaining(mut self, seconds: i32) -> Self {
        self.seconds_remaining = seconds;
        self
    }

    /// Makes this the record that ends the activity.
    pub fn completed(mut self) -> Self {
        self.completed = true;
        self
    }
}

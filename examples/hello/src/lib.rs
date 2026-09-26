//! `Get-Greeting` plus one cmdlet per output mode.

use pwrs::prelude::*;

/// Writes a greeting for each name.
///
/// Greets once by default; `-Count` repeats the greeting.
///
/// # Examples
/// Get-Greeting -Name World
/// 'Ada', 'Bob' | Get-Greeting
/// Get-Greeting -Name World -Count 3
#[cmdlet(verb = "Get", noun = "Greeting", output = ["System.String"])]
#[derive(Default)]
pub struct GetGreeting {
    /// Who to greet.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub name: String,
    /// How many times to greet.
    #[param(validate_range(1, 1000000000))]
    pub count: Option<i64>,
    /// Fail with a non-terminating error instead of greeting.
    #[param]
    pub fail: bool,
    /// Panic instead of greeting.
    #[param]
    pub panic: bool,
}

impl Cmdlet for GetGreeting {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.panic {
            panic!("requested panic for {}", self.name);
        }
        if self.fail {
            return Err(PsError::new(ErrorCategory::InvalidData, "GreetingRefused", format!("refusing to greet {}", self.name)));
        }
        pwrs::verbose!(ps, "greeting {}", self.name)?;
        for _ in 0..self.count.unwrap_or(1) {
            if ps.stopping() {
                break;
            }
            ps.write(format!("Hello, {}!", self.name))?;
        }
        Ok(())
    }
}

/// A person record, copied into a CLR object on output.
#[psclass(name = "Hello.Person")]
#[derive(Default, Clone)]
pub struct Person {
    /// Display name.
    pub name: String,
    /// Age in years.
    pub age: i64,
    /// Free-form tags.
    pub tags: Vec<String>,
    /// Score, when known.
    pub score: Option<f64>,
    /// Whether the record is active.
    pub active: bool,
}

/// Builds a person record.
#[cmdlet(verb = "Get", noun = "Person", output = ["Hello.Person"])]
#[derive(Default)]
pub struct GetPerson {
    #[param(mandatory, position = 0)]
    pub name: String,
    #[param]
    pub age: Option<i64>,
    #[param]
    pub score: Option<f64>,
    #[param]
    pub tag: Vec<String>,
}

impl Cmdlet for GetPerson {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Person {
            name: self.name.clone(),
            age: self.age.unwrap_or(0),
            tags: std::mem::take(&mut self.tag),
            score: self.score,
            active: true,
        })
    }
}

/// A counter whose value lives in Rust until the object is disposed.
#[psclass(name = "Hello.Counter", mode = proxy)]
#[derive(Default, Clone)]
pub struct Counter {
    /// Label of the counter.
    pub label: String,
    /// Current value.
    pub value: i64,
    /// Every value `Advance` produced, in order.
    pub history: Vec<i64>,
}

/// Methods a script calls on a `Hello.Counter` object.
#[psmethods]
impl Counter {
    /// Adds `by` and returns the new value.
    pub fn advance(&mut self, by: i64) -> PsResult<i64> {
        self.value = match self.value.checked_add(by) {
            Some(v) => v,
            None => return Err(PsError::new(ErrorCategory::InvalidOperation, "CounterOverflow", "the counter would overflow")),
        };
        self.history.push(self.value);
        Ok(self.value)
    }

    /// `prefix` (the label when absent), `=`, and the value.
    pub fn describe(&self, prefix: Option<String>) -> PsResult<String> {
        let prefix = match prefix {
            Some(p) => p,
            None => self.label.clone(),
        };
        Ok(format!("{prefix}={}", self.value))
    }

    /// Sets the value to zero and clears the history.
    pub fn reset(&mut self) -> PsResult<()> {
        self.value = 0;
        self.history.clear();
        Ok(())
    }

    /// Moves half of the value into a new counter labeled `label`,
    /// which is returned as its own proxy object.
    pub fn split(&mut self, label: String) -> PsResult<Counter> {
        let half = self.value / 2;
        self.value -= half;
        Ok(Counter { label, value: half, history: Vec::new() })
    }

    /// A counter labeled `label` starting at `start`. A script reaches
    /// this as `[Hello.Counter]::new(label, start)`. A label holding `=`
    /// is refused, since `Parse` could not read `Describe`'s text of such
    /// a counter back.
    pub fn new(label: String, start: i64) -> PsResult<Counter> {
        if label.contains('=') {
            return Err(PsError::new(ErrorCategory::InvalidArgument, "CounterLabel", format!("a counter's label cannot hold '=': {label}")));
        }
        Ok(Counter { label, value: start, history: Vec::new() })
    }

    /// A counter from `label=value` text, the form `Describe` writes.
    pub fn parse(text: String) -> PsResult<Counter> {
        let (label, value) = text
            .split_once('=')
            .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "CounterParse", format!("expected label=value, got {text}")))?;
        let value = value
            .trim()
            .parse::<i64>()
            .map_err(|e| PsError::new(ErrorCategory::InvalidArgument, "CounterParse", format!("{value} is not a number: {e}")))?;
        Ok(Counter { label: label.trim().to_string(), value, history: Vec::new() })
    }

    /// The largest value a counter holds.
    pub fn limit() -> PsResult<i64> {
        Ok(i64::MAX)
    }

    /// Adds another counter's value, read back through its properties.
    pub fn absorb(&mut self, other: Counter) -> PsResult<i64> {
        self.advance(other.value)
    }

    /// Whether another counter holds the same value, read back through
    /// its properties; the other counter may be this one.
    pub fn same_as(&self, other: Counter) -> PsResult<bool> {
        Ok(self.value == other.value)
    }
}

/// A stretch of a timeline: where it starts and how long it runs.
///
/// Copied, so every property read is a plain field of a CLR object and
/// nothing crosses into Rust. A script still makes one without a
/// cmdlet, through the constructor below, and cannot make one of CLR
/// zeros by mistake: declaring `new` takes the parameterless
/// constructor C# would otherwise supply.
#[psclass(name = "Hello.Stretch")]
#[derive(Clone)]
pub struct Stretch {
    /// Where it starts.
    pub start: i64,
    /// How long it runs; never negative.
    pub length: i64,
}

/// Not zeros, so a constructor starting from here is told apart from
/// one filling CLR zeros.
impl Default for Stretch {
    fn default() -> Self {
        Stretch { start: 0, length: 60 }
    }
}

/// Statics of `Hello.Stretch`. A copied class has no Rust value behind
/// its object, so everything declared here runs on the type.
#[psmethods]
impl Stretch {
    /// Makes a stretch. Each argument is optional and a left-out one
    /// takes its default, so `[Hello.Stretch]::new()` starts from
    /// `Default` and one `new` answers every arity. A negative length is
    /// refused, as an exception, since a constructor has no stream to
    /// warn on.
    pub fn new(start: Option<i64>, length: Option<i64>) -> PsResult<Self> {
        let mut made = Stretch::default();
        if let Some(start) = start {
            made.start = start;
        }
        if let Some(length) = length {
            if length < 0 {
                return Err(PsError::new(ErrorCategory::InvalidArgument, "HelloStretchLength", format!("a stretch cannot run {length}")));
            }
            made.length = length;
        }
        Ok(made)
    }

    /// Reads `start+length`, the form the stretch is written in.
    pub fn parse(text: String) -> PsResult<Self> {
        let (start, length) = text
            .split_once('+')
            .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "HelloStretchText", format!("{text} is not start+length")))?;
        let number = |part: &str| {
            part.trim()
                .parse::<i64>()
                .map_err(|e| PsError::new(ErrorCategory::InvalidArgument, "HelloStretchText", format!("{part} is not a number: {e}")))
        };
        Stretch::new(Some(number(start)?), Some(number(length)?))
    }
}

/// A proxy whose tick count lives in Rust only: `Ticks` is not a
/// property and the object is not read back by value.
#[psclass(name = "Hello.Ticker", mode = proxy)]
#[derive(Default, Clone)]
pub struct Ticker {
    /// Label of the ticker.
    pub label: String,
    /// How wide the ticker counts, as a narrow integer.
    pub width: u32,
    /// The step, as a single-precision number.
    pub step: f32,
    /// A bound, when there is one.
    pub limit: Option<u16>,
    #[psfield(skip)]
    ticks: u64,
}

/// Methods a script calls on a `Hello.Ticker` object.
#[psmethods]
impl Ticker {
    /// Counts one tick and returns the count.
    pub fn tick(&mut self) -> PsResult<u64> {
        self.ticks += 1;
        Ok(self.ticks)
    }

    /// Adds every byte of `data` to the count and returns it.
    pub fn feed(&mut self, data: Vec<u8>) -> PsResult<u64> {
        self.ticks += data.iter().map(|&b| b as u64).sum::<u64>();
        Ok(self.ticks)
    }

    /// The width, as the narrow integer it is.
    pub fn narrow(&self) -> PsResult<u32> {
        Ok(self.width)
    }

    /// The step, as the single-precision number it is.
    pub fn ratio(&self) -> PsResult<f32> {
        Ok(self.step)
    }

    /// The bound, when there is one.
    pub fn bound(&self) -> PsResult<Option<u16>> {
        Ok(self.limit)
    }
}

/// Creates a ticker.
#[cmdlet(verb = "New", noun = "RustTicker", output = ["Hello.Ticker"], alias = ["nrtick"])]
#[derive(Default)]
pub struct NewRustTicker {
    #[param(mandatory, position = 0)]
    pub label: String,
    #[param]
    pub width: Option<u32>,
    #[param]
    pub limit: Option<u16>,
}

impl Cmdlet for NewRustTicker {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Ticker { label: self.label.clone(), width: self.width.unwrap_or(7), step: 0.5, limit: self.limit, ticks: 0 })
    }
}

/// Repeats `Text` until it is at least `Width` characters long, then
/// trims the result to exactly that many.
///
/// The synopsis above is one sentence written across two source
/// lines, and this paragraph is the description.
#[cmdlet(verb = "Expand", noun = "RustText", output = ["System.String"])]
#[derive(Default)]
pub struct ExpandRustText {
    #[param(mandatory, position = 0)]
    pub text: String,
    #[param(mandatory, validate_range(1, 4096))]
    pub width: i64,
}

impl Cmdlet for ExpandRustText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.text.is_empty() {
            return Err(PsError::new(ErrorCategory::InvalidArgument, "EmptyText", "the text is empty"));
        }
        let mut out = String::new();
        while out.chars().count() < self.width as usize {
            out.push_str(&self.text);
        }
        ps.write(out.chars().take(self.width as usize).collect::<String>())
    }
}

/// Streams values produced on a worker thread, reporting progress.
///
/// # Examples
/// Get-RustStream -Count 5
#[cmdlet(verb = "Get", noun = "RustStream", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustStream {
    /// How many values to produce.
    #[param(mandatory, position = 0, validate_range(1, 100000))]
    pub count: i64,
}

impl Cmdlet for GetRustStream {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let n = self.count;
        ps.progress(1, "Streaming", &format!("{n} values"), 0)?;
        ps.stream_from_thread(move |tx| {
            for i in 1..=n {
                if tx.send(i).is_err() {
                    break;
                }
            }
        })?;
        // A negative percent completes the progress record.
        ps.progress(1, "Streaming", "done", -1)
    }
}

/// Squares each of `1..Count` on a worker pool and writes the results.
///
/// Without `-AsReady` the results come back in input order; with it,
/// each is written as its worker finishes.
///
/// # Examples
/// Get-RustParallel -Count 8
/// Get-RustParallel -Count 8 -AsReady
#[cmdlet(verb = "Get", noun = "RustParallel", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustParallel {
    /// How many values to square.
    #[param(mandatory, position = 0, validate_range(1, 100000))]
    pub count: i64,
    /// Write each result as it finishes rather than in input order.
    #[param]
    pub as_ready: bool,
}

impl Cmdlet for GetRustParallel {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let order = if self.as_ready { Order::AsReady } else { Order::Input };
        ps.par_map((1..=self.count).collect::<Vec<i64>>(), order, |n| n * n)
    }
}

/// Writes a table script can read both ways and write neither.
///
/// # Examples
/// $t = Get-RustReadOnlyTable; $t.alpha; $t['alpha']
#[cmdlet(verb = "Get", noun = "RustReadOnlyTable")]
#[derive(Default)]
pub struct GetRustReadOnlyTable {
    /// Wrap an ordered source instead, to show the order survives.
    #[param]
    pub ordered: bool,
}

impl Cmdlet for GetRustReadOnlyTable {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.ordered {
            let src = PsType::from_name("System.Collections.Specialized.OrderedDictionary").new(&[])?;
            for (key, value) in [("z", 1i64), ("a", 2), ("m", 3)] {
                src.call("set_Item", &[key.into_ps()?, value.into_ps()?])?;
            }
            return ps.write(PsReadOnlyTable::over(&src)?);
        }
        let nested = PsHashtable::new()?;
        nested.set("n", 0i64.into_ps()?)?;
        let source = PsHashtable::new()?;
        source.set("alpha", 1i64.into_ps()?)?;
        source.set("beta", "two".into_ps()?)?;
        source.set("inner", nested.0)?;
        ps.write(PsReadOnlyTable::over(&source.0)?)
    }
}

/// Adds `1..Count` on a worker pool and writes the total.
///
/// # Examples
/// Measure-RustParallel -Count 100
#[cmdlet(verb = "Measure", noun = "RustParallel", output = ["System.Int64"])]
#[derive(Default)]
pub struct MeasureRustParallel {
    /// How many values to add.
    #[param(mandatory, position = 0, validate_range(1, 100000))]
    pub count: i64,
}

impl Cmdlet for MeasureRustParallel {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        // The workers reach the total through an atomic rather than a
        // lock, and nothing is written from them: the sum is put on
        // the pipeline here, on the thread that may.
        let total = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
        let adding = std::sync::Arc::clone(&total);
        ps.par_for_each((1..=self.count).collect::<Vec<i64>>(), move |n| {
            adding.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        })?;
        ps.write(total.load(std::sync::atomic::Ordering::Relaxed))
    }
}

/// Writes one value of each CLR width, in the order sbyte, short,
/// int, byte, ushort, uint, float, long, double, so a caller can read
/// back the type the engine gave each one.
///
/// # Examples
/// Get-RustWidths | ForEach-Object { $_.GetType().Name }
#[cmdlet(verb = "Get", noun = "RustWidths")]
#[derive(Default)]
pub struct GetRustWidths;

impl Cmdlet for GetRustWidths {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(1i8)?;
        ps.write(2i16)?;
        ps.write(3i32)?;
        ps.write(4u8)?;
        ps.write(5u16)?;
        ps.write(6u32)?;
        ps.write(7.5f32)?;
        ps.write(8i64)?;
        ps.write(9.5f64)
    }
}

/// The tag PWRS gives one object's type, as a number, and
/// `PS_TYPE_OBJECT` (0) for a type outside the vocabulary.
///
/// # Examples
/// Get-RustTypeTag -InputObject ([int]1)
#[cmdlet(verb = "Get", noun = "RustTypeTag", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustTypeTag {
    /// The object whose type to name.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub input_object: PsObject,
}

impl Cmdlet for GetRustTypeTag {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(i64::from(self.input_object.type_tag()?))
    }
}

/// Reads a decimal into its four `Decimal.GetBits` words and builds a
/// new one from them, so a round trip through Rust is observable.
///
/// # Examples
/// Get-RustDecimalRoundTrip -Value 123.456
#[cmdlet(verb = "Get", noun = "RustDecimalRoundTrip", output = ["System.Decimal"])]
#[derive(Default)]
pub struct GetRustDecimalRoundTrip {
    /// The value to take apart and rebuild.
    #[param(mandatory, position = 0)]
    pub value: PsObject,
    /// Write the scale instead of the value.
    #[param]
    pub scale: bool,
}

impl Cmdlet for GetRustDecimalRoundTrip {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let d = PsDecimal::from_ps(&self.value)?;
        if self.scale {
            return ps.write(i64::from(d.scale()));
        }
        ps.write(d)
    }
}

/// Sums a `Decimal[]` by pinning it, which reads the whole array as
/// one block rather than one object at a time.
///
/// # Examples
/// Measure-RustDecimalBlock -Value ([decimal[]](1.5, 2.25))
#[cmdlet(verb = "Measure", noun = "RustDecimalBlock", output = ["System.Decimal"])]
#[derive(Default)]
pub struct MeasureRustDecimalBlock {
    /// The array to pin.
    #[param(mandatory, position = 0)]
    pub value: PsObject,
}

impl Cmdlet for MeasureRustDecimalBlock {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        // Every element must share one scale for the words to add,
        // which is what this refuses on rather than answering wrong.
        let block = self.value.pin::<PsDecimalBits>()?.to_vec();
        let mut total: i64 = 0;
        let mut scale = None;
        for bits in &block {
            let d = PsDecimal::from(*bits);
            if d.hi != 0 || d.mid != 0 {
                return Err(PsError::new(
                    ErrorCategory::InvalidArgument,
                    "HelloDecimalTooWide",
                    "this sums only values whose mantissa fits the low word",
                ));
            }
            match scale {
                None => scale = Some(d.scale()),
                Some(s) if s == d.scale() => {}
                Some(s) => {
                    return Err(PsError::new(
                        ErrorCategory::InvalidArgument,
                        "HelloDecimalScale",
                        format!("scale {} does not match {s}", d.scale()),
                    ))
                }
            }
            let magnitude = i64::from(d.lo);
            total += if d.is_negative() { -magnitude } else { magnitude };
        }
        let flags = i32::from(scale.unwrap_or(0)) << 16;
        let negative = total < 0;
        let magnitude = total.unsigned_abs();
        ps.write(PsDecimal::from_bits(
            magnitude as i32,
            (magnitude >> 32) as i32,
            0,
            if negative { flags | i32::MIN } else { flags },
        ))
    }
}

/// Round-trips a `DateTimeOffset`, or writes its offset in minutes.
///
/// # Examples
/// Get-RustOffset -Value ([datetimeoffset]::Now) -Minutes
#[cmdlet(verb = "Get", noun = "RustOffset")]
#[derive(Default)]
pub struct GetRustOffset {
    /// The value to read.
    #[param(mandatory, position = 0)]
    pub value: PsObject,
    /// Write the offset in whole minutes instead of the value.
    #[param]
    pub minutes: bool,
    /// Write the same instant as ticks on the UTC clock.
    #[param]
    pub utc_ticks: bool,
}

impl Cmdlet for GetRustOffset {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let v = PsDateTimeOffset::from_ps(&self.value)?;
        if self.minutes {
            return ps.write(i64::from(v.offset_minutes));
        }
        if self.utc_ticks {
            return ps.write(v.to_utc_ticks());
        }
        ps.write(v)
    }
}

/// Runs another command by name from Rust and writes what it wrote:
/// `Get-Greeting` from this module unless `-Command` names another,
/// with `-Name` bound as its `Name` parameter when given; or, with
/// `-Sort`, `Sort-Object` over those numbers piped in. No script block
/// is built for any of it.
///
/// # Examples
/// Get-RustComposed -Name Ada
/// Get-RustComposed -Command Get-Location
/// Get-RustComposed -Sort 3, 1, 2
#[cmdlet(verb = "Get", noun = "RustComposed")]
#[derive(Default)]
pub struct GetRustComposed {
    /// The command to run; `Get-Greeting` when absent.
    #[param]
    pub command: Option<String>,
    /// Bound as the command's `Name` parameter when given.
    #[param(position = 0)]
    pub name: Option<String>,
    /// Numbers piped into `Sort-Object` instead of running a command.
    #[param]
    pub sort: Option<Vec<i64>>,
}

impl Cmdlet for GetRustComposed {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let results = if let Some(numbers) = &self.sort {
            ps.invoke_with_input("Sort-Object", &[], Some(&numbers.clone().into_ps()?))?
        } else {
            let command = self.command.as_deref().unwrap_or("Get-Greeting");
            let mut parameters: Vec<(&str, PsObject)> = Vec::new();
            if let Some(name) = &self.name {
                parameters.push(("Name", name.clone().into_ps()?));
            }
            ps.invoke(command, &parameters)?
        };
        for result in &results {
            ps.write_object(result)?;
        }
        Ok(())
    }
}

/// Asks the person at the console through the host's own prompts and
/// writes what they answered: `Line` reads a line, `Secure` reads one
/// without echo and writes only its length, `Choice` offers
/// `-Choices` and writes the index chosen. A host that cannot prompt
/// raises the engine's own error.
///
/// # Examples
/// Read-RustHost Line
/// Read-RustHost Choice -Choices '&Yes', '&No'
#[cmdlet(verb = "Read", noun = "RustHost")]
#[derive(Default)]
pub struct ReadRustHost {
    /// `Line`, `Secure` or `Choice`.
    #[param(mandatory, position = 0)]
    pub kind: String,
    /// The labels offered for `Choice`; `&` marks the hot key.
    #[param]
    pub choices: Option<Vec<String>>,
    /// Written to the host's own output before asking, off the pipeline.
    #[param]
    pub say: Option<String>,
}

impl Cmdlet for ReadRustHost {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let ui = ps.host_ui()?;
        if let Some(text) = &self.say {
            ui.write_line(text)?;
        }
        match self.kind.as_str() {
            "Line" => ps.write(ui.read_line()?),
            "Secure" => ps.write(ui.read_line_as_secure_string()?.len()? as i64),
            "Choice" => {
                let labels = self.choices.clone().unwrap_or_default();
                let choices: Vec<(&str, &str)> = labels.iter().map(|l| (l.as_str(), "")).collect();
                ps.write(ui.prompt_for_choice("Choose", "Which one?", &choices, 0)? as i64)
            }
            other => Err(PsError::new(ErrorCategory::InvalidArgument, "HelloReadKind", format!("{other} is not Line, Secure or Choice"))),
        }
    }
}

/// Writes where it stands in its pipeline, as `position/length`, and
/// passes anything piped into it straight through, so a chain of them
/// shows every position in it. The place is read in `begin`, which is
/// where a command that must know its neighbors before any input
/// arrives reads it, and written at `end`.
///
/// # Examples
/// Get-RustInvocation
/// Get-RustInvocation | Get-RustInvocation | Get-RustInvocation
#[cmdlet(verb = "Get", noun = "RustInvocation", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustInvocation {
    /// Passed through untouched.
    #[param(value_from_pipeline)]
    pub input_object: Option<PsObject>,
    place: String,
}

impl Cmdlet for GetRustInvocation {
    fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let invocation = ps.invocation()?;
        let position = i64::from_ps(&invocation.get("PipelinePosition")?)?;
        let length = i64::from_ps(&invocation.get("PipelineLength")?)?;
        self.place = format!("{position}/{length}");
        Ok(())
    }

    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if let Some(input) = &self.input_object {
            ps.write_object(input)?;
        }
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.place.clone())
    }
}

/// Reads an `ErrorRecord`'s parts from Rust and writes them as
/// `category|id|message|target`, with `target` the target object's
/// text, or empty when the record carries none.
///
/// # Examples
/// try { Get-Item nope -ErrorAction Stop } catch { $_ | Get-RustErrorInfo }
#[cmdlet(verb = "Get", noun = "RustErrorInfo", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustErrorInfo {
    /// The record to read.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub record: PsObject,
}

impl Cmdlet for GetRustErrorInfo {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let record = PsErrorRecord::from_ps(&self.record)?;
        let target = if record.target.is_null() { String::new() } else { String::from_ps(&record.target)? };
        ps.write(format!("{:?}|{}|{}|{}", record.category, record.error_id, record.message, target))
    }
}

static IMPORTS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static REMOVES: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Counts the import. Runs on every import of the module, including
/// one that follows a removal in the same session.
#[on_import]
fn count_import() -> PsResult<()> {
    IMPORTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Counts the removal. A removal unloads nothing and runs no
/// destructor, so this is where a module holding resources releases
/// them.
#[on_remove]
fn count_remove() -> PsResult<()> {
    REMOVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// How many times this library's import hook has run, or with
/// `-Removes` its removal hook. The counts live in the library, which
/// a removal does not unload, so they carry across a remove and the
/// import that follows it.
///
/// # Examples
/// Get-RustLifecycle
/// Get-RustLifecycle -Removes
#[cmdlet(verb = "Get", noun = "RustLifecycle", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustLifecycle {
    /// Write the removal count instead of the import count.
    #[param]
    pub removes: bool,
}

impl Cmdlet for GetRustLifecycle {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let counter = if self.removes { &REMOVES } else { &IMPORTS };
        ps.write(counter.load(std::sync::atomic::Ordering::Relaxed))
    }
}

/// Asks every object its type, `Passes` times over, by tag or by
/// name, and writes how many asks that was. The loop does nothing
/// else, so the call's wall time over that number is what one ask
/// costs by that route.
///
/// # Examples
/// Measure-RustTypeReads -InputObject $items -Passes 100
/// Measure-RustTypeReads -InputObject $items -Passes 100 -ByName
#[cmdlet(verb = "Measure", noun = "RustTypeReads", output = ["System.Int64"])]
#[derive(Default)]
pub struct MeasureRustTypeReads {
    /// The objects to ask.
    #[param(mandatory, position = 0)]
    pub input_object: Vec<PsObject>,
    /// How many times to walk the whole collection.
    #[param(mandatory, position = 1, validate_range(1, 100000))]
    pub passes: i64,
    /// Ask through `type_name` rather than `type_tag`.
    #[param]
    pub by_name: bool,
}

impl Cmdlet for MeasureRustTypeReads {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        // black_box keeps each ask from being optimised away. The
        // branch is on a value that does not change inside the loop.
        let mut asks: i64 = 0;
        for _ in 0..self.passes {
            for object in &self.input_object {
                if self.by_name {
                    std::hint::black_box(object.type_name()?);
                } else {
                    std::hint::black_box(object.type_tag()?);
                }
                asks += 1;
            }
        }
        ps.write(asks)
    }
}

/// Reads one property from every object, `Passes` times over, and
/// writes how many reads that was. The loop does nothing else, so the
/// call's wall time over that number is what one property read costs
/// when Rust takes it.
///
/// # Examples
/// Measure-RustPropertyReads -InputObject $items -Name Name -Passes 100
#[cmdlet(verb = "Measure", noun = "RustPropertyReads", output = ["System.Int64"])]
#[derive(Default)]
pub struct MeasureRustPropertyReads {
    /// The objects to read from.
    #[param(mandatory, position = 0)]
    pub input_object: Vec<PsObject>,
    /// The property to read from each object.
    #[param(mandatory, position = 1)]
    pub name: String,
    /// How many times to walk the whole collection.
    #[param(mandatory, position = 2, validate_range(1, 100000))]
    pub passes: i64,
}

impl Cmdlet for MeasureRustPropertyReads {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        // black_box keeps the read from being optimised away, and the
        // handle it returns is dropped where it is taken: that drop is
        // part of what a read costs and stays inside the timing.
        let mut reads: i64 = 0;
        for _ in 0..self.passes {
            for object in &self.input_object {
                std::hint::black_box(object.get(&self.name)?);
                reads += 1;
            }
        }
        ps.write(reads)
    }
}

/// Reads one property off a `PSObject` by name and writes it, note
/// properties included. A name the object does not carry is an error
/// rather than a null.
///
/// # Examples
/// [pscustomobject]@{ Name = 'x' } | Get-RustProperty -Name Name
#[cmdlet(verb = "Get", noun = "RustProperty")]
#[derive(Default)]
pub struct GetRustProperty {
    /// The object to read from.
    #[param(mandatory, value_from_pipeline)]
    pub input_object: PsObject,
    /// The property to read.
    #[param(mandatory, position = 0)]
    pub name: String,
}

impl Cmdlet for GetRustProperty {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write_object(&pwrs::object::property(&self.input_object, &self.name)?)
    }
}

/// Writes one record to each of the four message streams, building
/// each message only when the engine would keep it.
///
/// # Examples
/// Write-RustStreams -Message hello -Verbose
#[cmdlet(verb = "Write", noun = "RustStreams")]
#[derive(Default)]
pub struct WriteRustStreams {
    /// The text each record carries.
    #[param(mandatory, position = 0)]
    pub message: String,
}

impl Cmdlet for WriteRustStreams {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        pwrs::verbose!(ps, "verbose: {}", self.message)?;
        pwrs::debug!(ps, "debug: {}", self.message)?;
        pwrs::warning!(ps, "warning: {}", self.message)?;
        pwrs::information!(ps, "information: {}", self.message)
    }
}

/// Refuses its argument with an error naming that object as the
/// target, and its display string in the message.
#[cmdlet(verb = "Test", noun = "RustTarget")]
#[derive(Default)]
pub struct TestRustTarget {
    /// The object the error will point at.
    #[param(mandatory, position = 0)]
    pub value: PsObject,
}

impl Cmdlet for TestRustTarget {
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        let shown = pwrs::types::display_string(&self.value)?;
        Err(PsError::new(ErrorCategory::InvalidData, "TargetedFailure", format!("refusing {shown}")).with_target(self.value.clone()))
    }
}

/// Pretends to remove a thing, under the engine's confirmation.
///
/// Writes what it removed when the engine allows the change, and
/// nothing at all under `-WhatIf`.
///
/// # Examples
/// Remove-RustThing -Name cache
/// Remove-RustThing -Name cache -WhatIf
#[cmdlet(verb = "Remove", noun = "RustThing", supports_should_process, confirm_impact = "Medium", output = ["System.String"])]
#[derive(Default)]
pub struct RemoveRustThing {
    /// What to remove.
    #[param(mandatory, position = 0)]
    pub name: String,
}

impl Cmdlet for RemoveRustThing {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if ps.should_process(&self.name, "Remove")? {
            ps.write(format!("removed {}", self.name))?;
        }
        Ok(())
    }
}

/// Reads a record by name or by id, showing the binding modifiers a
/// parameter can carry.
///
/// `-Name` and `-Id` are separate parameter sets; `ByName` is the
/// default. `-Tag` binds from a property of a piped object, and any
/// leftover arguments land in `-Rest`.
#[cmdlet(verb = "Get", noun = "RustRecord", default_parameter_set = "ByName", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustRecord {
    /// Lowercase name of the record.
    #[param(mandatory, position = 0, set = "ByName", validate_not_null_or_empty, validate_pattern = "^[a-z]+$")]
    pub name: String,
    /// Numeric id of the record.
    #[param(mandatory, set = "ById")]
    pub id: i64,
    /// Bound from a `Tag` property of a piped object.
    #[param(value_from_pipeline_by_property_name)]
    pub tag: Option<String>,
    /// Everything left over on the command line.
    #[param(value_from_remaining)]
    pub rest: Vec<String>,
    /// Hidden from completion and help.
    #[param(dont_show)]
    pub internal: bool,
}

impl Cmdlet for GetRustRecord {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let subject = if self.name.is_empty() { format!("id {}", self.id) } else { format!("name {}", self.name) };
        let tag = match &self.tag {
            Some(t) => t.clone(),
            None => "none".to_string(),
        };
        ps.write(format!("{subject} tag {tag} rest {} internal {}", self.rest.join(","), self.internal))
    }
}

/// Writes the module's own name; takes no parameters.
#[cmdlet(verb = "Get", noun = "RustModuleName", output = ["System.String"], alias = ["grmn"])]
#[derive(Default)]
pub struct GetRustModuleName;

impl Cmdlet for GetRustModuleName {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write("Hello")
    }
}

/// A slot map whose methods carry a collection's natural names, so
/// the generated class declares Get, Set, Call and Contains beside
/// the property getters that read its fields.
#[psclass(name = "Hello.Slots", mode = proxy)]
#[derive(Default, Clone)]
pub struct Slots {
    /// Where the slots came from.
    pub origin: String,
    /// How many slots there are.
    pub capacity: u64,
    #[psfield(skip)]
    values: Vec<i64>,
}

/// Methods a script calls on a `Hello.Slots` object.
#[psmethods]
impl Slots {
    /// The value in `index`.
    pub fn get(&self, index: u64) -> PsResult<i64> {
        match self.values.get(index as usize) {
            Some(v) => Ok(*v),
            None => Err(PsError::new(ErrorCategory::InvalidArgument, "NoSlot", format!("there is no slot {index}"))),
        }
    }

    /// Puts `value` in `index` and returns what was there.
    pub fn set(&mut self, index: u64, value: i64) -> PsResult<i64> {
        match self.values.get_mut(index as usize) {
            Some(slot) => Ok(std::mem::replace(slot, value)),
            None => Err(PsError::new(ErrorCategory::InvalidArgument, "NoSlot", format!("there is no slot {index}"))),
        }
    }

    /// True when any slot holds `value`.
    pub fn contains(&self, value: i64) -> PsResult<bool> {
        Ok(self.values.contains(&value))
    }

    /// Runs `name` over the slots: `sum` or `count`.
    pub fn call(&self, name: String) -> PsResult<i64> {
        match name.as_str() {
            "sum" => Ok(self.values.iter().sum()),
            "count" => Ok(self.values.len() as i64),
            other => Err(PsError::new(ErrorCategory::InvalidArgument, "NoSuchOp", format!("{other} is not sum or count"))),
        }
    }
}

/// Creates a slot map with `Capacity` zeroed slots.
#[cmdlet(verb = "New", noun = "RustSlots", output = ["Hello.Slots"])]
#[derive(Default)]
pub struct NewRustSlots {
    #[param(mandatory, position = 0)]
    pub origin: String,
    #[param(mandatory, validate_range(0, 4096))]
    pub capacity: u64,
}

impl Cmdlet for NewRustSlots {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Slots { origin: self.origin.clone(), capacity: self.capacity, values: vec![0; self.capacity as usize] })
    }
}

/// Creates a proxy-backed counter.
#[cmdlet(verb = "New", noun = "Counter", output = ["Hello.Counter"])]
#[derive(Default)]
pub struct NewCounter {
    #[param(mandatory, position = 0)]
    pub label: String,
    #[param]
    pub value: Option<i64>,
}

impl Cmdlet for NewCounter {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Counter { label: self.label.clone(), value: self.value.unwrap_or(0), history: Vec::new() })
    }
}

/// A note emitted as a PSObject with note properties.
#[psclass(name = "Hello.Note", mode = psobject)]
#[derive(Default, Clone)]
pub struct Note {
    pub text: String,
    pub priority: i32,
}

/// Emits a note.
#[cmdlet(verb = "Get", noun = "Note")]
#[derive(Default)]
pub struct GetNote {
    #[param(mandatory, position = 0)]
    pub text: String,
    #[param]
    pub priority: Option<i32>,
}

impl Cmdlet for GetNote {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Note { text: self.text.clone(), priority: self.priority.unwrap_or(3) })
    }
}

/// Invokes a script block with arguments and writes its output.
#[cmdlet(verb = "Invoke", noun = "RustBlock")]
#[derive(Default)]
pub struct InvokeRustBlock {
    #[param(mandatory, position = 0)]
    pub script: PsScriptBlock,
    #[param]
    pub arg: Vec<PsObject>,
}

impl Cmdlet for InvokeRustBlock {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for out in self.script.call(ps, &self.arg)? {
            ps.write_object(&out)?;
        }
        Ok(())
    }
}

/// Summary of a hashtable.
#[psclass(name = "Hello.TableInfo")]
#[derive(Default, Clone)]
pub struct TableInfo {
    pub count: i64,
    pub keys: Vec<String>,
}

/// Reports the size and keys of a hashtable.
#[cmdlet(verb = "Get", noun = "RustTableInfo", output = ["Hello.TableInfo"])]
#[derive(Default)]
pub struct GetRustTableInfo {
    #[param(mandatory, position = 0)]
    pub table: PsHashtable,
}

impl Cmdlet for GetRustTableInfo {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(TableInfo { count: self.table.len()? as i64, keys: self.table.keys()? })
    }
}

/// Reads one key of a table through `PsHashtable::contains` and
/// `PsHashtable::get`, from the table as it was passed: a Hashtable, an
/// ordered dictionary or a generic dictionary, unconverted. Writes
/// `held:` or `absent:` and the value, `null` for none.
///
/// # Examples
/// Get-RustTableEntry -Table @{ a = 'one' } -Key a
#[cmdlet(verb = "Get", noun = "RustTableEntry", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustTableEntry {
    #[param(mandatory, position = 0)]
    pub table: PsObject,
    #[param(mandatory, position = 1)]
    pub key: String,
}

impl Cmdlet for GetRustTableEntry {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let table = PsHashtable::from_ps(&self.table)?;
        let held = if table.contains(&self.key)? { "held" } else { "absent" };
        let value = table.get(&self.key)?;
        let shown = if value.is_null() { "null".to_string() } else { String::from_ps(&value)? };
        ps.write(format!("{held}:{shown}"))
    }
}

/// Builds a hashtable from `key=value` pairs.
#[cmdlet(verb = "New", noun = "RustTable")]
#[derive(Default)]
pub struct NewRustTable {
    #[param(mandatory, position = 0)]
    pub pairs: Vec<String>,
}

impl Cmdlet for NewRustTable {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let mut map = std::collections::HashMap::new();
        for pair in &self.pairs {
            match pair.split_once('=') {
                Some((k, v)) => {
                    map.insert(k.to_string(), v.to_string());
                }
                None => {
                    return Err(PsError::new(ErrorCategory::InvalidArgument, "BadPair", format!("{pair} is not key=value")));
                }
            }
        }
        ps.write(map)
    }
}

/// Sums a byte array through a pinned borrow.
#[cmdlet(verb = "Get", noun = "RustChecksum")]
#[derive(Default)]
pub struct GetRustChecksum {
    #[param(mandatory, position = 0)]
    pub bytes: PsObject,
}

impl Cmdlet for GetRustChecksum {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let pinned = self.bytes.pin::<u8>()?;
        let sum: i64 = pinned.iter().map(|&b| b as i64).sum();
        ps.write(sum)
    }
}

/// Writes a byte array filled from Rust through one pin.
#[cmdlet(verb = "Get", noun = "RustBytes")]
#[derive(Default)]
pub struct GetRustBytes {
    #[param(mandatory, position = 0)]
    pub count: i64,
}

impl Cmdlet for GetRustBytes {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let data: Vec<u8> = (0..self.count).map(|i| i as u8).collect();
        ps.write_object(&PsObject::from_slice(&data)?)
    }
}

/// Measures input the way a compressor takes it: bytes piped whole to
/// `-InputObject`, or files piped by their `PSPath` to `-LiteralPath`.
///
/// `-InputObject` is a `PsObject` declared `byte[]` with `clr`, so the
/// binder hands a byte array over as it is and leaves a piped file to
/// `-LiteralPath`, and the bytes are read through a pin rather than
/// copied. `-Invert` flips every bit of them where they lie.
///
/// # Examples
/// , [byte[]](1, 2, 3) | Measure-RustInput
/// Get-Item notes.txt | Measure-RustInput
#[cmdlet(verb = "Measure", noun = "RustInput", default_parameter_set = "Bytes", output = ["System.String"])]
#[derive(Default)]
pub struct MeasureRustInput {
    /// The bytes to measure; an empty array is measured too.
    #[param(mandatory, position = 0, set = "Bytes", value_from_pipeline, clr = "byte[]", allow_empty_collection)]
    pub input_object: PsObject,
    /// A file, named exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: String,
    /// Flip every bit of the input's bytes in place.
    #[param(set = "Bytes")]
    pub invert: bool,
}

impl Cmdlet for MeasureRustInput {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if ps.parameter_is_bound("LiteralPath") {
            return match std::path::Path::new(&self.literal_path).file_name() {
                Some(leaf) => ps.write(format!("file {}", leaf.to_string_lossy())),
                None => Err(PsError::new(ErrorCategory::InvalidArgument, "NoFileName", format!("{} names no file", self.literal_path))),
            };
        }
        let mut pinned = self.input_object.pin::<u8>()?;
        let sum: u64 = pinned.iter().map(|&b| u64::from(b)).sum();
        if self.invert {
            for b in pinned.iter_mut() {
                *b = !*b;
            }
        }
        ps.write(format!("bytes {} sum {sum}", pinned.len()))
    }
}

/// Doubles a BigInteger by way of its two's-complement bytes.
#[cmdlet(verb = "Test", noun = "RustBigInt")]
#[derive(Default)]
pub struct TestRustBigInt {
    #[param(mandatory, position = 0)]
    pub value: PsObject,
}

impl Cmdlet for TestRustBigInt {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let big = PsBigInt::from_ps(&self.value)?;
        ps.write(big.doubled())
    }
}

/// Exercises the dynamic .NET surface: an instance method, a static
/// method, and a constructor by type name.
#[cmdlet(verb = "Test", noun = "RustDynamic")]
#[derive(Default)]
pub struct TestRustDynamic {
    #[param(mandatory, position = 0)]
    pub text: String,
    #[param]
    pub uri: Option<String>,
}

impl Cmdlet for TestRustDynamic {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if let Some(uri) = &self.uri {
            let obj = PsType::from_name("System.Uri").new(&[uri.as_str().into_ps()?])?;
            return ps.write(String::from_ps(&obj.get("Host")?)?);
        }
        let upper = String::from_ps(&self.text.as_str().into_ps()?.call("ToUpper", &[])?)?;
        let len = -(self.text.chars().count() as i64);
        let abs = i64::from_ps(&PsType::from_name("System.Math").call_static("Abs", &[len.into_ps()?])?)?;
        ps.write(format!("{upper}:{abs}"))
    }
}

/// Resolves a PSPath through the session's providers. `-Path` expands
/// wildcards; `-LiteralPath` takes the string as written, answers to
/// `PSPath` and `LP`, and binds from a piped object's `PSPath`.
#[cmdlet(verb = "Resolve", noun = "RustPath", default_parameter_set = "Path")]
#[derive(Default)]
pub struct ResolveRustPath {
    /// A path whose wildcards the provider expands.
    #[param(mandatory, position = 0, set = "Path")]
    pub path: String,
    /// A path taken exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: String,
}

impl Cmdlet for ResolveRustPath {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let literal = ps.parameter_is_bound("LiteralPath");
        let path = if literal { &self.literal_path } else { &self.path };
        for p in ps.resolve_path(path, literal)? {
            ps.write(p)?;
        }
        Ok(())
    }
}

/// Says where one input would go, with a parameter that belongs to some
/// parameter sets and not others.
///
/// `-Path` and `-LiteralPath` each name a file and `-Text` carries the
/// input itself. `-Destination` belongs to the two file sets only, so the
/// binder refuses it beside `-Text`.
///
/// # Examples
/// Get-RustRoute notes.txt archive.txt
/// Get-RustRoute -Text hello
#[cmdlet(verb = "Get", noun = "RustRoute", default_parameter_set = "Path", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustRoute {
    /// A file, named with wildcards.
    #[param(mandatory, position = 0, set = "Path")]
    pub path: String,
    /// A file, named exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: String,
    /// The input itself.
    #[param(mandatory, set = "Text")]
    pub text: String,
    /// Where a file would go.
    #[param(position = 1, set = ["Path", "LiteralPath"])]
    pub destination: Option<String>,
}

impl Cmdlet for GetRustRoute {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let from = if ps.parameter_is_bound("Text") {
            format!("text {}", self.text)
        } else if ps.parameter_is_bound("LiteralPath") {
            format!("literal {}", self.literal_path)
        } else {
            format!("path {}", self.path)
        };
        ps.write(format!("{from} -> {}", self.destination.as_deref().unwrap_or("none")))
    }
}

/// Reports the CLR type name of any value.
#[cmdlet(verb = "Get", noun = "RustTypeName")]
#[derive(Default)]
pub struct GetRustTypeName {
    #[param(mandatory, position = 0)]
    pub value: PsObject,
}

impl Cmdlet for GetRustTypeName {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.value.type_name()?)
    }
}

/// Looks up a color by name, with completion over the known set.
#[cmdlet(verb = "Get", noun = "RustColor", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustColor {
    #[param(mandatory, position = 0)]
    pub name: String,
}

impl Cmdlet for GetRustColor {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("color:{}", self.name))
    }
}

const COLORS: &[&str] = &["red", "green", "blue", "crimson", "cornflowerblue"];

/// Completes `-Name` for Get-RustColor.
#[completer(cmdlet = "Get-RustColor", parameter = "Name")]
fn complete_color(ctx: &CompletionContext) -> PsResult<Vec<Completion>> {
    let prefix = ctx.word.to_lowercase();
    Ok(COLORS.iter().filter(|c| c.starts_with(&prefix)).map(|c| Completion::value(*c).with_tooltip(format!("the {c} color"))).collect())
}

/// Reads a value; the parameters it accepts depend on -Kind.
#[cmdlet(verb = "Get", noun = "RustReading")]
#[derive(Default)]
pub struct GetRustReading {
    #[param(mandatory, position = 0)]
    pub kind: String,
}

impl Cmdlet for GetRustReading {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let extra = if ps.parameter_is_bound("Unit") {
            String::from_ps(&ps.parameter("Unit")?)?
        } else {
            "none".to_string()
        };
        ps.write(format!("{}:{}", self.kind, extra))
    }
}

/// Adds a -Unit parameter only when -Kind is temperature.
#[dynamic_params(cmdlet = GetRustReading)]
fn reading_dynamic_params(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>> {
    let kind = if bound.contains("Kind")? { String::from_ps(&bound.get("Kind")?)? } else { String::new() };
    if kind == "temperature" {
        Ok(vec![DynamicParam::string("Unit").with_validate_set(["C".to_string(), "F".to_string()])])
    } else {
        Ok(Vec::new())
    }
}

/// Reads a value exactly as Get-RustReading does, with no dynamic
/// parameters: `benches/dynamic_params.ps1` times the two against each
/// other to price them.
#[cmdlet(verb = "Get", noun = "RustStaticReading")]
#[derive(Default)]
pub struct GetRustStaticReading {
    #[param(mandatory, position = 0)]
    pub kind: String,
}

impl Cmdlet for GetRustStaticReading {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let extra = if ps.parameter_is_bound("Unit") {
            String::from_ps(&ps.parameter("Unit")?)?
        } else {
            "none".to_string()
        };
        ps.write(format!("{}:{}", self.kind, extra))
    }
}

/// Reads a value exactly as Get-RustReading does, with a dynamic-parameter
/// hook that adds nothing and never reads what is bound:
/// `benches/dynamic_params.ps1` prices the hook's reads against
/// Get-RustReading and the rest of the pass against Get-RustStaticReading.
#[cmdlet(verb = "Get", noun = "RustBlindReading")]
#[derive(Default)]
pub struct GetRustBlindReading {
    #[param(mandatory, position = 0)]
    pub kind: String,
}

impl Cmdlet for GetRustBlindReading {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let extra = if ps.parameter_is_bound("Unit") {
            String::from_ps(&ps.parameter("Unit")?)?
        } else {
            "none".to_string()
        };
        ps.write(format!("{}:{}", self.kind, extra))
    }
}

/// Adds no parameter and reads nothing from what is bound.
#[dynamic_params(cmdlet = GetRustBlindReading)]
fn blind_reading_dynamic_params(_bound: &PsHashtable) -> PsResult<Vec<DynamicParam>> {
    Ok(Vec::new())
}

/// Sums the numbers piped in and writes the total once.
///
/// Begin announces itself on the verbose stream, Process accumulates,
/// End writes the total.
#[cmdlet(verb = "Measure", noun = "RustTotal", output = ["System.Int64"])]
#[derive(Default)]
pub struct MeasureRustTotal {
    /// A number to add.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub value: i64,
    total: i64,
}

impl Cmdlet for MeasureRustTotal {
    fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        self.total = 0;
        ps.verbose("begin")
    }

    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        self.total += self.value;
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.total)
    }
}

/// Turns `4KB`, `2MB`, `1GB` and plain numbers into a count of
/// bytes, before the binder coerces the argument to `-Size`'s own
/// `long`. A value it does not recognise is handed back untouched for
/// the binder to coerce or refuse, and a suffix on a number that is
/// not one is refused here, which the engine reports as a binding
/// failure naming the parameter.
#[transform(cmdlet = "Get-RustSize", parameter = "Size")]
fn as_bytes(value: &PsObject) -> PsResult<PsObject> {
    let text = String::from_ps(value)?;
    let trimmed = text.trim();
    let (digits, scale) = match trimmed.to_ascii_uppercase() {
        t if t.ends_with("KB") => (&trimmed[..trimmed.len() - 2], 1024_i64),
        t if t.ends_with("MB") => (&trimmed[..trimmed.len() - 2], 1024 * 1024),
        t if t.ends_with("GB") => (&trimmed[..trimmed.len() - 2], 1024 * 1024 * 1024),
        _no_suffix => return value.clone().into_ps(),
    };
    match digits.trim().parse::<i64>() {
        Ok(n) => n.checked_mul(scale).ok_or_else(|| size_error(trimmed, "is more than a long can hold"))?.into_ps(),
        Err(_not_a_number) => Err(size_error(trimmed, "is not a number followed by KB, MB or GB")),
    }
}

fn size_error(given: &str, why: &str) -> PsError {
    PsError::new(ErrorCategory::InvalidArgument, "HelloSize", format!("{given} {why}"))
}

/// Writes the size in bytes. `-Size` is a `long`, and the transform
/// is what lets a caller write `2MB` where the engine's own suffixes
/// are not available.
///
/// # Examples
/// Get-RustSize -Size 2MB
/// Get-RustSize -Size 512
#[cmdlet(verb = "Get", noun = "RustSize", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustSize {
    /// The size, as a number or with a KB, MB or GB suffix.
    #[param(mandatory, position = 0)]
    pub size: i64,
}

impl Cmdlet for GetRustSize {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.size)
    }
}

/// A traffic signal, declared to PowerShell as the enum `Hello.Signal`.
#[psenum(name = "Hello.Signal")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Signal {
    /// Stop.
    #[default]
    Red,
    /// Prepare to stop.
    Amber = 5,
    /// Go.
    Green,
}

/// Describes a signal; the engine binds the enum from its name.
#[cmdlet(verb = "Get", noun = "RustSignal", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustSignal {
    /// The current signal.
    #[param(mandatory, position = 0)]
    pub signal: Signal,
    /// The signal that follows, when known.
    #[param]
    pub next: Option<Signal>,
    /// Earlier signals.
    #[param]
    pub history: Vec<Signal>,
}

impl Cmdlet for GetRustSignal {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("{:?}:{:?}:{}", self.signal, self.next, self.history.len()))
    }
}

/// Writes the signal whose underlying value is given.
#[cmdlet(verb = "ConvertTo", noun = "RustSignal", output = ["Hello.Signal"])]
#[derive(Default)]
pub struct ConvertToRustSignal {
    /// The underlying value: 0, 5, or 6.
    #[param(mandatory, position = 0)]
    pub value: i64,
}

impl Cmdlet for ConvertToRustSignal {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let signal = match self.value {
            0 => Signal::Red,
            5 => Signal::Amber,
            6 => Signal::Green,
            other => return Err(PsError::new(ErrorCategory::InvalidArgument, "NoSuchSignal", format!("{other} is not a signal value"))),
        };
        ps.write(signal)
    }
}

/// `System.ConsoleColor`, which the runtime already declares, mapped
/// onto a Rust enum rather than mirrored as a second CLR type. Only
/// the four variants this module cares about are named: a value
/// outside them is an error rather than a silent default.
#[psenum(clr = "System.ConsoleColor")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ink {
    /// `ConsoleColor.Black`.
    #[default]
    Black = 0,
    /// `ConsoleColor.DarkBlue`.
    DarkBlue = 1,
    /// `ConsoleColor.Red`.
    Red = 12,
    /// `ConsoleColor.White`.
    White = 15,
}

/// Takes a `System.ConsoleColor` the binder validated and completed
/// from the CLR type itself, and writes the one that follows it,
/// again as a real `System.ConsoleColor`.
///
/// # Examples
/// Get-RustInk -Color Red
/// (Get-RustInk -Color Black).GetType().FullName
#[cmdlet(verb = "Get", noun = "RustInk", output = ["System.ConsoleColor"])]
#[derive(Default)]
pub struct GetRustInk {
    /// The colour to read.
    #[param(mandatory, position = 0)]
    pub color: Ink,
    /// Write the name and number instead of the value.
    #[param]
    pub describe: bool,
}

impl Cmdlet for GetRustInk {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.describe {
            return ps.write(format!("{:?}", self.color));
        }
        let next = match self.color {
            Ink::Black => Ink::DarkBlue,
            Ink::DarkBlue => Ink::Red,
            Ink::Red => Ink::White,
            Ink::White => Ink::Black,
        };
        ps.write(next)
    }
}

/// A light with an enum-typed field, copied into a CLR object.
#[psclass(name = "Hello.Light")]
#[derive(Default, Clone)]
pub struct Light {
    /// Where the light stands.
    pub name: String,
    /// What it shows.
    pub state: Signal,
    /// What it showed before, when known.
    pub previous: Option<Signal>,
}

/// Builds a light record.
#[cmdlet(verb = "New", noun = "RustLight", output = ["Hello.Light"])]
#[derive(Default)]
pub struct NewRustLight {
    #[param(mandatory, position = 0)]
    pub name: String,
    #[param(mandatory)]
    pub state: Signal,
    #[param]
    pub previous: Option<Signal>,
}

impl Cmdlet for NewRustLight {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Light { name: self.name.clone(), state: self.state, previous: self.previous })
    }
}

/// Echoes an unsigned 64-bit value and sums a list of them.
#[cmdlet(verb = "Get", noun = "RustUnsigned")]
#[derive(Default)]
pub struct GetRustUnsigned {
    /// Written back unchanged as a UInt64.
    #[param(mandatory, position = 0)]
    pub value: u64,
    /// Summed and written as a second UInt64.
    #[param]
    pub values: Vec<u64>,
}

impl Cmdlet for GetRustUnsigned {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.value)?;
        if !self.values.is_empty() {
            let total: u64 = self.values.iter().sum();
            ps.write(total)?;
        }
        Ok(())
    }
}

/// A stamped record: dates, a span, GUIDs, a character and signals,
/// copied into a CLR object.
#[psclass(name = "Hello.Stamp")]
#[derive(Default, Clone)]
pub struct Stamp {
    /// The moment, as given.
    pub at: PsDateTime,
    /// The same moment in UTC.
    pub at_utc: PsDateTime,
    /// How long it took.
    pub took: PsTimeSpan,
    /// Identity of the record.
    pub id: PsGuid,
    /// First letter of the label.
    pub initial: char,
    /// Earlier identities.
    pub history: Vec<PsGuid>,
    /// Signals seen, in order.
    pub signals: Vec<Signal>,
}

/// Builds a stamp from a date, a span, a GUID and a label.
#[cmdlet(verb = "New", noun = "RustStamp", output = ["Hello.Stamp"])]
#[derive(Default)]
pub struct NewRustStamp {
    /// The moment to stamp.
    #[param(mandatory, position = 0)]
    pub at: PsDateTime,
    /// How long it took; zero when absent.
    #[param]
    pub took: Option<PsTimeSpan>,
    /// Identity of the record.
    #[param(mandatory)]
    pub id: PsGuid,
    /// Its first letter becomes the initial.
    #[param(mandatory)]
    pub label: String,
    /// Earlier identities.
    #[param]
    pub history: Vec<PsGuid>,
    /// Signals seen.
    #[param]
    pub signals: Vec<Signal>,
}

impl Cmdlet for NewRustStamp {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let initial = match self.label.chars().next() {
            Some(c) => c,
            None => return Err(PsError::new(ErrorCategory::InvalidArgument, "EmptyLabel", "the label is empty")),
        };
        ps.write(Stamp {
            at: self.at,
            at_utc: self.at.to_utc()?,
            took: self.took.unwrap_or(PsTimeSpan::from_ticks(0)),
            id: self.id,
            initial,
            history: std::mem::take(&mut self.history),
            signals: std::mem::take(&mut self.signals),
        })
    }
}

/// Writes the date shifted by the span, keeping its kind, then the
/// span negated.
#[cmdlet(verb = "Add", noun = "RustTime", output = ["System.DateTime", "System.TimeSpan"])]
#[derive(Default)]
pub struct AddRustTime {
    #[param(mandatory, position = 0)]
    pub at: PsDateTime,
    #[param(mandatory, position = 1)]
    pub span: PsTimeSpan,
}

impl Cmdlet for AddRustTime {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let ticks = match self.at.ticks.checked_add(self.span.ticks) {
            Some(t) => t,
            None => return Err(PsError::new(ErrorCategory::InvalidArgument, "TimeOverflow", "the sum leaves the DateTime range")),
        };
        ps.write(PsDateTime::new(ticks, self.at.kind))?;
        ps.write(PsTimeSpan::from_ticks(-self.span.ticks))
    }
}

/// Writes a GUID's text, the GUID itself, a character, and how many
/// GUIDs were listed.
#[cmdlet(verb = "Test", noun = "RustValues")]
#[derive(Default)]
pub struct TestRustValues {
    #[param(mandatory, position = 0)]
    pub id: PsGuid,
    #[param(mandatory, position = 1)]
    pub letter: char,
    #[param]
    pub ids: Vec<PsGuid>,
}

impl Cmdlet for TestRustValues {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.id.to_string())?;
        ps.write(self.id)?;
        ps.write(self.letter)?;
        ps.write(self.ids.len() as i64)
    }
}

/// Writes `user:password-length` for a credential; with `-Reverse`
/// also a credential built in Rust whose password is reversed; with
/// `-Secret` the secret's text.
#[cmdlet(verb = "Test", noun = "RustCredential")]
#[derive(Default)]
pub struct TestRustCredential {
    #[param(mandatory, position = 0)]
    pub credential: PsCredential,
    #[param]
    pub reverse: bool,
    #[param]
    pub secret: Option<PsSecureString>,
}

impl Cmdlet for TestRustCredential {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let password = self.credential.password.reveal()?;
        ps.write(format!("{}:{}", self.credential.user_name, password.chars().count()))?;
        if self.reverse {
            let reversed: String = password.chars().rev().collect();
            ps.write(PsCredential::new(&self.credential.user_name, &reversed)?)?;
        }
        if let Some(secret) = &self.secret {
            ps.write(secret.reveal()?)?;
        }
        Ok(())
    }
}

/// A team: a lead and members, each a `Hello.Person`.
#[psclass(name = "Hello.Team")]
#[derive(Default, Clone)]
pub struct Team {
    /// Who leads.
    pub lead: Person,
    /// Everyone else.
    pub members: Vec<Person>,
    /// The note pinned to the team, when there is one.
    pub note: Option<Note>,
}

/// Builds a team from person objects.
#[cmdlet(verb = "New", noun = "RustTeam", output = ["Hello.Team"])]
#[derive(Default)]
pub struct NewRustTeam {
    #[param(mandatory, position = 0)]
    pub lead: Person,
    #[param]
    pub member: Vec<Person>,
    #[param]
    pub note: Option<Note>,
}

impl Cmdlet for NewRustTeam {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Team { lead: self.lead.clone(), members: std::mem::take(&mut self.member), note: self.note.take() })
    }
}

/// Reads a team back and writes `lead:count:note`.
#[cmdlet(verb = "Get", noun = "RustTeamSummary", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustTeamSummary {
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub team: Team,
}

impl Cmdlet for GetRustTeamSummary {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let note = match &self.team.note {
            Some(n) => n.text.clone(),
            None => "none".to_string(),
        };
        ps.write(format!("{}:{}:{}", self.team.lead.name, self.team.members.len(), note))
    }
}

/// Reads a proxy counter back into Rust and writes `label=value`.
#[cmdlet(verb = "Get", noun = "RustCounterText", output = ["System.String"])]
#[derive(Default)]
pub struct GetRustCounterText {
    #[param(mandatory, position = 0)]
    pub counter: Counter,
}

impl Cmdlet for GetRustCounterText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("{}={}", self.counter.label, self.counter.value))
    }
}

/// Writes `count` bytes, 0 upward, as a `Memory<byte>` over Rust
/// memory (a `byte[]` copy on Windows PowerShell).
#[cmdlet(verb = "Get", noun = "RustMemory")]
#[derive(Default)]
pub struct GetRustMemory {
    #[param(mandatory, position = 0, validate_range(0, 1000000000))]
    pub count: i64,
}

impl Cmdlet for GetRustMemory {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let mut m = PsMemory::<u8>::zeroed(self.count as usize)?;
        for (i, b) in m.iter_mut().enumerate() {
            *b = i as u8;
        }
        ps.write(m)
    }
}

/// Sums a `byte[]` bound to a `Vec<u8>` parameter.
#[cmdlet(verb = "Get", noun = "RustByteSum", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustByteSum {
    #[param(mandatory, position = 0)]
    pub bytes: Vec<u8>,
}

impl Cmdlet for GetRustByteSum {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.bytes.iter().map(|&b| b as i64).sum::<i64>())
    }
}

/// Sums a byte array bound without the engine's array coercion.
#[cmdlet(verb = "Get", noun = "RustRawByteSum", output = ["System.Int64"])]
#[derive(Default)]
pub struct GetRustRawByteSum {
    /// Declared `object`, converted to `Vec<u8>` in Rust.
    #[param(mandatory, position = 0, raw)]
    pub bytes: Vec<u8>,
}

impl Cmdlet for GetRustRawByteSum {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.bytes.iter().map(|&b| b as i64).sum::<i64>())
    }
}

/// Writes `count` bytes, 0 upward, as one `byte[]` from a `Vec<u8>`.
#[cmdlet(verb = "Get", noun = "RustByteRange")]
#[derive(Default)]
pub struct GetRustByteRange {
    #[param(mandatory, position = 0, validate_range(0, 1000000000))]
    pub count: i64,
}

impl Cmdlet for GetRustByteRange {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let bytes: Vec<u8> = (0..self.count).map(|i| i as u8).collect();
        ps.write(PsArray(bytes))
    }
}

/// Reserves `bytes` bytes through the fallible allocator API and writes
/// how many it holds. A size the allocator cannot meet comes back as
/// the `PwrsOutOfMemory` error record; the infallible forms would end
/// the host process instead.
#[cmdlet(verb = "New", noun = "RustReservation", output = ["System.UInt64"])]
#[derive(Default)]
pub struct NewRustReservation {
    #[param(mandatory, position = 0)]
    pub bytes: u64,
}

impl Cmdlet for NewRustReservation {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(self.bytes as usize)?;
        ps.write(buf.capacity() as u64)
    }
}

/// Reads the type name of `InputObject` from a thread this cmdlet
/// starts, the call a worker must not make, and says what happened:
/// `refused: PwrsOffThread` while the thread check is on, the name when
/// it is off.
#[cmdlet(verb = "Test", noun = "RustOffThread", output = ["System.String"])]
#[derive(Default)]
pub struct TestRustOffThread {
    #[param(mandatory, position = 0)]
    pub input_object: PsObject,
}

impl Cmdlet for TestRustOffThread {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let obj = self.input_object.clone();
        let answer = match std::thread::spawn(move || obj.type_name()).join() {
            Ok(answer) => answer,
            Err(_panic) => return Err(PsError::new(ErrorCategory::InvalidOperation, "RustOffThreadPanic", "the worker panicked")),
        };
        match answer {
            Ok(name) => ps.write(format!("ran: {name}")),
            Err(e) => ps.write(format!("refused: {}", e.error_id)),
        }
    }
}

/// Sums a `Double[]`, pinned in place, through the widest kernel
/// `pwrs::cpu::has` allows here, and writes the tier that ran and then
/// the sum. Every tier adds in one order, element `i` into running sum
/// `i % 8` and the eight combined in a fixed tree, so AVX-512, AVX2 and
/// scalar give the same bits.
///
/// # Examples
/// Measure-RustTieredSum -Values ([double[]](1..100))
#[cmdlet(verb = "Measure", noun = "RustTieredSum")]
#[derive(Default)]
pub struct MeasureRustTieredSum {
    /// A `Double[]`, taken without the binder's coercion so it pins.
    #[param(mandatory, position = 0, raw)]
    pub values: PsObject,
}

impl Cmdlet for MeasureRustTieredSum {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let values = self.values.pin::<f64>()?;
        let (tier, lanes) = tiered_lanes(&values);
        ps.write(tier)?;
        ps.write(((lanes[0] + lanes[4]) + (lanes[2] + lanes[6])) + ((lanes[1] + lanes[5]) + (lanes[3] + lanes[7])))
    }
}

/// The eight running sums through the widest tier allowed, and its name.
fn tiered_lanes(x: &[f64]) -> (&'static str, [f64; 8]) {
    #[cfg(target_arch = "x86_64")]
    {
        use pwrs::cpu::{has, Isa};
        if has(Isa::Avx512f) {
            return ("avx512f", unsafe { lanes_avx512(x) });
        }
        if has(Isa::Avx2) {
            return ("avx2", unsafe { lanes_avx2(x) });
        }
    }
    ("scalar", lanes_scalar(x))
}

fn lanes_scalar(x: &[f64]) -> [f64; 8] {
    let mut acc = [0.0f64; 8];
    for (i, v) in x.iter().enumerate() {
        acc[i % 8] += v;
    }
    acc
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn lanes_avx2(x: &[f64]) -> [f64; 8] {
    use std::arch::x86_64::{_mm256_add_pd, _mm256_loadu_pd, _mm256_setzero_pd, _mm256_storeu_pd};
    let blocks = x.len() / 8;
    let mut lo = _mm256_setzero_pd();
    let mut hi = _mm256_setzero_pd();
    for b in 0..blocks {
        let p = x.as_ptr().add(b * 8);
        lo = _mm256_add_pd(lo, _mm256_loadu_pd(p));
        hi = _mm256_add_pd(hi, _mm256_loadu_pd(p.add(4)));
    }
    let mut acc = [0.0f64; 8];
    _mm256_storeu_pd(acc.as_mut_ptr(), lo);
    _mm256_storeu_pd(acc.as_mut_ptr().add(4), hi);
    for (j, v) in x[blocks * 8..].iter().enumerate() {
        acc[j] += v;
    }
    acc
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn lanes_avx512(x: &[f64]) -> [f64; 8] {
    use std::arch::x86_64::{_mm512_add_pd, _mm512_loadu_pd, _mm512_setzero_pd, _mm512_storeu_pd};
    let blocks = x.len() / 8;
    let mut v = _mm512_setzero_pd();
    for b in 0..blocks {
        v = _mm512_add_pd(v, _mm512_loadu_pd(x.as_ptr().add(b * 8)));
    }
    let mut acc = [0.0f64; 8];
    _mm512_storeu_pd(acc.as_mut_ptr(), v);
    for (j, v) in x[blocks * 8..].iter().enumerate() {
        acc[j] += v;
    }
    acc
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tiered_sum_tests {
    use super::{lanes_avx2, lanes_avx512, lanes_scalar};
    use pwrs::cpu::{detected, Isa};

    /// Lengths leaving every remainder from 0 to 7 after the eight-wide
    /// blocks, over values whose sums come out differently in another order.
    fn inputs() -> Vec<Vec<f64>> {
        [0usize, 1, 7, 8, 9, 15, 16, 17, 1003, 4099]
            .iter()
            .map(|&n| (0..n).map(|i| ((i as f64) * 0.37).sin() * 1e3 + 1.0 / (i as f64 + 1.0)).collect())
            .collect()
    }

    /// Compares, bit for bit, each vector tier the CPU and the operating
    /// system offer with the scalar one over every input, and answers the
    /// tiers compared. `detected` rather than `has`, so `PWRS_CPU_MAX`
    /// narrows nothing here.
    fn compare_detected_tiers() -> Vec<&'static str> {
        let (avx2, avx512f) = (detected(Isa::Avx2), detected(Isa::Avx512f));
        for x in inputs() {
            let want = lanes_scalar(&x).map(f64::to_bits);
            if avx2 {
                // SAFETY: the CPU and the operating system offer AVX2.
                assert_eq!(unsafe { lanes_avx2(&x) }.map(f64::to_bits), want, "avx2 at length {}", x.len());
            }
            if avx512f {
                // SAFETY: the CPU and the operating system offer AVX-512F.
                assert_eq!(unsafe { lanes_avx512(&x) }.map(f64::to_bits), want, "avx512f at length {}", x.len());
            }
        }
        let mut compared = vec!["scalar"];
        if avx2 {
            compared.push("avx2");
        }
        if avx512f {
            compared.push("avx512f");
        }
        compared
    }

    #[test]
    fn every_tier_this_machine_offers_matches_scalar() {
        compare_detected_tiers();
    }

    /// Intel SDE sets `SDE_COMMAND_LINE` for the program it runs, so this
    /// runs only under SDE and returns at once everywhere else. Run the test
    /// binary under an emulated CPU with AVX-512, `sde -spr -- <binary>`,
    /// and a machine without those extensions proves them too.
    #[test]
    fn under_sde_every_tier_matches_scalar() {
        if std::env::var_os("SDE_COMMAND_LINE").is_none() {
            return;
        }
        let compared = compare_detected_tiers();
        eprintln!("under SDE, compared with scalar: {compared:?}");
        assert_eq!(compared, ["scalar", "avx2", "avx512f"], "the emulated CPU must offer AVX2 and AVX-512F; run the test binary with sde -spr or a later CPU");
    }
}

/// One x86-64 instruction-set extension as this module sees it.
#[psclass(name = "Hello.CpuFeature")]
#[derive(Default, Clone)]
pub struct CpuFeature {
    /// The `target_feature` name.
    pub name: String,
    /// The psABI level it belongs to, or `native` for one no level includes.
    pub level: String,
    /// Whether this library was compiled to require it.
    pub compiled: bool,
    /// Whether the CPU and the operating system offer it.
    pub detected: bool,
    /// Whether a kernel may use it here: detected, and within `PWRS_CPU_MAX`.
    pub usable: bool,
}

/// Lists the x86-64 extensions `pwrs::cpu` knows, with what this library
/// was compiled for, what the machine offers, and what a kernel may use.
///
/// # Examples
/// Get-RustCpu -Compiled
#[cmdlet(verb = "Get", noun = "RustCpu", output = ["Hello.CpuFeature"])]
#[derive(Default)]
pub struct GetRustCpu {
    /// Only the extensions this library was compiled to require.
    #[param]
    pub compiled: bool,
}

impl Cmdlet for GetRustCpu {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for f in pwrs::cpu::FEATURES {
            if self.compiled && !f.compiled {
                continue;
            }
            ps.write(CpuFeature {
                name: f.name.to_string(),
                level: f.level.name().to_string(),
                compiled: f.compiled,
                detected: pwrs::cpu::detected(f.isa),
                usable: pwrs::cpu::has(f.isa),
            })?;
        }
        Ok(())
    }
}

/// One step of a series' plan.
#[derive(Clone, Copy, Debug)]
enum Step {
    Scale(f64),
    Shift(f64),
    Above(f64),
}

/// A lazy series of numbers. The source counts up from 0; each stage
/// that takes the series by type writes a new one whose plan has its
/// step appended. Nothing is computed until a cmdlet reads the series,
/// which runs every step over every number in one pass.
#[psclass(name = "Hello.Series", mode = proxy)]
#[derive(Clone)]
pub struct Series {
    /// How many numbers the source counts, from 0.
    pub count: u64,
    /// The plan's steps, in the order they apply.
    pub plan: Vec<String>,
    #[psfield(skip)]
    steps: Vec<Step>,
}

impl Series {
    fn push(&mut self, step: Step) {
        self.plan.push(match step {
            Step::Scale(k) => format!("scale {k}"),
            Step::Shift(k) => format!("shift {k}"),
            Step::Above(k) => format!("above {k}"),
        });
        self.steps.push(step);
    }

    /// Runs the plan over the source and hands each number that survives
    /// it to `f`, in order, until `f` answers false.
    fn run(&self, mut f: impl FnMut(f64) -> bool) {
        'numbers: for i in 0..self.count {
            let mut v = i as f64;
            for step in &self.steps {
                match *step {
                    Step::Scale(k) => v *= k,
                    Step::Shift(k) => v += k,
                    Step::Above(k) => {
                        if v.is_nan() || v <= k {
                            continue 'numbers;
                        }
                    }
                }
            }
            if !f(v) {
                break;
            }
        }
    }
}

/// Starts a series of `Count` numbers counting up from 0, with an empty
/// plan: one object standing for all of them.
///
/// # Examples
/// New-RustSeries -Count 1000000 | Add-RustSeriesStep -Scale 2 | Measure-RustSeries
#[cmdlet(verb = "New", noun = "RustSeries", output = ["Hello.Series"])]
#[derive(Default)]
pub struct NewRustSeries {
    /// How many numbers the source counts.
    #[param(mandatory, position = 0, validate_range(0, 1000000000))]
    pub count: u64,
}

impl Cmdlet for NewRustSeries {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Series { count: self.count, plan: Vec::new(), steps: Vec::new() })
    }
}

/// Writes a new series whose plan is the given one's with the named
/// steps appended, in the order Scale, Shift, Above. The given series is
/// left as it was, and no number is computed here.
///
/// # Examples
/// New-RustSeries 10 | Add-RustSeriesStep -Scale 2 -Above 5 | Expand-RustSeries
#[cmdlet(verb = "Add", noun = "RustSeriesStep", output = ["Hello.Series"])]
#[derive(Default)]
pub struct AddRustSeriesStep {
    /// The series to extend, taken by type from the pipeline.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub series: PsProxy<Series>,
    /// Multiplies every number.
    #[param]
    pub scale: Option<f64>,
    /// Adds to every number.
    #[param]
    pub shift: Option<f64>,
    /// Keeps only the numbers greater than this.
    #[param]
    pub above: Option<f64>,
}

impl Cmdlet for AddRustSeriesStep {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.scale.is_none() && self.shift.is_none() && self.above.is_none() {
            return Err(PsError::new(ErrorCategory::InvalidArgument, "NoStep", "name at least one of -Scale, -Shift and -Above"));
        }
        let mut next = self.series.with(Series::clone)?;
        if let Some(k) = self.scale {
            next.push(Step::Scale(k));
        }
        if let Some(k) = self.shift {
            next.push(Step::Shift(k));
        }
        if let Some(k) = self.above {
            next.push(Step::Above(k));
        }
        ps.write(next)
    }
}

/// Writes every number of the series after its plan, one object each:
/// the way from a series back to rows.
#[cmdlet(verb = "Expand", noun = "RustSeries", output = ["System.Double"])]
#[derive(Default)]
pub struct ExpandRustSeries {
    /// The series to read, taken by type from the pipeline.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub series: PsProxy<Series>,
}

impl Cmdlet for ExpandRustSeries {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        // The rows are written from a copy, so the series is not held
        // while a command downstream runs and may reach it.
        let series = self.series.with(Series::clone)?;
        let mut written = Ok(());
        series.run(|v| match ps.write(v) {
            Ok(()) => true,
            Err(e) => {
                written = Err(e);
                false
            }
        });
        written
    }
}

/// What a series comes to after its plan.
#[psclass(name = "Hello.SeriesTotal")]
#[derive(Default, Clone)]
pub struct SeriesTotal {
    /// How many numbers survived the plan.
    pub count: u64,
    /// Their sum.
    pub sum: f64,
}

/// Counts and sums a series after its plan, in one pass, writing none
/// of its numbers.
#[cmdlet(verb = "Measure", noun = "RustSeries", output = ["Hello.SeriesTotal"])]
#[derive(Default)]
pub struct MeasureRustSeries {
    /// The series to read, taken by type from the pipeline.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub series: PsProxy<Series>,
}

impl Cmdlet for MeasureRustSeries {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let total = self.series.with(|s| {
            let mut total = SeriesTotal::default();
            s.run(|v| {
                total.count += 1;
                total.sum += v;
                true
            });
            total
        })?;
        ps.write(total)
    }
}

/// Holds the series while `Script` runs, as a shared reader through
/// `with` or exclusively through `with_mut`, then writes `ran:` and the
/// strings the script wrote, joined by commas, or `refused:` and why.
#[cmdlet(verb = "Test", noun = "RustSeriesHold", output = ["System.String"])]
#[derive(Default)]
pub struct TestRustSeriesHold {
    /// The series to hold.
    #[param(mandatory, position = 0)]
    pub series: PsProxy<Series>,
    /// What runs while it is held.
    #[param(mandatory, position = 1)]
    pub script: PsScriptBlock,
    /// Hold the series exclusively, as `with_mut` does.
    #[param]
    pub exclusive: bool,
}

impl Cmdlet for TestRustSeriesHold {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let script = &self.script;
        let outcome = if self.exclusive {
            self.series.with_mut(|_| script.call(ps, &[]))?
        } else {
            self.series.with(|_| script.call(ps, &[]))?
        };
        match outcome {
            Ok(out) => {
                let text = out.iter().map(String::from_ps).collect::<PsResult<Vec<String>>>()?;
                ps.write(format!("ran: {}", text.join(",")))
            }
            Err(e) => ps.write(format!("refused: {}", e.message)),
        }
    }
}

/// Writes `started` from a worker thread that then sends nothing for
/// `Seconds`, and `finished` once that time has run out. A stopped
/// pipeline reaches the silent worker through its stop signal.
#[cmdlet(verb = "Wait", noun = "RustSilence", output = ["System.String"])]
#[derive(Default)]
pub struct WaitRustSilence {
    /// How long the worker stays silent.
    #[param(mandatory, position = 0, validate_range(1, 3600))]
    pub seconds: u64,
}

impl Cmdlet for WaitRustSilence {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let quiet = std::time::Duration::from_secs(self.seconds);
        ps.stream_from_thread_until(move |tx, stop| {
            if tx.send("started".to_string()).is_err() {
                return;
            }
            let until = std::time::Instant::now() + quiet;
            while std::time::Instant::now() < until && !stop.is_set() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })?;
        if ps.stopping() {
            return Ok(());
        }
        ps.write("finished")
    }
}

/// Values of `Hello.Ballast` not yet freed.
static BALLAST_ALIVE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// Values of `Hello.QuietBallast` not yet freed.
static QUIET_BALLAST_ALIVE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// A proxy that reports `Claimed` bytes of native memory without
/// allocating them, so a test sees what the report does to collection
/// without spending the memory.
#[psclass(name = "Hello.Ballast", mode = proxy, native_bytes = Ballast::claimed_bytes)]
pub struct Ballast {
    /// The bytes it reports.
    pub claimed: u64,
}

impl Ballast {
    fn claimed_bytes(&self) -> usize {
        self.claimed as usize
    }
}

impl Drop for Ballast {
    fn drop(&mut self) {
        BALLAST_ALIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `Hello.Ballast` without the report.
#[psclass(name = "Hello.QuietBallast", mode = proxy)]
pub struct QuietBallast {
    /// The bytes it would report.
    pub claimed: u64,
}

impl Drop for QuietBallast {
    fn drop(&mut self) {
        QUIET_BALLAST_ALIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Makes one `Hello.Ballast` claiming `Megabytes`, or with `-Quiet` one
/// `Hello.QuietBallast`, and writes it.
#[cmdlet(verb = "New", noun = "RustBallast", output = ["Hello.Ballast", "Hello.QuietBallast"])]
#[derive(Default)]
pub struct NewRustBallast {
    /// What it claims to hold, in megabytes.
    #[param(mandatory, position = 0, validate_range(0, 65536))]
    pub megabytes: u64,
    /// Make one that reports nothing.
    #[param]
    pub quiet: bool,
}

impl Cmdlet for NewRustBallast {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        use std::sync::atomic::Ordering;
        let claimed = self.megabytes << 20;
        if self.quiet {
            QUIET_BALLAST_ALIVE.fetch_add(1, Ordering::Relaxed);
            ps.write(QuietBallast { claimed })
        } else {
            BALLAST_ALIVE.fetch_add(1, Ordering::Relaxed);
            ps.write(Ballast { claimed })
        }
    }
}

/// Changes what a ballast claims, in place through `with_mut`, and
/// writes the same object on.
#[cmdlet(verb = "Set", noun = "RustBallast", output = ["Hello.Ballast"])]
#[derive(Default)]
pub struct SetRustBallast {
    /// The ballast to change, taken by type from the pipeline.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub ballast: PsProxy<Ballast>,
    /// What it claims from now on, in megabytes.
    #[param(mandatory, validate_range(0, 65536))]
    pub megabytes: u64,
}

impl Cmdlet for SetRustBallast {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let claimed = self.megabytes << 20;
        self.ballast.with_mut(|b| b.claimed = claimed)?;
        ps.write_object(self.ballast.object())
    }
}

/// What `Measure-RustPressure` saw.
#[psclass(name = "Hello.PressureReport")]
#[derive(Default, Clone)]
pub struct PressureReport {
    /// How many objects were made.
    pub made: u64,
    /// Collections of any generation that ran while they were made.
    pub collections: i64,
    /// Of those, full collections.
    pub full_collections: i64,
    /// Values made here and not yet freed once the pending finalizers ran.
    pub alive: i64,
}

/// Makes `Count` ballast objects claiming `Megabytes` each, `Interval`
/// milliseconds apart, letting go of each at once, then writes how many
/// collections ran meanwhile, of any generation and full, and how many
/// values were left once the finalizers those collections queued had
/// run. `-Quiet` makes objects that report nothing.
#[cmdlet(verb = "Measure", noun = "RustPressure", output = ["Hello.PressureReport"])]
#[derive(Default)]
pub struct MeasureRustPressure {
    /// How many objects to make.
    #[param(mandatory, position = 0, validate_range(1, 100000))]
    pub count: u64,
    /// What each claims to hold, in megabytes.
    #[param(mandatory, validate_range(1, 65536))]
    pub megabytes: u64,
    /// Milliseconds between one object and the next.
    #[param(validate_range(0, 1000))]
    pub interval: u64,
    /// Make objects that report nothing.
    #[param]
    pub quiet: bool,
}

impl Cmdlet for MeasureRustPressure {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        use std::sync::atomic::Ordering;
        let gc = PsType::from_name("System.GC");
        // Generation 0 is collected by every collection, full ones included.
        let collections = |generation: i32| -> PsResult<i64> { i64::from_ps(&gc.call_static("CollectionCount", &[generation.into_ps()?])?) };
        let alive = if self.quiet { &QUIET_BALLAST_ALIVE } else { &BALLAST_ALIVE };
        let claimed = self.megabytes << 20;
        let alive_before = alive.load(Ordering::Relaxed);
        let (any_before, full_before) = (collections(0)?, collections(2)?);
        let interval = std::time::Duration::from_millis(self.interval);
        for i in 0..self.count {
            if i > 0 && !interval.is_zero() {
                std::thread::sleep(interval);
            }
            alive.fetch_add(1, Ordering::Relaxed);
            let obj = if self.quiet { QuietBallast { claimed }.into_ps()? } else { Ballast { claimed }.into_ps()? };
            drop(obj);
        }
        let report = PressureReport {
            made: self.count,
            collections: collections(0)? - any_before,
            full_collections: collections(2)? - full_before,
            alive: 0,
        };
        gc.call_static("WaitForPendingFinalizers", &[])?;
        ps.write(PressureReport { alive: alive.load(Ordering::Relaxed) - alive_before, ..report })
    }
}

/// Runs a helper executable the module ships and writes each line it
/// prints. The helper starts from the copy `pwrs::helper_path` stages
/// for this process, never from the module folder.
///
/// # Examples
/// Invoke-HelloHelper echo hello world
/// Invoke-HelloHelper where
#[cmdlet(verb = "Invoke", noun = "HelloHelper", output = ["System.String"])]
#[derive(Default)]
pub struct InvokeHelloHelper {
    /// The helper's arguments.
    #[param(position = 0, value_from_remaining)]
    pub argument_list: Vec<String>,
    /// Which helper to run; hello-helper when absent.
    #[param]
    pub name: Option<String>,
}

impl Cmdlet for InvokeHelloHelper {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let helper = pwrs::helper_path(self.name.as_deref().unwrap_or("hello-helper"))?;
        let out = std::process::Command::new(&helper).args(&self.argument_list).output()?;
        if !out.status.success() {
            return Err(PsError::new(
                ErrorCategory::InvalidResult,
                "HelloHelperFailed",
                format!("{} exited with {}: {}", helper.display(), out.status, String::from_utf8_lossy(&out.stderr).trim()),
            ));
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            ps.write(line.to_string())?;
        }
        Ok(())
    }
}

/// Helpers `Start-HelloHelper` started that `Stop-HelloHelper` has not
/// waited for yet.
static STARTED_HELPERS: std::sync::Mutex<Vec<std::process::Child>> = std::sync::Mutex::new(Vec::new());

fn started_helpers() -> std::sync::MutexGuard<'static, Vec<std::process::Child>> {
    match STARTED_HELPERS.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Starts hello-helper waiting on its standard input and writes its
/// process id. It runs until `Stop-HelloHelper` closes that input.
///
/// # Examples
/// $id = Start-HelloHelper
#[cmdlet(verb = "Start", noun = "HelloHelper", output = ["System.Int64"])]
#[derive(Default)]
pub struct StartHelloHelper;

impl Cmdlet for StartHelloHelper {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let child = std::process::Command::new(pwrs::helper_path("hello-helper")?)
            .arg("wait")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let id = i64::from(child.id());
        started_helpers().push(child);
        ps.write(id)
    }
}

/// Closes the input of every helper `Start-HelloHelper` started, waits
/// for each to exit, and writes what each printed.
///
/// # Examples
/// Stop-HelloHelper
#[cmdlet(verb = "Stop", noun = "HelloHelper", output = ["System.String"])]
#[derive(Default)]
pub struct StopHelloHelper;

impl Cmdlet for StopHelloHelper {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let children = std::mem::take(&mut *started_helpers());
        for child in children {
            // wait_with_output closes the child's input first, which is
            // what the waiting helper reads to its end.
            let out = child.wait_with_output()?;
            if !out.status.success() {
                return Err(PsError::new(ErrorCategory::InvalidResult, "HelloHelperFailed", format!("hello-helper exited with {}", out.status)));
            }
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                ps.write(line.to_string())?;
            }
        }
        Ok(())
    }
}

pwrs::export_module! {
    name: "Hello",
    cmdlets: [
        GetGreeting, GetPerson, NewCounter, GetNote,
        InvokeRustBlock, GetRustTableInfo, GetRustTableEntry, NewRustTable, GetRustChecksum, GetRustBytes,
        TestRustBigInt, TestRustDynamic, ResolveRustPath, GetRustTypeName,
        GetRustColor, GetRustReading, GetRustStaticReading, GetRustBlindReading, MeasureRustTotal,
        GetRustSignal, ConvertToRustSignal, NewRustLight, GetRustUnsigned,
        NewRustStamp, AddRustTime, TestRustValues, TestRustCredential,
        NewRustTeam, GetRustTeamSummary, GetRustCounterText, GetRustMemory,
        GetRustByteSum, GetRustByteRange, GetRustRawByteSum, NewRustTicker, NewRustSlots,
        GetRustModuleName, ExpandRustText, RemoveRustThing, GetRustRecord,
        GetRustStream, TestRustTarget, WriteRustStreams, GetRustProperty,
        GetRustParallel, MeasureRustParallel, GetRustReadOnlyTable,
        MeasureRustPropertyReads, MeasureRustTypeReads, GetRustWidths, GetRustTypeTag,
        GetRustDecimalRoundTrip, MeasureRustDecimalBlock, GetRustOffset, GetRustLifecycle, GetRustComposed, ReadRustHost,
        GetRustErrorInfo, GetRustInk, GetRustSize, GetRustInvocation, NewRustReservation, GetRustCpu, TestRustOffThread, MeasureRustTieredSum,
        NewRustSeries, AddRustSeriesStep, ExpandRustSeries, MeasureRustSeries, TestRustSeriesHold, WaitRustSilence, MeasureRustPressure,
        NewRustBallast, SetRustBallast, InvokeHelloHelper, StartHelloHelper, StopHelloHelper, GetRustRoute, MeasureRustInput,
    ],
    classes: [
        Person, Counter, Note, TableInfo, Light, Stamp, Team, Ticker, Slots, Stretch, CpuFeature,
        Series, SeriesTotal, Ballast, QuietBallast, PressureReport,
    ],
    enums: [Signal],
    completers: [complete_color],
    transforms: [as_bytes],
    dynamic_params: [GetRustReading, GetRustBlindReading],
    on_import: count_import,
    on_remove: count_remove,
}

---
title: Objects In
weight: 5
---

Chapter 4 wrote objects out. Commands also take objects in: a script block that returns true or false, a hashtable of settings, an object of .NET's own. This chapter adds two commands that take all three. `Find-FileFactsLine` finds lines that match a pattern or a script block, and `Update-FileFactsText` replaces text in files from a hashtable.

The examples use two files:

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
Set-Content notes.txt "Buy teh bread.`nCall Ann about teh trip.`nBANANA bread`n" -NoNewline
```

Each line `Find-FileFactsLine` returns is a small copied class, as in chapter 4:

```rust
/// A line Find-FileFactsLine found.
#[psclass(name = "FileFacts.LineMatch", columns = ["File", "Line", "Text"])]
#[derive(Default, Clone)]
pub struct LineMatch {
    /// The file's name.
    pub file: String,
    /// The line's number, counted from 1.
    pub line: i64,
    /// The line's text.
    pub text: String,
}
```

## A .NET object made from Rust

`-Pattern` is a regular expression. Rust has regular expression crates of its own, but PowerShell users know .NET's, the ones `-match` and `Select-String` use, so `Find-FileFactsLine` uses .NET's too, and a pattern means what it means everywhere else in PowerShell. Rust makes the .NET object and calls its method:

```rust
        let regex = match &self.pattern {
            Some(pattern) => Some(PsType::from_name("System.Text.RegularExpressions.Regex").new(&[format!("(?i){pattern}").into_ps()?])?),
            None => None,
        };
```

```rust
                if let Some(regex) = &regex
                    && !bool::from_ps(&regex.call("IsMatch", &[line.into_ps()?])?)?
                {
                    continue;
                }
```

`PsType::from_name` names a .NET type, and `new` calls its constructor with the arguments given, each made with `into_ps`. `new` returns a `PsObject`, a handle to the .NET object, and `call` runs one of its methods by name. `bool::from_ps` turns what the method returned into a Rust `bool`. `(?i)` at the front of the pattern makes it ignore case, as `-match` does.

```powershell
Find-FileFactsLine fruit.txt, notes.txt -Pattern 'an'
```

```text
File      Line Text
----      ---- ----
fruit.txt    2 banana
notes.txt    2 Call Ann about teh trip.
notes.txt    3 BANANA bread
```

![PowerShell 7 showing the three matching lines as a table, its header in green](/images/objects-in-find.png)

`PsObject` also has `get` and `set` for properties, and `PsType` has `call_static` for static methods; [How To Call .NET From Rust](../../how-to/how-to-call-dotnet-from-rust/) lists them all. Each call goes from Rust into .NET and back, which costs far more than a Rust function call, so code that runs once for every line of a large file is better done in Rust when it can be.

When .NET refuses, its own error comes back: a pattern that does not parse ends the command with .NET's reason:

```powershell
Find-FileFactsLine fruit.txt -Pattern '('
```

```text
Find-FileFactsLine: RegexParseException: Invalid pattern '(?i)(' at offset 5. Not enough )'s.
```

![PowerShell 7 after Find-FileFactsLine fruit.txt -Pattern '(': .NET's reason in red](/images/objects-in-bad-pattern.png)

<!-- powershell -->

```text
Find-FileFactsLine : ArgumentException: parsing "(?i)(" - Not enough )'s.
At line:1 char:1
+ Find-FileFactsLine fruit.txt -Pattern '('
+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    + CategoryInfo          : NotSpecified: (:) [Find-FileFactsLine], PwrsException
    + FullyQualifiedErrorId : PwrsRuntimeError,Pwrs.Modules.FileFacts.FindLineCommand
```

![Windows PowerShell 5.1 after the same command: .NET's reason, the line, category and id in red](/images/objects-in-bad-pattern-powershell.png)

PowerShell 7 and Windows PowerShell 5.1 run on different versions of .NET, so the wording of the reason differs between them.

## A script block

`-FilterScript` takes a script block, the way `Where-Object -FilterScript` does. Its type is `PsScriptBlock`, and the command runs it once for each line:

```rust
    /// A script block that gets each line and returns whether it passes.
    #[param]
    pub filter_script: Option<PsScriptBlock>,
```

```rust
                if let Some(block) = &self.filter_script {
                    let answer = block.call(ps, &[line.into_ps()?])?;
                    let keep = match answer.first() {
                        Some(first) => bool::from_ps(first)?,
                        None => false,
                    };
                    if !keep {
                        continue;
                    }
                }
```

`call` runs the block with the arguments given, which the block reads with `param(...)` or `$args`, and returns everything the block wrote. `bool::from_ps` reads the first of it the way PowerShell reads a condition, so a number other than zero or a string that is not empty counts as true, and a block that writes nothing counts as false. A block runs on the pipeline thread, which is why `call` takes `ps`.

An `Option` parameter is `None` when it is not given, so `-Pattern` and `-FilterScript` are both optional, and a line must pass each one that is:

```powershell
Find-FileFactsLine fruit.txt, notes.txt -FilterScript { param($line) $line.Length -gt 10 }
Find-FileFactsLine notes.txt -Pattern bread -FilterScript { param($line) $line -cmatch '^[A-Z]+ ' }
```

```text
File      Line Text
----      ---- ----
notes.txt    1 Buy teh bread.
notes.txt    2 Call Ann about teh trip.
notes.txt    3 BANANA bread


File      Line Text
----      ---- ----
notes.txt    3 BANANA bread
```

![PowerShell 7 after the two commands: a table of the three long lines and a table of the one line both tests pass, each header in green](/images/objects-in-filter.png)

## A hashtable

`-Replace` takes a hashtable whose keys are the text to find and whose values are the text to put in its place. Its type is `PsHashtable`:

```rust
    /// What to replace: each key is the text to find, and its value the
    /// text to put in its place.
    #[param(mandatory)]
    pub replace: PsHashtable,
```

```rust
        let mut pairs = Vec::new();
        for key in self.replace.keys()? {
            let value = String::from_ps(&self.replace.get(&key)?)?;
            pairs.push((key, value));
        }
```

`keys` returns the keys as strings, and `get` returns the value under a key as a `PsObject`, which `String::from_ps` turns into Rust text. The command reads the table once, before the files, and then calls `should_process` before it writes each file that changes, as `Remove-FileFactsEmpty` does in chapter 3:

```powershell
Update-FileFactsText notes.txt -Replace @{ teh = 'the' } -WhatIf
Update-FileFactsText notes.txt -Replace @{ teh = 'the' }
Get-Content notes.txt
```

```text
What if: Performing the operation "Make 2 replacements" on target "notes.txt".
Buy the bread.
Call Ann about the trip.
BANANA bread
```

A `PsHashtable` parameter takes the caller's own table as it is, with no copy. A plain `@{ }` table has its keys in no set order, while an `[ordered]` one has them in the order they were written, and `keys` returns them in that order. That matters when one replacement makes text another one finds: here `apple` becomes `pear` first, and then `pear` becomes `plum`:

```powershell
Update-FileFactsText fruit.txt -Replace ([ordered]@{ apple = 'pear'; pear = 'plum' })
Get-Content fruit.txt -TotalCount 1
```

```text
plum
```

## The whole module so far

`Cargo.toml` is the same as at the end of chapter 1.

<details>
<summary>src/lib.rs after this chapter</summary>

<!-- file: src/lib.rs -->
```rust
use pwrs::prelude::*;
use std::collections::HashMap;

/// How the lines of a file end.
#[psenum(name = "FileFacts.LineEnding")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// No line ends: the file is empty or has one line with no ending.
    #[default]
    None,
    /// Every line ends in a line feed.
    Lf,
    /// Every line ends in a carriage return and a line feed.
    CrLf,
    /// Some lines end one way and some the other.
    Mixed,
}

/// What Measure-FileFactsText found in one file.
#[psclass(name = "FileFacts.TextFacts", columns = ["Name", "Lines", "Words", "Bytes", "LineEnding"])]
#[derive(Default, Clone)]
pub struct TextFacts {
    /// The file's name.
    pub name: String,
    /// The file's full path.
    pub path: String,
    /// How many lines it has.
    pub lines: i64,
    /// How many words it has: runs of characters between spaces and
    /// line breaks.
    pub words: i64,
    /// How many bytes it has.
    pub bytes: i64,
    /// How its lines end.
    pub line_ending: LineEnding,
}

impl TextFacts {
    /// The facts of the text read from `path`.
    fn of(path: &str, text: &str) -> TextFacts {
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        let line_ending = match (lf, crlf) {
            (0, 0) => LineEnding::None,
            (_, 0) => LineEnding::Lf,
            (0, _) => LineEnding::CrLf,
            _ => LineEnding::Mixed,
        };
        TextFacts {
            name: file_name(path),
            path: path.to_string(),
            lines: text.lines().count() as i64,
            words: text.split_whitespace().count() as i64,
            bytes: text.len() as i64,
            line_ending,
        }
    }
}

/// A line Find-FileFactsLine found.
#[psclass(name = "FileFacts.LineMatch", columns = ["File", "Line", "Text"])]
#[derive(Default, Clone)]
pub struct LineMatch {
    /// The file's name.
    pub file: String,
    /// The line's number, counted from 1.
    pub line: i64,
    /// The line's text.
    pub text: String,
}

/// One place a word appears.
#[psclass(name = "FileFacts.WordPlace", show = "{File}:{Line}")]
#[derive(Default, Clone)]
pub struct WordPlace {
    /// The file's name.
    pub file: String,
    /// The line, counted from 1.
    pub line: i64,
}

/// The words of a set of files, stored in Rust, with the places each one
/// appears.
#[psclass(name = "FileFacts.WordIndex", mode = proxy, view = summary)]
#[derive(Default, Clone)]
pub struct WordIndex {
    /// How many files the index has.
    pub files: i64,
    /// How many different words they have, compared without regard to
    /// case.
    pub words: i64,
    /// Each word in lowercase, with the places it appears.
    #[psfield(skip)]
    places: HashMap<String, Vec<WordPlace>>,
}

/// The methods a script calls on a FileFacts.WordIndex.
#[psmethods]
impl WordIndex {
    /// The places `word` appears, compared without regard to case, in the
    /// order the files were read.
    pub fn find(&self, word: String) -> PsResult<Vec<WordPlace>> {
        match self.places.get(&word.to_lowercase()) {
            Some(places) => Ok(places.clone()),
            None => Ok(Vec::new()),
        }
    }

    /// One line describing the index, which is how it shows on its own.
    pub fn summary(&self) -> PsResult<String> {
        Ok(format!("{} words in {} files", self.words, self.files))
    }
}

impl WordIndex {
    /// Adds the words of `text`, read from the file named `file`.
    fn add(&mut self, file: &str, text: &str) {
        self.files += 1;
        for (number, line) in text.lines().enumerate() {
            for word in line.split_whitespace() {
                let word = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
                if word.is_empty() {
                    continue;
                }
                let place = WordPlace { file: file.to_string(), line: number as i64 + 1 };
                self.places.entry(word).or_default().push(place);
            }
        }
        self.words = self.places.len() as i64;
    }
}

/// Measures text files: their lines, words, bytes and line endings.
///
/// Writes one FileFacts.TextFacts object for each file.
///
/// # Examples
/// Measure-FileFactsText fruit.txt, veg.txt
/// Get-ChildItem *.txt | Measure-FileFactsText | Sort-Object Lines
/// Measure-FileFactsText *.txt | Measure-Object Lines -Sum
#[cmdlet(verb = "Measure", noun = "FileFactsText", default_parameter_set = "Path", output = ["FileFacts.TextFacts"])]
#[derive(Default)]
pub struct MeasureText {
    /// The files to read. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to read, taken exactly as written. A file piped from
    /// Get-ChildItem binds here.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
}

impl Cmdlet for MeasureText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let files = resolve_files(ps, &self.path, &self.literal_path)?;
        for (done, file) in files.iter().enumerate() {
            if ps.stopping() {
                return Ok(());
            }
            ps.progress(1, "Measuring text", &file_name(file), (done * 100 / files.len()) as i32)?;
            if let Some(text) = read_text(ps, file)? {
                ps.write(TextFacts::of(file, &text))?;
            }
        }
        ps.progress(1, "Measuring text", "done", -1)
    }
}

/// Reads files into a word index stored in Rust.
///
/// Writes one FileFacts.WordIndex after the last file. Its Find method
/// says where a word appears.
///
/// # Examples
/// $index = New-FileFactsIndex *.txt; $index.Find('apple')
#[cmdlet(verb = "New", noun = "FileFactsIndex", default_parameter_set = "Path", output = ["FileFacts.WordIndex"])]
#[derive(Default)]
pub struct NewIndex {
    /// The files to read. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to read, taken exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
    /// The index the files are read into.
    index: WordIndex,
}

impl Cmdlet for NewIndex {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for file in resolve_files(ps, &self.path, &self.literal_path)? {
            if let Some(text) = read_text(ps, &file)? {
                self.index.add(&file_name(&file), &text);
            }
        }
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(std::mem::take(&mut self.index))
    }
}

/// Finds the lines of text files that match a pattern or a script block.
///
/// -Pattern is a .NET regular expression, compared without regard to
/// case as PowerShell's -match is. -FilterScript is a script block that
/// gets each line as its first argument, and a line passes when it
/// returns true. Given both, a line must pass both.
///
/// # Examples
/// Find-FileFactsLine *.txt -Pattern '^b'
/// Find-FileFactsLine *.txt -FilterScript { param($line) $line.Length -gt 5 }
#[cmdlet(verb = "Find", noun = "FileFactsLine", default_parameter_set = "Path", output = ["FileFacts.LineMatch"])]
#[derive(Default)]
pub struct FindLine {
    /// The files to read. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to read, taken exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
    /// A .NET regular expression a line must match.
    #[param(position = 1)]
    pub pattern: Option<String>,
    /// A script block that gets each line and returns whether it passes.
    #[param]
    pub filter_script: Option<PsScriptBlock>,
}

impl Cmdlet for FindLine {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let regex = match &self.pattern {
            Some(pattern) => Some(PsType::from_name("System.Text.RegularExpressions.Regex").new(&[format!("(?i){pattern}").into_ps()?])?),
            None => None,
        };
        for file in resolve_files(ps, &self.path, &self.literal_path)? {
            let Some(text) = read_text(ps, &file)? else { continue };
            for (number, line) in text.lines().enumerate() {
                if let Some(regex) = &regex
                    && !bool::from_ps(&regex.call("IsMatch", &[line.into_ps()?])?)?
                {
                    continue;
                }
                if let Some(block) = &self.filter_script {
                    let answer = block.call(ps, &[line.into_ps()?])?;
                    let keep = match answer.first() {
                        Some(first) => bool::from_ps(first)?,
                        None => false,
                    };
                    if !keep {
                        continue;
                    }
                }
                ps.write(LineMatch { file: file_name(&file), line: number as i64 + 1, text: line.to_string() })?;
            }
        }
        Ok(())
    }
}

/// Replaces text in files.
///
/// -Replace is a hashtable: each key is text to find, compared exactly,
/// and its value is the text to put in its place. A file is written only
/// when something in it changes. Under -WhatIf it only says what it would
/// do; under -Confirm it prompts before each file.
///
/// # Examples
/// Update-FileFactsText *.txt -Replace @{ teh = 'the' } -WhatIf
#[cmdlet(verb = "Update", noun = "FileFactsText", supports_should_process, confirm_impact = "Medium", default_parameter_set = "Path")]
#[derive(Default)]
pub struct UpdateText {
    /// The files to change. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to change, taken exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
    /// What to replace: each key is the text to find, and its value the
    /// text to put in its place.
    #[param(mandatory)]
    pub replace: PsHashtable,
}

impl Cmdlet for UpdateText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let mut pairs = Vec::new();
        for key in self.replace.keys()? {
            let value = String::from_ps(&self.replace.get(&key)?)?;
            pairs.push((key, value));
        }
        for file in resolve_files(ps, &self.path, &self.literal_path)? {
            let Some(mut text) = read_text(ps, &file)? else { continue };
            let mut count = 0;
            for (from, to) in &pairs {
                if from.is_empty() {
                    continue;
                }
                count += text.matches(from.as_str()).count();
                text = text.replace(from.as_str(), to);
            }
            if count > 0 && ps.should_process(&file_name(&file), &format!("Make {count} replacements"))? {
                std::fs::write(&file, text)?;
            }
        }
        Ok(())
    }
}

/// Shows how many lines each file has, in color, for a person to read.
///
/// Writes to the console rather than to the output, so nothing reaches
/// the pipeline.
///
/// # Examples
/// Show-FileFactsText *.txt
#[cmdlet(verb = "Show", noun = "FileFactsText", default_parameter_set = "Path")]
#[derive(Default)]
pub struct ShowText {
    /// The files to read. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to read, taken exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
}

impl Cmdlet for ShowText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for file in resolve_files(ps, &self.path, &self.literal_path)? {
            let Some(text) = read_text(ps, &file)? else { continue };
            let lines = text.lines().count();
            let color = if lines == 0 { ConsoleColor::Yellow } else { ConsoleColor::Cyan };
            ps.write_host(&format!("{:<12}", file_name(&file)), Some(color), None, true)?;
            ps.write_host(&format!("{lines} lines"), None, None, false)?;
        }
        Ok(())
    }
}

/// Removes empty files.
///
/// Under -WhatIf it only says which files it would remove; under
/// -Confirm it prompts before each one.
///
/// # Examples
/// Remove-FileFactsEmpty *.txt -WhatIf
/// Get-ChildItem -File | Remove-FileFactsEmpty
#[cmdlet(verb = "Remove", noun = "FileFactsEmpty", supports_should_process, confirm_impact = "Medium", default_parameter_set = "Path")]
#[derive(Default)]
pub struct RemoveEmpty {
    /// The files to look at. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to look at, taken exactly as written.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
}

impl Cmdlet for RemoveEmpty {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for file in resolve_files(ps, &self.path, &self.literal_path)? {
            if std::fs::metadata(&file)?.len() == 0 && ps.should_process(&file_name(&file), "Remove the empty file")? {
                std::fs::remove_file(&file)?;
            }
        }
        Ok(())
    }
}

/// The files `-Path` or `-LiteralPath` names, whichever was bound. A name
/// that matches no file gets an error of its own, and the rest go on.
fn resolve_files(ps: &Pipeline<'_>, path: &[String], literal_path: &[String]) -> PsResult<Vec<String>> {
    let literal = ps.parameter_is_bound("LiteralPath");
    let names = if literal { literal_path } else { path };
    let mut files = Vec::new();
    for name in names {
        match ps.resolve_path(name, literal) {
            Ok(found) => files.extend(found),
            Err(cause) => ps.write_error(
                &PsError::new(ErrorCategory::ObjectNotFound, "FileNotFound", format!("cannot find {name}"))
                    .with_target(name.as_str().into_ps()?)
                    .with_details(cause.message),
            )?,
        }
    }
    Ok(files)
}

/// The text of `file`, or `None` once an error saying why it could not be
/// read has been written. Warns about an empty file.
fn read_text(ps: &Pipeline<'_>, file: &str) -> PsResult<Option<String>> {
    let name = file_name(file);
    pwrs::verbose!(ps, "reading {name}")?;
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) => {
            let error = match e.kind() {
                std::io::ErrorKind::NotFound => PsError::new(ErrorCategory::ObjectNotFound, "FileNotFound", format!("cannot find {name}")),
                std::io::ErrorKind::InvalidData => PsError::new(ErrorCategory::InvalidData, "NotText", format!("{name} is not UTF-8 text")),
                _ => PsError::from(e),
            };
            ps.write_error(&error.with_target(name.as_str().into_ps()?))?;
            return Ok(None);
        }
    };
    if text.is_empty() {
        pwrs::warning!(ps, "{name} is empty")?;
    }
    Ok(Some(text))
}

/// The last part of a path: its file name.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned())
}

pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [MeasureText, NewIndex, FindLine, UpdateText, ShowText, RemoveEmpty],
    classes: [TextFacts, LineMatch, WordPlace, WordIndex],
    enums: [LineEnding],
}
```

</details>

Next: [Help and the binder](../help-and-the-binder/).

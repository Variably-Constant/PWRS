---
title: Long Work
weight: 7
---

Some work takes long enough that a person waits for it: hashing a folder of large files is one. A Rust module can spread such work over every processor, show how far it has got, stop when the person presses Ctrl+C, or go on working after the command has returned and say when it is done. This chapter adds `Get-FileFactsHash`, which hashes files with SHA-256 on a pool of threads, two commands that use its hashes to find and remove duplicate files, and `Start-FileFactsHash`, which hashes in the background and raises an event at the end.

The examples use three files, two of them the same:

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
Set-Content copy.txt "apple`nbanana`ncherry`n" -NoNewline
Set-Content veg.txt "kale`nleek`n" -NoNewline
```

## A crate for the hashing

Rust's `sha2` crate does the hashing. A module adds crates the way any Rust crate does, under `[dependencies]` in `Cargo.toml`:

```toml
[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.8.0" }
sha2 = "0.11.1"
```

`cargo pwrs build` copies the license of every crate the module links into a notices file beside its library, and refuses to build when a crate declares no license or one outside the list a module allows; [Shipping](../shipping/) says more.

## Collect first, then work in parallel

When files are piped in, `process` sees them one at a time, which leaves nothing to spread over the processors. So `Get-FileFactsHash` only collects them in `process`, and hashes them all in `end`:

```rust
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let found = resolve_files(ps, &self.path, &self.literal_path)?;
        self.files.extend(found);
        Ok(())
    }
```

```rust
        ps.par_map_with(files, Order::Input, move |file, w| {
            let hashed = hash_file(&file, || w.stopping());
            let count = done.fetch_add(1, Ordering::Relaxed) + 1;
            w.write_progress(Progress::new(1, "Hashing files", format!("{count} of {total}")).with_percent((count * 100 / total) as i32));
            match hashed {
                Ok(Some(hash)) => vec![hash],
                Ok(None) => Vec::new(),
                Err(e) => {
                    w.write_error(PsError::new(ErrorCategory::ReadError, "FileNotRead", format!("cannot read {}: {e}", file_name(&file))));
                    Vec::new()
                }
            }
        })?;
```

`par_map_with` runs the function once for each item on a pool of threads, one for each processor the computer has but never more than there are items, and writes what each call returns. `Order::Input` writes the results in the order the items came in, no matter which finishes first; `Order::AsReady` would write each as it finishes. A returned `Vec` is written one element at a time, so an empty one writes nothing, which is how a file that could not be read leaves no result.

## Talking from a worker thread

PowerShell lets only the pipeline thread write to its streams. A worker thread writes through the `Worker` it is given instead: `w.write_progress`, `w.write_error`, `w.warning`, `w.verbose` and the rest send their record to the pipeline thread, which writes it. Each worker counts the files done in an `AtomicUsize` they share, so the progress bar says how many of all the files are finished. Progress sent faster than the console draws it costs nothing extra: of the records waiting at one time, only the latest for each bar is written. After `par_map_with` returns, the pipeline thread ends the bar itself:

```rust
        if total > 0 {
            ps.write_progress(&Progress::new(1, "Hashing files", "done").completed())?;
        }
```

## Stopping in the middle of a file

A command that runs workers does not end until every one of them has returned, so a worker partway through a large file would make Ctrl+C wait until it reached the end. `hash_file` reads a megabyte at a time instead, and before each read calls `stop`, the check it is given. `Get-FileFactsHash` passes it `|| w.stopping()`, which returns true soon after the person presses Ctrl+C, and the worker then stops reading the file. `par_map_with` starts no new file after a stop:

```rust
fn hash_file(path: &str, stop: impl Fn() -> bool) -> std::io::Result<Option<FileHash>> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0i64;
    loop {
        if stop() {
            return Ok(None);
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as i64;
    }
    let hash = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(Some(FileHash { name: file_name(path), path: path.to_string(), bytes, hash }))
}
```

```powershell
Get-FileFactsHash fruit.txt, copy.txt, veg.txt
```

```text
Name      Bytes Hash
----      ----- ----
fruit.txt    20 6883ada0490e1bd845b6032e95119d522212f98b840eef466ebb09f4a9eb7a03
copy.txt     20 6883ada0490e1bd845b6032e95119d522212f98b840eef466ebb09f4a9eb7a03
veg.txt      10 078dde30f7246e7127610d453bbfa8e90684398d2313b54b966c6e7525537d81
```

![PowerShell 7 showing the three files' sizes and hashes as a table, its header in green](/images/long-work-hash.png)

## The module's own objects coming back in

`Find-FileFactsDuplicate` takes the `FileFacts.FileHash` objects `Get-FileFactsHash` writes. A parameter can be of one of the module's own classes; PowerShell passes the object in, and Rust reads it property by property:

```rust
    /// A file's hash, from Get-FileFactsHash.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub input_object: FileHash,
```

It groups the files by hash as they come down the pipeline and, in `end`, writes one `FileFacts.DuplicateGroup` for each hash two or more files share:

```powershell
Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate
```

```text
Count Names                 Hash
----- -----                 ----
    2 {fruit.txt, copy.txt} 6883ada0490e1bd845b6032e95119d522212f98b840eef466ebb09f4a9eb7a03
```

![PowerShell 7 showing the one set of duplicates as a table, its header in green](/images/long-work-duplicate.png)

`Remove-FileFactsDuplicate` takes those groups and removes every file but the first of each. Removing files cannot be undone, so its `confirm_impact` is `High`: PowerShell prompts for confirmation before each file unless the caller adds `-Confirm:$false`, and under `-WhatIf` the command only says what it would remove:

```powershell
Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -WhatIf
Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -Confirm:$false
Get-ChildItem -Name
```

```text
What if: Performing the operation "Remove the copy of fruit.txt" on target "copy.txt".
fruit.txt
veg.txt
```

## Work that outlives the command

The console is busy with `Get-FileFactsHash` until the last file is hashed. `Start-FileFactsHash` returns at once: it passes the files to a thread of its own and, when the last one is done, raises a PowerShell engine event, a notice the session stores for scripts to read.

`ps.events()` gives the engine events of the session the command runs in. Unlike the pipeline, the `PsEvents` it returns can move to another thread and raise events from there, after the command has returned too:

```rust
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let files = std::mem::take(&mut self.files);
        let events = ps.events()?;
        std::thread::spawn(move || {
            let mut run = HashRun::default();
            for file in files {
                match hash_file(&file, || false) {
                    Ok(Some(hash)) => run.hashes.push(hash),
                    Ok(None) => break,
                    Err(e) => run.errors.push(format!("cannot read {}: {e}", file_name(&file))),
                }
            }
            if let Err(e) = events.raise("FileFacts.Hashed", run) {
                eprintln!("FileFacts: the FileFacts.Hashed event could not be raised: {}", e.message);
            }
        });
        Ok(())
    }
```

The event's name, its source identifier, is `FileFacts.Hashed`, and its `MessageData` is a `FileFacts.HashRun`: the hashes, in the order the files came in, and why any file could not be read. Once the command has returned there is no pipeline to write errors to, so the event includes them, and a raise that fails can only be printed. Ctrl+C cannot reach the thread either, so it passes `hash_file` `|| false`, a check that always returns false.

`Wait-Event` waits for the event and returns it:

```powershell
Start-FileFactsHash fruit.txt, veg.txt
$done = Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10
$done.MessageData.Hashes
```

```text
Name      Bytes Hash
----      ----- ----
fruit.txt    20 6883ada0490e1bd845b6032e95119d522212f98b840eef466ebb09f4a9eb7a03
veg.txt      10 078dde30f7246e7127610d453bbfa8e90684398d2313b54b966c6e7525537d81
```

![PowerShell 7 after the three lines: the hashes from the event, as a table with its header in green](/images/long-work-event.png)

The event stays in the session's queue after `Wait-Event` returns it, where `Get-Event` reads it again, until `Remove-Event` takes it out:

```powershell
@(Get-Event).Count
Remove-Event -SourceIdentifier FileFacts.Hashed
@(Get-Event).Count
```

```text
1
0
```

An action registered for the event with `Register-EngineEvent` runs when the event arrives, and reads it as `$Event`. It runs on the session's own thread, as soon as the session next looks for events, and an event an action takes is not queued. Here `Start-Sleep` gives the hashing time to finish before the queue is counted:

```powershell
$null = Register-EngineEvent -SourceIdentifier FileFacts.Hashed -Action { Write-Host "$($Event.MessageData.Hashes.Count) files hashed" }
Start-FileFactsHash fruit.txt, veg.txt
Start-Sleep -Seconds 1
@(Get-Event).Count
```

```text
2 files hashed
0
```

`Unregister-Event` ends the subscription, and `Remove-Job` removes the job PowerShell made for its action:

```powershell
Unregister-Event -SourceIdentifier FileFacts.Hashed
Get-Job -Name FileFacts.Hashed | Remove-Job
```

## The whole module so far

<details>
<summary>The files of the crate after this chapter</summary>

<!-- file: Cargo.toml -->
```toml
[package]
name = "file-facts"
version = "0.1.0"
edition = "2024"
# The module manifest takes Author from the first of `authors`,
# Description from `description`, ProjectUri from `repository`, and
# its tags from `keywords`, unless [package.metadata.pwrs] below sets
# the gallery's own. `cargo pwrs publish` refuses a module that has
# not set its own authors and description.
# authors = ["Your Name <you@example.com>"]
description = "FileFacts, a PowerShell module written in Rust"
keywords = ["powershell"]
# repository = "https://example.invalid/file-facts"

# Every other key a module manifest has, each under the manifest
# key's own name in kebab case, since a cargo manifest has no field
# for them, and a description and tags the gallery shows in place of
# the ones above. A key left unset is not written, so the manifest has
# no key without a value.
# [package.metadata.pwrs]
# description = "What the gallery shows about FileFacts."
# tags = ["Windows", "Linux", "MacOS"]
# license-uri = "https://example.invalid/file-facts/blob/main/license"
# release-notes = "First release."
# icon-uri = "https://example.invalid/file-facts/raw/main/icon.png"
# company = "Your Company"
# copyright = "(c) Your Name"
# prerelease = "beta1"                     # a gallery hides it from a plain install
# require-license-acceptance = true
# external-module-dependencies = ["Pester"]
# powershell-version = "7.2"               # default 5.1, what the shells target
# compatible-ps-editions = ["Core"]        # default both, one shell each
# powershell-host-name = "ConsoleHost"
# powershell-host-version = "5.1"
# dotnet-framework-version = "4.7.2"       # Windows PowerShell reads these two;
# clr-version = "4.0"                      # PowerShell Core ignores them
# processor-architecture = "Amd64"
# help-info-uri = "https://example.invalid/file-facts/help"
# default-command-prefix = "Xy"            # inserted into every exported name
# required-modules = ["Storage"]
# required-assemblies = ["System.Xml.dll"]
# scripts-to-process = ["init.ps1"]
# types-to-process = ["FileFacts.Types.ps1xml"]
# nested-modules = ["Extra.psm1"]
# dsc-resources-to-export = ["YourResource"]
# module-list = ["FileFacts"]
# file-list = ["readme.md"]

[lib]
crate-type = ["cdylib"]

[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.8.0" }
sha2 = "0.11.1"

# Whole-program LTO and one codegen unit so pwrs inlines into the
# cmdlet body. Leave panic = "unwind" as it is: pwrs catches panics at the
# boundary and reports them as errors instead of aborting the host.
[profile.release]
lto = "fat"
codegen-units = 1
panic = "unwind"
```

<!-- file: src/lib.rs -->
```rust
use pwrs::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
#[cmdlet(verb = "Measure", noun = "FileFactsText", default_parameter_set = "Path", output = ["FileFacts.TextFacts"], alias = ["mft"])]
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
        let case_matters = ps.parameter_is_bound("Case") && String::from_ps(&ps.parameter("Case")?)? == "Match";
        let options = if case_matters { "" } else { "(?i)" };
        let regex = match &self.pattern {
            Some(pattern) => Some(PsType::from_name("System.Text.RegularExpressions.Regex").new(&[format!("{options}{pattern}").into_ps()?])?),
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

/// Patterns -Pattern offers when Tab is pressed, with what each finds.
const PATTERNS: &[(&str, &str)] = &[("'^$'", "blank lines"), ("'\\d'", "lines containing a digit"), ("'\\s+$'", "lines ending in spaces"), ("TODO", "lines containing TODO")];

/// Completes -Pattern for Find-FileFactsLine with patterns for common
/// searches, each with a tooltip saying what it finds.
#[completer(cmdlet = "Find-FileFactsLine", parameter = "Pattern")]
fn complete_pattern(ctx: &CompletionContext) -> PsResult<Vec<Completion>> {
    let typed = ctx.word.trim_matches('\'');
    Ok(PATTERNS
        .iter()
        .filter(|(pattern, _)| pattern.trim_matches('\'').starts_with(typed))
        .map(|(pattern, finds)| Completion::value(*pattern).with_tooltip(format!("finds {finds}")))
        .collect())
}

/// Gives Find-FileFactsLine a -Case parameter once -Pattern is given:
/// Ignore, the default, or Match, which makes the pattern tell capital
/// letters from small ones.
#[dynamic_params(cmdlet = FindLine)]
fn find_line_parameters(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>> {
    if bound.contains("Pattern")? {
        Ok(vec![DynamicParam::string("Case").with_validate_set(["Ignore".to_string(), "Match".to_string()])])
    } else {
        Ok(Vec::new())
    }
}

/// Lets -Replace take text such as 'teh=the' as well as a table. Each
/// string becomes one key and its value, in the order given; a table is
/// passed on as it is.
#[transform(cmdlet = "Update-FileFactsText", parameter = "Replace")]
fn replace_pairs(value: &PsObject) -> PsResult<PsObject> {
    let texts = match value.type_name()?.as_str() {
        "System.String" => vec![String::from_ps(value)?],
        "System.Object[]" | "System.String[]" => Vec::<String>::from_ps(value)?,
        _not_text => return Ok(value.clone()),
    };
    let table = PsType::from_name("System.Collections.Specialized.OrderedDictionary").new(&[])?;
    for text in texts {
        let Some((from, to)) = text.split_once('=') else {
            return Err(PsError::new(ErrorCategory::InvalidArgument, "ReplaceText", format!("{text} is not of the form text=replacement")));
        };
        table.call("Add", &[from.into_ps()?, to.into_ps()?])?;
    }
    Ok(table)
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

/// A file's SHA-256 hash, from Get-FileFactsHash.
#[psclass(name = "FileFacts.FileHash", columns = ["Name", "Bytes", "Hash"])]
#[derive(Default, Clone)]
pub struct FileHash {
    /// The file's name.
    pub name: String,
    /// The file's full path.
    pub path: String,
    /// How many bytes it has.
    pub bytes: i64,
    /// Its SHA-256 hash, as 64 hexadecimal digits.
    pub hash: String,
}

/// Files with the same bytes, from Find-FileFactsDuplicate.
#[psclass(name = "FileFacts.DuplicateGroup", columns = ["Count", "Names", "Hash"])]
#[derive(Default, Clone)]
pub struct DuplicateGroup {
    /// How many files have these bytes.
    pub count: i64,
    /// Their names, in the order they came in.
    pub names: Vec<String>,
    /// Their full paths, in the same order.
    pub paths: Vec<String>,
    /// The hash they share.
    pub hash: String,
}

/// Hashes files with SHA-256, several at once.
///
/// Collects every file it is given, then hashes them on a pool of
/// threads and writes one FileFacts.FileHash for each, in the order the
/// files came in.
///
/// # Examples
/// Get-FileFactsHash *.txt
/// Get-ChildItem -Recurse -File | Get-FileFactsHash
#[cmdlet(verb = "Get", noun = "FileFactsHash", default_parameter_set = "Path", output = ["FileFacts.FileHash"])]
#[derive(Default)]
pub struct GetHash {
    /// The files to hash. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to hash, taken exactly as written. A file piped from
    /// Get-ChildItem binds here.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
    /// The files collected so far, hashed in `end`.
    files: Vec<String>,
}

impl Cmdlet for GetHash {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let found = resolve_files(ps, &self.path, &self.literal_path)?;
        self.files.extend(found);
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let files = std::mem::take(&mut self.files);
        let total = files.len();
        let done = Arc::new(AtomicUsize::new(0));
        ps.par_map_with(files, Order::Input, move |file, w| {
            let hashed = hash_file(&file, || w.stopping());
            let count = done.fetch_add(1, Ordering::Relaxed) + 1;
            w.write_progress(Progress::new(1, "Hashing files", format!("{count} of {total}")).with_percent((count * 100 / total) as i32));
            match hashed {
                Ok(Some(hash)) => vec![hash],
                Ok(None) => Vec::new(),
                Err(e) => {
                    w.write_error(PsError::new(ErrorCategory::ReadError, "FileNotRead", format!("cannot read {}: {e}", file_name(&file))));
                    Vec::new()
                }
            }
        })?;
        if total > 0 {
            ps.write_progress(&Progress::new(1, "Hashing files", "done").completed())?;
        }
        Ok(())
    }
}

/// The SHA-256 hash of `path`, read a megabyte at a time, or `None` when
/// `stop` returns true before the end. `stop` is called before each read.
fn hash_file(path: &str, stop: impl Fn() -> bool) -> std::io::Result<Option<FileHash>> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0i64;
    loop {
        if stop() {
            return Ok(None);
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as i64;
    }
    let hash = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(Some(FileHash { name: file_name(path), path: path.to_string(), bytes, hash }))
}

/// Finds files with the same bytes, from the hashes piped in.
///
/// Takes the FileFacts.FileHash objects Get-FileFactsHash writes and
/// writes one FileFacts.DuplicateGroup for each hash two or more files
/// share, in the order each hash first came in.
///
/// # Examples
/// Get-FileFactsHash *.txt | Find-FileFactsDuplicate
#[cmdlet(verb = "Find", noun = "FileFactsDuplicate", output = ["FileFacts.DuplicateGroup"])]
#[derive(Default)]
pub struct FindDuplicate {
    /// A file's hash, from Get-FileFactsHash.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub input_object: FileHash,
    /// The groups so far, in the order each hash first came in.
    groups: Vec<DuplicateGroup>,
    /// Where each hash's group is in `groups`.
    by_hash: HashMap<String, usize>,
}

impl Cmdlet for FindDuplicate {
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        let file = &self.input_object;
        match self.by_hash.get(&file.hash) {
            Some(&at) => {
                let group = &mut self.groups[at];
                group.count += 1;
                group.names.push(file.name.clone());
                group.paths.push(file.path.clone());
            }
            None => {
                self.by_hash.insert(file.hash.clone(), self.groups.len());
                self.groups.push(DuplicateGroup { count: 1, names: vec![file.name.clone()], paths: vec![file.path.clone()], hash: file.hash.clone() });
            }
        }
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for group in std::mem::take(&mut self.groups) {
            if group.count > 1 {
                ps.write(group)?;
            }
        }
        Ok(())
    }
}

/// Removes duplicate files, leaving the first file of each group.
///
/// Takes the FileFacts.DuplicateGroup objects Find-FileFactsDuplicate
/// writes. It prompts before removing each file unless -Confirm:$false is
/// given; under -WhatIf it only says which files it would remove.
///
/// # Examples
/// Get-FileFactsHash *.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -WhatIf
#[cmdlet(verb = "Remove", noun = "FileFactsDuplicate", supports_should_process, confirm_impact = "High")]
#[derive(Default)]
pub struct RemoveDuplicate {
    /// A group of files with the same bytes, from
    /// Find-FileFactsDuplicate.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub input_object: DuplicateGroup,
}

impl Cmdlet for RemoveDuplicate {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let group = &self.input_object;
        let Some(kept) = group.names.first() else { return Ok(()) };
        for (name, path) in group.names.iter().zip(&group.paths).skip(1) {
            if ps.should_process(name, &format!("Remove the copy of {kept}"))? {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

/// What a run of Start-FileFactsHash found, the MessageData of the
/// FileFacts.Hashed event it raises at the end.
#[psclass(name = "FileFacts.HashRun")]
#[derive(Default, Clone)]
pub struct HashRun {
    /// The hash of each file that was read, in the order the files came in.
    pub hashes: Vec<FileHash>,
    /// Why each file that could not be read has no hash.
    pub errors: Vec<String>,
}

/// Hashes files with SHA-256 in the background.
///
/// Returns at once and hashes the files on a thread of its own, one
/// after another. When the last one is done it raises the engine event
/// FileFacts.Hashed, whose MessageData is a FileFacts.HashRun with the
/// hashes and why any file could not be read. Wait-Event and Get-Event
/// read the event, and an action registered for it with
/// Register-EngineEvent runs when it arrives. Ctrl+C does not stop the
/// work, since the command has already returned.
///
/// # Examples
/// Start-FileFactsHash *.txt; (Wait-Event -SourceIdentifier FileFacts.Hashed).MessageData.Hashes
#[cmdlet(verb = "Start", noun = "FileFactsHash", default_parameter_set = "Path")]
#[derive(Default)]
pub struct StartHash {
    /// The files to hash. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to hash, taken exactly as written. A file piped from
    /// Get-ChildItem binds here.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
    /// The files collected so far, passed to the thread in `end`.
    files: Vec<String>,
}

impl Cmdlet for StartHash {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let found = resolve_files(ps, &self.path, &self.literal_path)?;
        self.files.extend(found);
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let files = std::mem::take(&mut self.files);
        let events = ps.events()?;
        std::thread::spawn(move || {
            let mut run = HashRun::default();
            for file in files {
                match hash_file(&file, || false) {
                    Ok(Some(hash)) => run.hashes.push(hash),
                    Ok(None) => break,
                    Err(e) => run.errors.push(format!("cannot read {}: {e}", file_name(&file))),
                }
            }
            if let Err(e) = events.raise("FileFacts.Hashed", run) {
                eprintln!("FileFacts: the FileFacts.Hashed event could not be raised: {}", e.message);
            }
        });
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
    cmdlets: [MeasureText, NewIndex, FindLine, UpdateText, GetHash, FindDuplicate, RemoveDuplicate, StartHash, ShowText, RemoveEmpty],
    classes: [TextFacts, LineMatch, WordPlace, WordIndex, FileHash, DuplicateGroup, HashRun],
    enums: [LineEnding],
    completers: [complete_pattern],
    transforms: [replace_pairs],
    dynamic_params: [FindLine],
}
```

</details>

Next: [Testing](../testing/).

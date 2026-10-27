---
title: Testing
weight: 8
---

FileFacts has ten commands now, and any change from here on could break one of them. This chapter tests the module two ways: Rust unit tests for the code that needs no PowerShell, and Pester tests that run the built module in pwsh and in Windows PowerShell. `cargo pwrs test` runs both, and then reports which of the module's declarations no test exercised.

## Rust unit tests

Code that only computes, such as how `TextFacts::of` detects line endings, how `WordIndex` finds a word, or how `hash_file` stops, needs no PowerShell to test. A module tests it the way any Rust crate does, in a `#[cfg(test)]` module at the end of `src/lib.rs`, which is compiled only for tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_are_told_apart() {
        assert_eq!(TextFacts::of("a.txt", "").line_ending, LineEnding::None);
        assert_eq!(TextFacts::of("a.txt", "one line").line_ending, LineEnding::None);
        assert_eq!(TextFacts::of("a.txt", "a\nb\n").line_ending, LineEnding::Lf);
        assert_eq!(TextFacts::of("a.txt", "a\r\nb\r\n").line_ending, LineEnding::CrLf);
        assert_eq!(TextFacts::of("a.txt", "a\nb\r\n").line_ending, LineEnding::Mixed);
    }
```

`assert_eq!` compares two values and shows both when they differ, which is what the `PartialEq` and `Debug` derives on `LineEnding` let it do. Because `hash_file` takes its stop check as a function, a test can pass it one that returns true at once:

```rust
    #[test]
    fn a_stop_before_the_first_read_gives_no_hash() {
        let path = std::env::temp_dir().join(format!("file-facts-test-{}.txt", std::process::id()));
        std::fs::write(&path, "apple\nbanana\ncherry\n").expect("write the sample");
        let name = path.to_string_lossy().into_owned();
        let read = hash_file(&name, || false);
        let stopped = hash_file(&name, || true);
        std::fs::remove_file(&path).expect("remove the sample");
        let hash = read.expect("read the sample").expect("no stop was requested");
        assert_eq!(hash.hash, "6883ada0490e1bd845b6032e95119d522212f98b840eef466ebb09f4a9eb7a03");
        assert_eq!(hash.bytes, 20);
        assert!(stopped.expect("open the sample").is_none());
    }
```

The four tests are in the files at the end of this chapter. `cargo test` runs them at the same time, so they can finish in any order:

```text
running 4 tests
test tests::lines_words_and_bytes_are_counted ... ok
test tests::the_index_finds_a_word_in_any_case ... ok
test tests::line_endings_are_told_apart ... ok
test tests::a_stop_before_the_first_read_gives_no_hash ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## Pester tests

Pester is PowerShell's test framework. A test file is a PowerShell script: a `Describe` or `Context` block groups tests, each `It` block is one test, and `Should` checks a value. The `tests/FileFacts.Tests.ps1` that `cargo pwrs new` wrote still tests `Get-FileFactsGreeting`, the command chapter 1 replaced, so this chapter replaces it with tests of FileFacts' own commands. The new file starts like this:

```ps1
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'FileFacts.psd1') -Force -ErrorAction Stop
}

Describe 'FileFacts' {
    # Each test starts in Pester's TestDrive folder, with the same two
    # files written fresh.
    BeforeEach {
        Push-Location -LiteralPath $TestDrive
        Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
        Set-Content veg.txt "kale`nleek`n" -NoNewline
    }

    AfterEach {
        Pop-Location
    }

    Context 'Measure-FileFactsText' {
        It 'counts the lines, words and bytes of a file and how its lines end' {
            $facts = Measure-FileFactsText fruit.txt
            $facts.Name | Should -Be 'fruit.txt'
            $facts.Lines | Should -Be 3
            $facts.Words | Should -Be 3
            $facts.Bytes | Should -Be 20
            $facts.LineEnding | Should -Be 'Lf'
        }
```

`cargo pwrs test` sets `PWRS_MODULE` to the folder of the module it built, so the file imports that build. `$TestDrive` is a folder Pester makes for the tests and removes when they are done. `BeforeEach` runs before every test in its block, and it has to be inside a `Describe`: Pester refuses one at the top of a file.

Each command has a `Context` of its own, and a few kinds of test come up in them. A command's errors are read through `-ErrorVariable`, with `-ErrorAction SilentlyContinue` so they do not show on the screen:

```ps1
            $facts = Measure-FileFactsText missing.txt, veg.txt -ErrorVariable problems -ErrorAction SilentlyContinue
            $facts.Name | Should -Be 'veg.txt'
            $problems.Count | Should -Be 1
            $problems[0].FullyQualifiedErrorId | Should -BeLike 'FileNotFound,*'
```

`Show-FileFactsText` writes to the console as `Write-Host` does, through the information stream, and `6>&1` turns what it writes into records a test can read, colors included:

```ps1
            $shown = @(Show-FileFactsText fruit.txt 6>&1)
            ($shown | ForEach-Object { "$_" }) -join '' | Should -Be 'fruit.txt   3 lines'
            $shown[0].MessageData.ForegroundColor | Should -Be 'Cyan'
```

A hash is checked against PowerShell's own `Get-FileHash` rather than against a value written into the test:

```ps1
            $hashes[0].Hash | Should -Be (Get-FileHash fruit.txt -Algorithm SHA256).Hash.ToLowerInvariant()
```

And `Start-FileFactsHash` is tested by waiting for its event, while its `Context` removes in `AfterEach` any event a test left behind:

```ps1
            Start-FileFactsHash fruit.txt, veg.txt
            $done = Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10
            $done | Should -Not -BeNullOrEmpty
            $done.MessageData.Hashes.Name | Should -Be @('fruit.txt', 'veg.txt')
            $done.MessageData.Errors | Should -BeNullOrEmpty
```

The whole file, 26 tests, is at the end of this chapter.

## Running them

`cargo pwrs test` runs `cargo test`, builds the module, and runs every `*.Tests.ps1` file under `tests/` with Pester 6.2.0, in pwsh and, on Windows, in Windows PowerShell. The first time, it downloads that Pester into the folder with the C# compiler, so there is nothing to install. With nothing left to compile, a run prints:

```text
PS> cargo pwrs test
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.02s
     Running unittests src\lib.rs (target\debug\deps\file_facts-481289612ae37c2a.exe)

running 4 tests
test tests::lines_words_and_bytes_are_counted ... ok
test tests::the_index_finds_a_word_in_any_case ... ok
test tests::line_endings_are_told_apart ... ok
test tests::a_stop_before_the_first_read_gives_no_hash ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
pwrs: file-facts declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses
pwrs: module folder C:\Temp\pwrs-tutorial\file-facts\target\pwrs\FileFacts
pwrs: Pester in pwsh

Running tests from 1 files.
What if: Performing the operation "Make 1 replacements" on target "notes.txt".
What if: Performing the operation "Remove the copy of fruit.txt" on target "copy.txt".
What if: Performing the operation "Remove the empty file" on target "empty.txt".
What if: Performing the operation "Remove the empty file" on target "empty.txt".
[+] C:\Temp\pwrs-tutorial\file-facts\tests\FileFacts.Tests.ps1 1.01s (26 tests)
Tests completed in 1.02s
Tests Passed: 26, Failed: 0, Skipped: 0, Inconclusive: 0, NotRun: 0
pwrs pester host=7.6.7 pester=6.2.0 passed=26 failed=0
pwrs: Pester in Windows PowerShell

Running tests from 1 files.
What if: Performing the operation "Make 1 replacements" on target "notes.txt".
What if: Performing the operation "Remove the copy of fruit.txt" on target "copy.txt".
What if: Performing the operation "Remove the empty file" on target "empty.txt".
What if: Performing the operation "Remove the empty file" on target "empty.txt".
[+] C:\Temp\pwrs-tutorial\file-facts\tests\FileFacts.Tests.ps1 1.24s (26 tests)
Tests completed in 1.26s
Tests Passed: 26, Failed: 0, Skipped: 0, Inconclusive: 0, NotRun: 0
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=26 failed=0
pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process
```

The `What if:` lines come from the suite's own `-WhatIf` tests, whose commands say what they would do. Each `pwrs pester` line is one host's summary: its version, Pester's version and the counts. A test that fails makes the whole command fail.

## What no test exercised

The last line is the surface check. Two things a command declares work in PowerShell whether or not the Rust code does its part. A parameter that takes pipeline input accepts whatever is piped to it, and `supports_should_process` gives a command `-WhatIf` and `-Confirm` even if its code never calls `should_process`, in which case `-WhatIf` does the very thing it should only describe. Neither shows an error, so `cargo pwrs test` records what the suite runs. It reports three things: a command no test ran, a command with `supports_should_process` that ran without ever calling `should_process`, and a parameter that takes pipeline input that nothing was ever piped into. A `literal_path` parameter is one of those: it takes the `PSPath` of what `Get-ChildItem` or `Get-Item` pipes to it.

With the one test that pipes into `Remove-FileFactsEmpty` left out, the check says:

```text
pwrs: surface check: 2 of the module's declarations were not exercised
  Remove-FileFactsEmpty takes -Path from the pipeline and nothing was ever piped into it.
  Remove-FileFactsEmpty takes -LiteralPath from the pipeline and nothing was ever piped into it.
```

A finding never fails the command. Each one is either a missing test or code that does not do what its declaration says, and only you can tell which. Here it is a missing test, and this one covers both findings:

```ps1
        It 'takes names and files from the pipeline' {
            'empty.txt' | Remove-FileFactsEmpty -WhatIf
            Get-Item empty.txt | Remove-FileFactsEmpty
            Test-Path empty.txt | Should -BeFalse
        }
```

`'empty.txt' |` pipes a name into `-Path`, and `Get-Item empty.txt |` pipes a file into `-LiteralPath`. Every `Context` in the suite has a test like it, which is why the full run reports nothing.

## The whole module so far

<details>
<summary>The files of the crate after this chapter</summary>

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_are_told_apart() {
        assert_eq!(TextFacts::of("a.txt", "").line_ending, LineEnding::None);
        assert_eq!(TextFacts::of("a.txt", "one line").line_ending, LineEnding::None);
        assert_eq!(TextFacts::of("a.txt", "a\nb\n").line_ending, LineEnding::Lf);
        assert_eq!(TextFacts::of("a.txt", "a\r\nb\r\n").line_ending, LineEnding::CrLf);
        assert_eq!(TextFacts::of("a.txt", "a\nb\r\n").line_ending, LineEnding::Mixed);
    }

    #[test]
    fn lines_words_and_bytes_are_counted() {
        let facts = TextFacts::of("fruit.txt", "apple\nbanana cherry\n");
        assert_eq!((facts.lines, facts.words, facts.bytes), (2, 3, 20));
    }

    #[test]
    fn the_index_finds_a_word_in_any_case() {
        let mut index = WordIndex::default();
        index.add("pie.txt", "Apple pie\nan apple, a tart\n");
        let found = index.find("APPLE".to_string()).expect("find");
        let places: Vec<(String, i64)> = found.into_iter().map(|p| (p.file, p.line)).collect();
        assert_eq!(places, vec![("pie.txt".to_string(), 1), ("pie.txt".to_string(), 2)]);
        assert_eq!((index.files, index.words), (1, 5));
    }

    #[test]
    fn a_stop_before_the_first_read_gives_no_hash() {
        let path = std::env::temp_dir().join(format!("file-facts-test-{}.txt", std::process::id()));
        std::fs::write(&path, "apple\nbanana\ncherry\n").expect("write the sample");
        let name = path.to_string_lossy().into_owned();
        let read = hash_file(&name, || false);
        let stopped = hash_file(&name, || true);
        std::fs::remove_file(&path).expect("remove the sample");
        let hash = read.expect("read the sample").expect("no stop was requested");
        assert_eq!(hash.hash, "6883ada0490e1bd845b6032e95119d522212f98b840eef466ebb09f4a9eb7a03");
        assert_eq!(hash.bytes, 20);
        assert!(stopped.expect("open the sample").is_none());
    }
}
```

<!-- file: tests/FileFacts.Tests.ps1 -->
```ps1
# FileFacts as it is built, run in pwsh and in Windows PowerShell.
# cargo pwrs test sets PWRS_MODULE to the module's folder.
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'FileFacts.psd1') -Force -ErrorAction Stop
}

Describe 'FileFacts' {
    # Each test starts in Pester's TestDrive folder, with the same two
    # files written fresh.
    BeforeEach {
        Push-Location -LiteralPath $TestDrive
        Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
        Set-Content veg.txt "kale`nleek`n" -NoNewline
    }

    AfterEach {
        Pop-Location
    }

    Context 'Measure-FileFactsText' {
        It 'counts the lines, words and bytes of a file and how its lines end' {
            $facts = Measure-FileFactsText fruit.txt
            $facts.Name | Should -Be 'fruit.txt'
            $facts.Lines | Should -Be 3
            $facts.Words | Should -Be 3
            $facts.Bytes | Should -Be 20
            $facts.LineEnding | Should -Be 'Lf'
        }

        It 'takes names and files from the pipeline' {
            @('fruit.txt', 'veg.txt' | Measure-FileFactsText).Lines | Should -Be @(3, 2)
            @(Get-Item fruit.txt, veg.txt | Measure-FileFactsText).Lines | Should -Be @(3, 2)
        }

        It 'reports a name that matches no file and measures the rest' {
            $facts = Measure-FileFactsText missing.txt, veg.txt -ErrorVariable problems -ErrorAction SilentlyContinue
            $facts.Name | Should -Be 'veg.txt'
            $problems.Count | Should -Be 1
            $problems[0].FullyQualifiedErrorId | Should -BeLike 'FileNotFound,*'
        }
    }

    Context 'New-FileFactsIndex' {
        It 'says where a word appears, without regard to case' {
            $index = New-FileFactsIndex fruit.txt, veg.txt
            $index.Files | Should -Be 2
            $index.Words | Should -Be 5
            $place = $index.Find('LEEK')
            $place.File | Should -Be 'veg.txt'
            $place.Line | Should -Be 2
        }

        It 'takes names and files from the pipeline' {
            ('fruit.txt' | New-FileFactsIndex).Files | Should -Be 1
            (Get-Item fruit.txt, veg.txt | New-FileFactsIndex).Files | Should -Be 2
        }
    }

    Context 'Find-FileFactsLine' {
        It 'finds the lines a pattern matches, without regard to case' {
            (Find-FileFactsLine fruit.txt -Pattern '^B').Text | Should -Be 'banana'
        }

        It 'tells capitals from small letters under -Case Match' {
            Find-FileFactsLine fruit.txt -Pattern '^B' -Case Match | Should -BeNullOrEmpty
        }

        It 'finds the lines a script block returns true for' {
            (Find-FileFactsLine fruit.txt -FilterScript { param($line) $line.Length -gt 5 }).Text | Should -Be @('banana', 'cherry')
        }

        It 'takes names and files from the pipeline' {
            ('veg.txt' | Find-FileFactsLine -Pattern '^k').Line | Should -Be 1
            (Get-Item veg.txt | Find-FileFactsLine -Pattern 'ee').Text | Should -Be 'leek'
        }
    }

    Context 'Update-FileFactsText' {
        BeforeEach {
            Set-Content notes.txt "teh cat sat`n" -NoNewline
        }

        It 'replaces text in a file' {
            Update-FileFactsText notes.txt -Replace @{ teh = 'the' }
            Get-Content notes.txt -Raw | Should -Be "the cat sat`n"
        }

        It 'changes nothing under -WhatIf' {
            Update-FileFactsText notes.txt -Replace @{ teh = 'the' } -WhatIf
            Get-Content notes.txt -Raw | Should -Be "teh cat sat`n"
        }

        It 'takes -Replace as text' {
            Update-FileFactsText notes.txt -Replace 'teh=the', 'cat=dog'
            Get-Content notes.txt -Raw | Should -Be "the dog sat`n"
        }

        It 'takes names and files from the pipeline' {
            'notes.txt' | Update-FileFactsText -Replace @{ teh = 'the' }
            Get-Item notes.txt | Update-FileFactsText -Replace @{ cat = 'dog' }
            Get-Content notes.txt -Raw | Should -Be "the dog sat`n"
        }
    }

    Context 'Get-FileFactsHash' {
        It 'hashes each file as Get-FileHash does, in the order given' {
            $hashes = Get-FileFactsHash fruit.txt, veg.txt
            $hashes.Name | Should -Be @('fruit.txt', 'veg.txt')
            $hashes[0].Hash | Should -Be (Get-FileHash fruit.txt -Algorithm SHA256).Hash.ToLowerInvariant()
            $hashes[1].Bytes | Should -Be 10
        }

        It 'takes names and files from the pipeline' {
            ('fruit.txt' | Get-FileFactsHash).Bytes | Should -Be 20
            (Get-Item veg.txt | Get-FileFactsHash).Bytes | Should -Be 10
        }
    }

    Context 'Find-FileFactsDuplicate and Remove-FileFactsDuplicate' {
        BeforeEach {
            Set-Content copy.txt "apple`nbanana`ncherry`n" -NoNewline
        }

        It 'groups the files with the same bytes' {
            $groups = @(Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate)
            $groups.Count | Should -Be 1
            $groups[0].Names | Should -Be @('fruit.txt', 'copy.txt')
        }

        It 'removes every copy but the first' {
            Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -Confirm:$false
            Test-Path copy.txt | Should -BeFalse
            Test-Path fruit.txt | Should -BeTrue
        }

        It 'removes nothing under -WhatIf' {
            Get-FileFactsHash fruit.txt, copy.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -WhatIf
            Test-Path copy.txt | Should -BeTrue
        }
    }

    Context 'Start-FileFactsHash' {
        AfterEach {
            Get-Event | Remove-Event
        }

        It 'raises FileFacts.Hashed with the hashes once they are done' {
            Start-FileFactsHash fruit.txt, veg.txt
            $done = Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10
            $done | Should -Not -BeNullOrEmpty
            $done.MessageData.Hashes.Name | Should -Be @('fruit.txt', 'veg.txt')
            $done.MessageData.Errors | Should -BeNullOrEmpty
        }

        It 'takes names and files from the pipeline' {
            'fruit.txt' | Start-FileFactsHash
            (Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10).MessageData.Hashes.Name | Should -Be 'fruit.txt'
            Remove-Event -SourceIdentifier FileFacts.Hashed
            Get-Item veg.txt | Start-FileFactsHash
            (Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10).MessageData.Hashes.Name | Should -Be 'veg.txt'
        }
    }

    Context 'Show-FileFactsText' {
        It 'writes each file and its line count to the console, the name in color' {
            $shown = @(Show-FileFactsText fruit.txt 6>&1)
            ($shown | ForEach-Object { "$_" }) -join '' | Should -Be 'fruit.txt   3 lines'
            $shown[0].MessageData.ForegroundColor | Should -Be 'Cyan'
        }

        It 'writes nothing to the output' {
            Show-FileFactsText fruit.txt 6>$null | Should -BeNullOrEmpty
        }

        It 'takes names and files from the pipeline' {
            @('veg.txt' | Show-FileFactsText 6>&1).Count | Should -Be 2
            @(Get-Item veg.txt | Show-FileFactsText 6>&1).Count | Should -Be 2
        }
    }

    Context 'Remove-FileFactsEmpty' {
        BeforeEach {
            Set-Content empty.txt '' -NoNewline
        }

        It 'removes empty files and leaves the rest' {
            Remove-FileFactsEmpty fruit.txt, empty.txt
            Test-Path empty.txt | Should -BeFalse
            Test-Path fruit.txt | Should -BeTrue
        }

        It 'removes nothing under -WhatIf' {
            Remove-FileFactsEmpty empty.txt -WhatIf
            Test-Path empty.txt | Should -BeTrue
        }

        It 'takes names and files from the pipeline' {
            'empty.txt' | Remove-FileFactsEmpty -WhatIf
            Get-Item empty.txt | Remove-FileFactsEmpty
            Test-Path empty.txt | Should -BeFalse
        }
    }
}
```

</details>

Next: [Shipping](../shipping/).

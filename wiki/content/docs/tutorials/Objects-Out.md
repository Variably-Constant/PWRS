---
title: Objects Out
weight: 4
---

So far `Measure-FileFactsText` has written lines of text such as `fruit.txt: 3`. Text is fine for a person and awkward for a script: to sort the files by length, a script would have to cut the number back out of each line. PowerShell commands write objects instead, with properties a script can sort, filter and add up. This chapter makes FileFacts write objects of its own: a class for what it measures, an enum for how lines end, and an index that stays in Rust, which a script searches through a method.

The examples use four files: two with line feeds, one with carriage returns and line feeds as Windows writes them, and one that mixes the two:

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
Set-Content veg.txt "kale`nleek`n" -NoNewline
Set-Content dos.txt "one`r`ntwo`r`n" -NoNewline
Set-Content both.txt "one`r`ntwo`nthree" -NoNewline
```

## A class for the facts

`#[psclass]` on a struct makes it a .NET class PowerShell can show, store and pass along. Each field becomes a property, named in PascalCase, and each field's `///` comment describes it:

```rust
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
```

`name` is the class's full .NET name. `columns` picks the properties PowerShell shows in its table by default, in that order; the others are still there, and `Format-List` or `Select-Object` shows them. The module's build writes these choices into a format file, `FileFacts.Format.ps1xml`, beside the manifest.

A class is copied by default. When the command writes one, every field is copied into a .NET object, and nothing of it stays in Rust: reading a property is as fast as reading any .NET object's. Writing it is one line:

```rust
            if let Some(text) = read_text(ps, file)? {
                ps.write(TextFacts::of(file, &text))?;
            }
```

`TextFacts::of` is an ordinary Rust function in its own `impl TextFacts` block that counts the lines, words and bytes and works out the line ending. The module lists every class it writes in `export_module!`:

```rust
pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [MeasureText, NewIndex, ShowText, RemoveEmpty],
    classes: [TextFacts, WordPlace, WordIndex],
    enums: [LineEnding],
}
```

```powershell
Measure-FileFactsText fruit.txt, veg.txt, dos.txt, both.txt
```

```text
Name      Lines Words Bytes LineEnding
----      ----- ----- ----- ----------
fruit.txt     3     3    20 Lf
veg.txt       2     2    10 Lf
dos.txt       2     2    10 CrLf
both.txt      3     3    14 Mixed
```

![PowerShell 7 showing the four files as a table, its header in green](/images/objects-out-table.png)

## An enum for how lines end

`#[psenum]` on a Rust enum makes it a .NET enum:

```rust
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
```

`TextFacts` derives `Default` and `Clone`, so a field of the enum's type needs the enum to derive them too, and `#[default]` marks the value `Default` gives. `Copy`, `PartialEq`, `Eq` and `Debug` make it easy to compare and print in Rust. A field of the enum's type becomes a property of the enum's type, so PowerShell shows it by name and compares it with a name.

## Objects work with PowerShell's own commands

Because the output is objects, the rest of PowerShell can work with it. `Sort-Object` sorts by any property, `Measure-Object` adds one up, and `Where-Object` filters by one, the enum compared with its name. That is why `-Total` is gone from this chapter's version: `Measure-Object` does the same for any property.

```powershell
Measure-FileFactsText fruit.txt, veg.txt | Sort-Object Lines | Select-Object -First 1 -Property Name, Lines
(Measure-FileFactsText fruit.txt, veg.txt | Measure-Object Lines -Sum).Sum
Measure-FileFactsText fruit.txt, veg.txt, dos.txt | Where-Object LineEnding -eq CrLf
```

```text
Name    Lines
----    -----
veg.txt     2

5

Name    Lines Words Bytes LineEnding
----    ----- ----- ----- ----------
dos.txt     2     2    10 CrLf
```

![PowerShell 7 after the three pipelines: a table of veg.txt's name and lines, the sum 5, and a table of dos.txt, each table's header in green](/images/objects-out-pipeline.png)

Each object's .NET type is the class, `FileFacts.TextFacts`, and its `LineEnding` property's type is the enum, `FileFacts.LineEnding`:

```powershell
$facts = Measure-FileFactsText fruit.txt
$facts.GetType().FullName
$facts.LineEnding.GetType().FullName
$facts | Format-List Name, Lines, Words, Bytes, LineEnding
```

```text
FileFacts.TextFacts
FileFacts.LineEnding

Name       : fruit.txt
Lines      : 3
Words      : 3
Bytes      : 20
LineEnding : Lf
```

![PowerShell 7 after the four lines: the two type names, then fruit.txt's facts as a list with its labels in green](/images/objects-out-list.png)

## An object that stays in Rust

Some results are too big or too busy to copy. An index of every word in a set of files is one: a script searches it now and then, and copying all of it into .NET for each search would waste the work. A proxy class leaves the value in Rust. PowerShell gets a small .NET object that points at it, and each property read or method call runs in Rust:

```rust
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
```

`mode = proxy` makes the class a proxy. `#[psfield(skip)]` hides a field from PowerShell: `places` is the index itself, and only Rust reads it. The fields a script can read must be `Clone`, because reading a property clones that field's value out of the Rust value.

`#[psmethods]` on an `impl` block makes its `pub fn`s into methods a script can call. A method that takes `&self` runs against the object's value in Rust:

```rust
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
```

`view = summary` makes the object show as the text `summary` returns, in place of a table. A copied class can have methods too, but only ones that take no `self`, since no Rust value stands behind a copied object.

The new command `New-FileFactsIndex` reads each file in `process` and writes the finished index in `end`, once:

```rust
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(std::mem::take(&mut self.index))
    }
```

```powershell
$index = New-FileFactsIndex fruit.txt, veg.txt
$index
$index.Find('BANANA')
$index.Files
```

```text
5 words in 2 files


File      Line
----      ----
fruit.txt    2

2
```

![PowerShell 7 after the four lines: 5 words in 2 files, a table of where banana is with its header in green, then 2](/images/objects-out-index.png)

The value in Rust is freed when PowerShell's garbage collector frees the object, or at once when a script calls `$index.Dispose()`.

## How an object prints inside text

`Find` returns `WordPlace` objects, a small copied class. `show` gives it a text form, used wherever PowerShell turns the object into a string, such as inside a quoted string or a property of another object:

```rust
/// One place a word appears.
#[psclass(name = "FileFacts.WordPlace", show = "{File}:{Line}")]
#[derive(Default, Clone)]
pub struct WordPlace {
    /// The file's name.
    pub file: String,
    /// The line, counted from 1.
    pub line: i64,
}
```

```powershell
"leek is at $($index.Find('leek'))"
```

```text
leek is at veg.txt:2
```

There is a third way to write an object: `mode = psobject` makes a plain `PSObject` with a note property for each field, and declares no .NET class at all. [How To Return Objects](../../how-to/how-to-return-objects/) compares the three.

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
    cmdlets: [MeasureText, NewIndex, ShowText, RemoveEmpty],
    classes: [TextFacts, WordPlace, WordIndex],
    enums: [LineEnding],
}
```

</details>

Next: [Objects in](../objects-in/).

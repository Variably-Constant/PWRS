---
title: Help And The Binder
weight: 6
---

Before a command runs, PowerShell has already done a lot for it: shown its help, completed its arguments when Tab was pressed, and bound each argument to a parameter, converting it on the way. This chapter shows where FileFacts takes part in each of those steps: help written as doc comments, an alias, completion of `-Pattern`, a parameter that appears only once another is given, and a conversion of its own for `-Replace`.

The examples use one file:

```powershell
Set-Content notes.txt "Buy teh bread.`nCall Ann about teh trip.`nBANANA bread`n" -NoNewline
```

## Help from doc comments

Every command already has help, written from its `///` comments when the module is built. The command's comment gives the synopsis, its first paragraph, and the description, everything after that. Each line under `# Examples` is an example, and each parameter's comment is that parameter's help:

```powershell
Get-Help Measure-FileFactsText
```

```text
NAME
    Measure-FileFactsText

SYNOPSIS
    Measures text files: their lines, words, bytes and line endings.


SYNTAX
    Measure-FileFactsText [-Path] <string[]> [<CommonParameters>]

    Measure-FileFactsText -LiteralPath <string[]> [<CommonParameters>]


DESCRIPTION
    Writes one FileFacts.TextFacts object for each file.


RELATED LINKS

REMARKS
    To see the examples, type: "Get-Help Measure-FileFactsText -Examples"
    For more information, type: "Get-Help Measure-FileFactsText -Detailed"
    For technical information, type: "Get-Help Measure-FileFactsText -Full"
```

![PowerShell 7 showing the help for Measure-FileFactsText: its name, synopsis, the two forms of its syntax, description and remarks](/images/help-and-the-binder-help.png)

<!-- powershell -->

```text
NAME
    Measure-FileFactsText

SYNOPSIS
    Measures text files: their lines, words, bytes and line endings.


SYNTAX
    Measure-FileFactsText [-Path] <string[]> [<CommonParameters>]

    Measure-FileFactsText -LiteralPath <string[]> [<CommonParameters>]


DESCRIPTION
    Writes one FileFacts.TextFacts object for each file.


RELATED LINKS

REMARKS
    To see the examples, type: "get-help Measure-FileFactsText -examples".
    For more information, type: "get-help Measure-FileFactsText -detailed".
    For technical information, type: "get-help Measure-FileFactsText -full".
```

![Windows PowerShell 5.1 showing the same help](/images/help-and-the-binder-help-powershell.png)

```powershell
Get-Help Find-FileFactsLine -Parameter Pattern
```

```text
-Pattern <string>
    A .NET regular expression a line must match.

    Required?                    false
    Position?                    1
    Default value                None
    Accept pipeline input?       False
    Aliases
    Accept wildcard characters?  false
```

![PowerShell 7 showing the help for -Pattern: its description, then whether it is required, its position, default, pipeline input, aliases and wildcards](/images/help-and-the-binder-parameter.png)

<!-- powershell -->

```text
-Pattern <string>
    A .NET regular expression a line must match.

    Required?                    false
    Position?                    1
    Default value                None
    Accept pipeline input?       False
    Accept wildcard characters?  false
```

![Windows PowerShell 5.1 showing the same help, which has no Aliases line](/images/help-and-the-binder-parameter-powershell.png)

`output` in `#[cmdlet(...)]` gives the help its return type, and `Get-Help -Examples` and `-Full` show the rest. The build writes all of it into a help file beside each command shell, so it works with no network and no `Update-Help`.

## An alias

`alias` in `#[cmdlet(...)]` gives the command a second name, which the module exports with it:

```rust
#[cmdlet(verb = "Measure", noun = "FileFactsText", default_parameter_set = "Path", output = ["FileFacts.TextFacts"], alias = ["mft"])]
```

```powershell
mft notes.txt
Get-Alias mft
```

```text
Name      Lines Words Bytes LineEnding
----      ----- ----- ----- ----------
notes.txt     3    10    53 Lf


CommandType     Name                                               Version    Source
-----------     ----                                               -------    ------
Alias           mft -> Measure-FileFactsText                       0.1.0      FileFacts
```

![PowerShell 7 after the two lines: notes.txt's facts as a table, then the alias mft as a table, each header in green](/images/help-and-the-binder-alias.png)

A parameter takes aliases the same way: `#[param(alias = ["p"])]`.

## Completing an argument

When Tab is pressed after a parameter, PowerShell gets from the command the values that could go there. A function marked `#[completer]` returns them for one parameter of one command:

```rust
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
```

`ctx.word` is what has been typed so far, `ctx.command` the command's name, and `ctx.bound` the other parameters already on the line. Each `Completion` is the text to put in, which must be valid PowerShell as it stands, so patterns with `$` in them include their own quotes. The module lists its completers in `export_module!`, after its enums:

```rust
    completers: [complete_pattern],
```

`TabExpansion2` is what PowerShell runs when Tab is pressed, given the line and where the cursor is, so it shows the completions without a keyboard:

```powershell
$line = 'Find-FileFactsLine notes.txt -Pattern '
(TabExpansion2 $line $line.Length).CompletionMatches | Select-Object CompletionText, ToolTip
```

```text
CompletionText ToolTip
-------------- -------
'^$'           finds blank lines
'\d'           finds lines containing a digit
'\s+$'         finds lines ending in spaces
TODO           finds lines containing TODO
```

![PowerShell 7 showing the four completions for -Pattern and their tooltips as a table, its header in green](/images/help-and-the-binder-completion.png)

## A parameter that appears when it means something

`-Case` makes a pattern tell capital letters from small ones. Without a pattern it would mean nothing, so `Find-FileFactsLine` has it only once `-Pattern` is given. A function marked `#[dynamic_params]` returns the parameters to add, based on the parameters already bound:

```rust
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
```

A dynamic parameter has no field of its own, so the command reads it by name:

```rust
        let case_matters = ps.parameter_is_bound("Case") && String::from_ps(&ps.parameter("Case")?)? == "Match";
        let options = if case_matters { "" } else { "(?i)" };
```

`with_validate_set` limits it to the values given, which PowerShell also offers on Tab. The command's type is listed under `dynamic_params` in `export_module!`:

```powershell
Find-FileFactsLine notes.txt -Pattern bread -Case Match
Find-FileFactsLine notes.txt -Case Match
```

```text
File      Line Text
----      ---- ----
notes.txt    1 Buy teh bread.
notes.txt    3 BANANA bread

Find-FileFactsLine: A parameter cannot be found that matches parameter name 'Case'.
```

![PowerShell 7 after the two commands: a table of the two lines containing bread in lowercase, its header in green, then the error for -Case in red](/images/help-and-the-binder-case.png)

<!-- powershell -->

```text
File      Line Text
----      ---- ----
notes.txt    1 Buy teh bread.
notes.txt    3 BANANA bread


Find-FileFactsLine : A parameter cannot be found that matches parameter name 'Case'.
At line:1 char:30
+ Find-FileFactsLine notes.txt -Case Match
+                              ~~~~~
    + CategoryInfo          : InvalidArgument: (:) [Find-FileFactsLine], ParameterBindingException
    + FullyQualifiedErrorId : NamedParameterNotFound,Pwrs.Modules.FileFacts.FindLineCommand
```

![Windows PowerShell 5.1 after the same two commands: the table, then the error for -Case, with its line, category and id, in red](/images/help-and-the-binder-case-powershell.png)

## Converting an argument first

The binder converts each argument to its parameter's type. A transform runs before that, and can turn an argument into something the type accepts. `-Replace` takes a table, and this one also takes text such as `'teh=the'`:

```rust
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
```

The transform gets the argument as it was given and returns what the binder should use instead. It builds an ordered table, so several pairs stay in the order given. An error it returns stops the binding, and PowerShell names the parameter. The module lists its transforms under `transforms`:

```powershell
Update-FileFactsText notes.txt -Replace 'teh=the' -WhatIf
Update-FileFactsText notes.txt -Replace 'teh=the', 'bread=rolls' -WhatIf
Update-FileFactsText notes.txt -Replace 'teh'
```

```text
What if: Performing the operation "Make 2 replacements" on target "notes.txt".
What if: Performing the operation "Make 4 replacements" on target "notes.txt".
Update-FileFactsText: Cannot process argument transformation on parameter 'Replace'. [ReplaceText] teh is not of the form text=replacement
```

![PowerShell 7 after the three commands: the two What if lines, then the error for 'teh' in red](/images/help-and-the-binder-replace.png)

<!-- powershell -->

```text
What if: Performing the operation "Make 2 replacements" on target "notes.txt".
What if: Performing the operation "Make 4 replacements" on target "notes.txt".
Update-FileFactsText : Cannot process argument transformation on parameter 'Replace'. [ReplaceText] teh is not of the
form text=replacement
At line:1 char:41
+ Update-FileFactsText notes.txt -Replace 'teh'
+                                         ~~~~~
    + CategoryInfo          : InvalidData: (:) [Update-FileFactsText], ParameterBindingArgumentTransformationException
    + FullyQualifiedErrorId : ParameterArgumentTransformationError,Pwrs.Modules.FileFacts.UpdateTextCommand
```

![Windows PowerShell 5.1 after the same three commands: the two What if lines, then the error for 'teh', with its line, category and id, in red](/images/help-and-the-binder-replace-powershell.png)

`export_module!` now lists all three kinds:

```rust
    completers: [complete_pattern],
    transforms: [replace_pairs],
    dynamic_params: [FindLine],
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
    completers: [complete_pattern],
    transforms: [replace_pairs],
    dynamic_params: [FindLine],
}
```

</details>

Next: [Long work](../long-work/).

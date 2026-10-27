---
title: Talking To The User
weight: 3
---

A command has more to say than its output: what it is doing, what looks wrong, what failed, and how far along it is. PowerShell gives each of these a stream of its own, so a script can capture the output and still see the rest, and a person can turn each one on or off. This chapter makes FileFacts use them. It also adds two commands: one that writes in color for a person to read, and one that can prompt for confirmation before it removes anything.

The examples use three files. `logo.bin` has four bytes that are not text:

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
Set-Content empty.txt "" -NoNewline
[System.IO.File]::WriteAllBytes("$PWD/logo.bin", [byte[]](0xff, 0xfe, 0x00, 0x81))
```

## Errors that let the other files go on

In chapter 2 a missing file stopped the command: `?` returned the failure, and the files after it were never read. When one input fails and the others can still be done, the PowerShell way is to report the failure and go on. The new `resolve_files` function does that for names that match nothing:

```rust
        match ps.resolve_path(name, literal) {
            Ok(found) => files.extend(found),
            Err(cause) => ps.write_error(
                &PsError::new(ErrorCategory::ObjectNotFound, "FileNotFound", format!("cannot find {name}"))
                    .with_target(name.as_str().into_ps()?)
                    .with_details(cause.message),
            )?,
        }
```

`PsError::new` takes the three things every PowerShell error has: a category, an id a script can test for, and a message for the person reading it. `with_target` names the object the error is about, here the name as it was given, and `with_details` attaches PowerShell's own explanation, which `$_.Exception.InnerException` shows. `ps.write_error` writes the error to the error stream and returns, so the loop goes on to the next name:

```powershell
Measure-FileFactsText missing.txt, fruit.txt
```

```text
Measure-FileFactsText: cannot find missing.txt
fruit.txt: 3
```

![PowerShell 7 after Measure-FileFactsText missing.txt, fruit.txt: the error in red, then fruit.txt: 3](/images/talking-to-the-user-missing.png)

<!-- powershell -->

```text
Measure-FileFactsText : cannot find missing.txt
At line:1 char:1
+ Measure-FileFactsText missing.txt, fruit.txt
+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    + CategoryInfo          : ObjectNotFound: (missing.txt:String) [Measure-FileFactsText], PwrsException
    + FullyQualifiedErrorId : FileNotFound,Pwrs.Modules.FileFacts.MeasureTextCommand

fruit.txt: 3
```

![Windows PowerShell 5.1 after the same command: the error, its line, category and id in red, then fruit.txt: 3](/images/talking-to-the-user-missing-powershell.png)

PowerShell 7 shows an error as the command's name and the message. Windows PowerShell 5.1 shows more: the line it came from, the category with the target, and the full error id, which is your id followed by the command's type.

`count_lines` does the same for a file that cannot be read, so a file that is not text is reported and the next one is still counted:

```rust
        Err(e) => {
            let error = match e.kind() {
                std::io::ErrorKind::NotFound => PsError::new(ErrorCategory::ObjectNotFound, "FileNotFound", format!("cannot find {name}")),
                std::io::ErrorKind::InvalidData => PsError::new(ErrorCategory::InvalidData, "NotText", format!("{name} is not UTF-8 text")),
                _ => PsError::from(e),
            };
            ps.write_error(&error.with_target(name.as_str().into_ps()?))?;
            return Ok(None);
        }
```

Any other failure goes through `PsError::from`, which turns a Rust `std::io::Error` into a PowerShell error: the id `IoError`, a category chosen from the kind of failure, and the system's own message. It is also what `?` does with an I/O error.

```powershell
Measure-FileFactsText logo.bin, fruit.txt
```

```text
Measure-FileFactsText: logo.bin is not UTF-8 text
fruit.txt: 3
```

![PowerShell 7 after Measure-FileFactsText logo.bin, fruit.txt: the error in red, then fruit.txt: 3](/images/talking-to-the-user-not-text.png)

<!-- powershell -->

```text
Measure-FileFactsText : logo.bin is not UTF-8 text
At line:1 char:1
+ Measure-FileFactsText logo.bin, fruit.txt
+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    + CategoryInfo          : InvalidData: (logo.bin:String) [Measure-FileFactsText], PwrsException
    + FullyQualifiedErrorId : NotText,Pwrs.Modules.FileFacts.MeasureTextCommand

fruit.txt: 3
```

![Windows PowerShell 5.1 after the same command: the error, its line, category and id in red, then fruit.txt: 3](/images/talking-to-the-user-not-text-powershell.png)

Whether an error stops anything is the caller's choice, not the command's. `-ErrorAction Stop` turns it into one that ends the command and can be caught:

```powershell
try { Measure-FileFactsText missing.txt -ErrorAction Stop } catch { "stopped by $($_.FullyQualifiedErrorId)" }
```

```text
stopped by FileNotFound,Pwrs.Modules.FileFacts.MeasureTextCommand
```

An error that must end the command whatever the caller asked for, because nothing after it can work, is made with `.terminating()` and returned from the step: `return Err(PsError::new(...).terminating());`. An error returned from a step without it is written like any other, and the step ends there.

## Saying what it is doing, and what looks wrong

The verbose stream is for a running account of what the command is doing, shown when the caller asks for it with `-Verbose`. The warning stream is for something that looks wrong but does not stop the work, and it is shown unless the caller turns it off. `count_lines` uses both:

```rust
    pwrs::verbose!(ps, "reading {name}")?;
```

```rust
    if text.is_empty() {
        pwrs::warning!(ps, "{name} is empty")?;
    }
```

`verbose!` and `warning!` take the same text as `format!`. They build the text only when it will go somewhere, so verbose messages cost almost nothing when nobody asked for them. `debug!` and `information!` work the same way for the other two streams.

```powershell
Measure-FileFactsText empty.txt, fruit.txt -Verbose
```

```text
VERBOSE: reading empty.txt
WARNING: empty.txt is empty
empty.txt: 0
VERBOSE: reading fruit.txt
fruit.txt: 3
```

![The console after Measure-FileFactsText empty.txt, fruit.txt -Verbose: the verbose and warning lines in yellow, then the two counts](/images/talking-to-the-user-verbose.png)

## A progress bar

When a command takes a while, a progress bar tells the person it is still working. `ps.progress` writes one: an id for the bar, a line saying what the work is, a line saying where it stands, and how far along it is, from 0 to 100. A negative percentage ends the bar.

```rust
        for (done, file) in files.iter().enumerate() {
            if ps.stopping() {
                return Ok(());
            }
            ps.progress(1, "Counting lines", &file_name(file), (done * 100 / files.len()) as i32)?;
```

```rust
        ps.progress(1, "Counting lines", "done", -1)
```

PowerShell draws the bar while the command runs and takes it away when it ends, so it only shows on a run long enough to see. PowerShell 7 draws it as one line under the command; Windows PowerShell 5.1 draws it as a block at the top of the window.

PowerShell passes each count down the pipeline before the command reads the next file, so a slow step after the command makes it wait at each file. Here `ForEach-Object` waits most of a second over each count, so the bar stays up long enough to see:

```powershell
Measure-FileFactsText fruit.txt, empty.txt | ForEach-Object { Start-Sleep -Milliseconds 800; $_ }
```

```text
fruit.txt: 3
WARNING: empty.txt is empty
empty.txt: 0
```

<!-- during: 1200 -->

![PowerShell 7 partway through the run: fruit.txt: 3 and the warning, and under them the bar for Counting lines at empty.txt, half done](/images/talking-to-the-user-progress.png)

<!-- powershell -->

<!-- during: 1200 -->

![Windows PowerShell 5.1 partway through the same run: the bar for Counting lines as a block across the top of the window](/images/talking-to-the-user-progress-powershell.png)

## Stopping when asked

The loop above also checks `ps.stopping()`. It turns true when the person presses Ctrl+C or the pipeline is stopped some other way, and the command should then return as soon as it can. Checking it once for each file is enough here, since one file is read quickly.

## Writing for a person

Output is for scripts: it goes down the pipeline. Sometimes a command is meant only to show something to a person, and then it writes to the console instead, the way `Write-Host` does. `ps.write_host` takes the text, a text color and a background color, each `None` for the console's own, and whether to stay on the same line:

```rust
            let color = if lines == 0 { ConsoleColor::Yellow } else { ConsoleColor::Cyan };
            ps.write_host(&format!("{:<12}", file_name(&file)), Some(color), None, true)?;
            ps.write_host(&format!("{lines} lines"), None, None, false)?;
```

`Show-FileFactsText` writes each file's name in cyan, or in yellow when the file is empty, then its line count:

```powershell
Show-FileFactsText fruit.txt, empty.txt
```

```text
fruit.txt   3 lines
WARNING: empty.txt is empty
empty.txt   0 lines
```

![The console after Show-FileFactsText fruit.txt, empty.txt: fruit.txt in cyan with 3 lines, a yellow warning that empty.txt is empty, then empty.txt in yellow with 0 lines](/images/talking-to-the-user-show.png)

Nothing goes down the pipeline, so a variable assigned from it gets nothing, while the lines still show:

```powershell
$shown = Show-FileFactsText fruit.txt
$null -eq $shown
```

```text
fruit.txt   3 lines
True
```

![The console after the two lines: fruit.txt in cyan with 3 lines, shown though the variable got nothing, then True](/images/talking-to-the-user-show-kept.png)

## Confirming before acting

A command that changes something should let the person see what it would do first, and let them confirm each change when they want to. `supports_should_process` in `#[cmdlet(...)]` gives the command PowerShell's `-WhatIf` and `-Confirm` parameters, and the body calls `ps.should_process` before each change:

```rust
#[cmdlet(verb = "Remove", noun = "FileFactsEmpty", supports_should_process, confirm_impact = "Medium", default_parameter_set = "Path")]
```

```rust
            if std::fs::metadata(&file)?.len() == 0 && ps.should_process(&file_name(&file), "Remove the empty file")? {
                std::fs::remove_file(&file)?;
            }
```

`should_process` takes what is about to change and what is about to be done to it. It returns `false` under `-WhatIf`, after saying what would have happened; under `-Confirm` it prompts the person and returns their choice; otherwise it returns `true`. `confirm_impact` says how serious the change is. PowerShell also prompts without `-Confirm` when the impact is at least as high as `$ConfirmPreference`, which is `High` unless someone changes it, so a `Medium` command prompts only when told to.

```powershell
Remove-FileFactsEmpty *.txt -WhatIf
Remove-FileFactsEmpty *.txt
Get-ChildItem -Name
```

```text
What if: Performing the operation "Remove the empty file" on target "empty.txt".
fruit.txt
logo.bin
```

## The whole module so far

`Cargo.toml` is the same as at the end of chapter 1.

<details>
<summary>src/lib.rs after this chapter</summary>

<!-- file: src/lib.rs -->
```rust
use pwrs::prelude::*;

/// Counts the lines of text files.
///
/// Writes one line for each file: its name and how many lines it has.
/// With -Total it writes only the sum, once every file has been read.
///
/// # Examples
/// Measure-FileFactsText fruit.txt, veg.txt
/// Measure-FileFactsText *.txt -Total
/// Get-ChildItem *.txt | Measure-FileFactsText
#[cmdlet(verb = "Measure", noun = "FileFactsText", default_parameter_set = "Path", output = ["System.String", "System.Int64"])]
#[derive(Default)]
pub struct MeasureText {
    /// The files to read. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
    /// The files to read, taken exactly as written. A file piped from
    /// Get-ChildItem binds here.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
    /// Write only the total number of lines, after the last file.
    #[param]
    pub total: bool,
    /// The lines counted so far, for -Total.
    sum: i64,
}

impl Cmdlet for MeasureText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let files = resolve_files(ps, &self.path, &self.literal_path)?;
        for (done, file) in files.iter().enumerate() {
            if ps.stopping() {
                return Ok(());
            }
            ps.progress(1, "Counting lines", &file_name(file), (done * 100 / files.len()) as i32)?;
            if let Some(lines) = count_lines(ps, file)? {
                if self.total {
                    self.sum += lines;
                } else {
                    ps.write(format!("{}: {lines}", file_name(file)))?;
                }
            }
        }
        ps.progress(1, "Counting lines", "done", -1)
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.total {
            ps.write(self.sum)?;
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
            let Some(lines) = count_lines(ps, &file)? else { continue };
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

/// The number of lines in `file`, or `None` once an error saying why it
/// could not be read has been written. Warns about an empty file.
fn count_lines(ps: &Pipeline<'_>, file: &str) -> PsResult<Option<i64>> {
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
    Ok(Some(text.lines().count() as i64))
}

/// The last part of a path: its file name.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned())
}

pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [MeasureText, ShowText, RemoveEmpty],
}
```

</details>

Next: [Objects out](../objects-out/).

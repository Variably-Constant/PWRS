---
title: Parameters And The Pipeline
weight: 2
---

The command from chapter 1 reads one file. This chapter lets it take many: several names at once, names with wildcards, files piped from `Get-ChildItem`, and names piped in as text. It also adds a switch, and uses the last of a command's three steps, `end`.

The examples use two small files. Make them in an empty folder:

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
Set-Content veg.txt "kale`nleek`n" -NoNewline
```

## Several files, and wildcards

A parameter whose field is a `Vec` takes a list. Change `path` to a `Vec<String>` and read each name in turn:

```rust
    /// The files to read. Wildcards are expanded.
    #[param(mandatory, position = 0, set = "Path", value_from_pipeline)]
    pub path: Vec<String>,
```

```rust
        for name in names {
            for file in ps.resolve_path(name, literal)? {
                let lines = std::fs::read_to_string(&file)?.lines().count() as i64;
                ps.write(format!("{}: {lines}", file_name(&file)))?;
            }
        }
```

`resolve_path(name, false)` lets PowerShell expand wildcards, so `*`, `?` and `[a-c]` mean what they mean to `Get-Content`, and one name can stand for many files. Each line of output names its file; `file_name` is a small function at the bottom of `src/lib.rs` that returns the last part of the path. The parts about `set`, `literal` and `names` are in the next section.

```powershell
Measure-FileFactsText fruit.txt, veg.txt
Measure-FileFactsText f*.txt
```

```text
fruit.txt: 3
veg.txt: 2
fruit.txt: 3
```

## Files from Get-ChildItem

`Get-ChildItem` writes file objects, not names. PowerShell's own file commands take them through a second parameter, `-LiteralPath`, which takes a path exactly as written, with no wildcards. FileFacts does the same:

```rust
    /// The files to read, taken exactly as written. A file piped from
    /// Get-ChildItem binds here.
    #[param(mandatory, set = "LiteralPath", literal_path)]
    pub literal_path: Vec<String>,
```

`literal_path` declares the parameter the way PowerShell declares every literal path: it gets the aliases `PSPath` and `LP`, and it takes its value from the `PSPath` property of a piped object. Every file `Get-ChildItem` writes has a `PSPath`, so a piped file binds here.

`-Path` and `-LiteralPath` must not be given together, so each belongs to a parameter set of its own, named by `set`. `default_parameter_set = "Path"` in `#[cmdlet(...)]` names the set PowerShell uses when the arguments given would fit either. The body checks which one was bound:

```rust
        let literal = ps.parameter_is_bound("LiteralPath");
        let names = if literal { &self.literal_path } else { &self.path };
```

```powershell
Get-ChildItem veg.txt | Measure-FileFactsText
```

```text
veg.txt: 2
```

A name with brackets shows the difference. To `-Path`, `[draft]` is a wildcard that matches one of the letters d, r, a, f and t; to `-LiteralPath` it is part of the name:

```powershell
Set-Content -LiteralPath '[draft].txt' "one`n" -NoNewline
Measure-FileFactsText -LiteralPath '[draft].txt'
Remove-Item -LiteralPath '[draft].txt'
```

```text
[draft].txt: 1
```

## Names from the pipeline

`value_from_pipeline` on `-Path` takes names piped in as text:

```powershell
'fruit.txt', 'veg.txt' | Measure-FileFactsText
```

```text
fruit.txt: 3
veg.txt: 2
```

`process` runs once for each name that comes down the pipeline, and `-Path` is set to that name each time.

## A switch, and the end of the pipeline

A `bool` field becomes a switch: a parameter that is on when it is named and off when it is not. `-Total` makes the command add up the lines and write only the sum:

```rust
    /// Write only the total number of lines, after the last file.
    #[param]
    pub total: bool,
    /// The lines counted so far, for -Total.
    sum: i64,
```

`sum` has no `#[param]`, so it is not a parameter. It is the command's own state: PowerShell makes one instance of the struct for each place the command appears in a pipeline, so the value of `sum` lasts from one call of `process` to the next. A command has three steps, each a method of `Cmdlet`:

| Step | Runs | Here |
|---|---|---|
| `begin` | once, before the first object | not used |
| `process` | once for each object piped in, or once when nothing is piped in | counts each file's lines, and adds them to `sum` under `-Total` |
| `end` | once, after the last object | writes `sum` under `-Total` |

`begin` and `end` do nothing unless you write them:

```rust
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.total {
            ps.write(self.sum)?;
        }
        Ok(())
    }
```

The total is the same whether the files are matched by a wildcard in one call or piped in one at a time:

```powershell
Measure-FileFactsText *.txt -Total
Get-ChildItem *.txt | Measure-FileFactsText -Total
```

```text
5
5
```

`output` in `#[cmdlet(...)]` now names both types the command can write, `System.String` and `System.Int64`.

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
        let literal = ps.parameter_is_bound("LiteralPath");
        let names = if literal { &self.literal_path } else { &self.path };
        for name in names {
            for file in ps.resolve_path(name, literal)? {
                let lines = std::fs::read_to_string(&file)?.lines().count() as i64;
                if self.total {
                    self.sum += lines;
                } else {
                    ps.write(format!("{}: {lines}", file_name(&file)))?;
                }
            }
        }
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.total {
            ps.write(self.sum)?;
        }
        Ok(())
    }
}

/// The last part of a path: its file name.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned())
}

pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [MeasureText],
}
```

</details>

Next: [Talking to the user](../talking-to-the-user/).

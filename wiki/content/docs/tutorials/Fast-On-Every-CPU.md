---
title: Fast On Every CPU
weight: 9
---

`Measure-FileFactsText` reads every byte of every file it measures. SIMD instructions (single instruction, multiple data) test 16, 32 or 64 bytes with one instruction. They come in instruction-set extensions: SSE2, AVX2 and AVX-512 on x86-64, NEON and SVE on Arm. FileFacts' library is native code, so it can use them in Windows PowerShell 5.1 and PowerShell 7 alike. Not every CPU has every extension, so PWRS chooses code by tier, a set of extensions that CPUs share. On x86-64 the tiers are the microarchitecture levels: the `x86-64` baseline that every x86-64 CPU supports, then `x86-64-v2`, `x86-64-v3` with AVX2, and `x86-64-v4` with AVX-512. On Arm they are `neon`, `dotprod`, `i8mm`, `sve` and `sve2`.

This chapter makes `Measure-FileFactsText` use SIMD while the module still loads on every CPU. A `pwrs::simd` kernel counts the lines, a `#[pwrs::tiered]` function counts a new property, `Characters`, and the tests run at every tier on one computer.

## Built for one CPU or for all of them

Rust compiles for a target, `x86_64-pc-windows-msvc` here, and by default uses only the extensions of that target's baseline, so the library runs on every x86-64 CPU. A build with `-C target-cpu=native` in `RUSTFLAGS` or in a cargo config lets the compiler use every extension of the build machine's CPU instead. `cargo pwrs build` warns about such a library and names each extension it requires beyond the baseline. On a Ryzen 9 7900X whose cargo config sets `target-cpu=native`, a build ended:

```text
PS> cargo pwrs build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.42s
pwrs: the win-x64 library is compiled to require ssse3, sse4.1, sse4.2, popcnt, avx, avx2, bmi1, bmi2, f16c, fma, lzcnt, movbe, xsave, avx512f, avx512bw, avx512cd, avx512dq, avx512vl, avx512ifma, avx512vbmi, avx512vbmi2, avx512vnni, avx512bitalg, avx512vpopcntdq, avx512bf16, gfni, vaes, vpclmulqdq, aes, pclmulqdq, sha, adx, rdrand, rdseed, sse4a, xsaveopt, xsavec, xsaves, beyond what its target assumes, and refuses to import on a CPU without them. -C target-cpu=native in RUSTFLAGS or a cargo config does this. Build with RUSTFLAGS='-C target-cpu=x86-64' for a module that loads on any x86-64 CPU, or list them under [package.metadata.pwrs] cpu-features to require them on purpose; cargo pwrs publish refuses them otherwise
pwrs: file-facts declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses
pwrs: module folder C:\Temp\pwrs-tutorial\file-facts\target\pwrs\FileFacts
```

On a CPU without one of them, the first such instruction raises an illegal-instruction fault and ends the session. So the import checks the CPU before any of the library's code runs, and refuses with the missing extensions named. `PWRS_CPU_MAX` shows the refusal on the build machine: it caps the tier the library may use, here at the `x86-64` baseline, and the import check honors the cap:

```text
PS> $env:PWRS_CPU_MAX = 'x86-64'
PS> Import-Module .\target\pwrs\FileFacts\FileFacts.psd1
Import-Module: file_facts.8df3929d6aced86-14fe00.dll was compiled for instruction-set extensions it cannot use here: PWRS_CPU_MAX=x86-64 withholds ssse3, sse4.1, sse4.2, popcnt, avx, avx2, bmi1, bmi2, f16c, fma, lzcnt, movbe, xsave, avx512f, avx512bw, avx512cd, avx512dq, avx512vl, avx512ifma, avx512vbmi, avx512vbmi2, avx512vnni, avx512bitalg, avx512vpopcntdq, avx512bf16, gfni, vaes, vpclmulqdq, aes, pclmulqdq, sha, adx, rdrand, rdseed, sse4a, xsaveopt, xsavec, xsaves. A module compiled with -C target-cpu=native, or under a cargo config that sets it, runs only on CPUs like the one that built it. Build it with RUSTFLAGS='-C target-cpu=x86-64' and choose wider kernels at run time with pwrs::cpu::has.
```

Windows PowerShell 5.1 refuses with the same message. `Remove-Item Env:PWRS_CPU_MAX` takes the cap away again.

So build a portable library by leaving `-C target-cpu` out. When your own cargo config sets it, as some do for programs that run only where they are built, set `RUSTFLAGS` for the build, which replaces the config's flags:

```text
PS> $env:RUSTFLAGS = '-C target-cpu=x86-64'
PS> cargo pwrs build
```

The rest of this chapter builds that way. The library then uses wider extensions only through runtime dispatch: code that checks the CPU at run time and calls the widest version it supports. The next two sections add it.

## A kernel already written

`pwrs::simd` has ready-made SIMD kernels. Each has a body for every tier, and every body returns what the kernel's scalar form returns, for every input. `pwrs::simd::bytes::count` counts the occurrences of a byte, and `count_scalar` is its scalar form. `TextFacts::of` in `src/lib.rs` counts the line feeds with it:

```rust
    fn of(path: &str, text: &str) -> TextFacts {
        let newlines = pwrs::simd::bytes::count(text.as_bytes(), b'\n');
        let crlf = text.matches("\r\n").count();
        let lf = newlines - crlf;
        let line_ending = match (lf, crlf) {
            (0, 0) => LineEnding::None,
            (_, 0) => LineEnding::Lf,
            (0, _) => LineEnding::CrLf,
            _ => LineEnding::Mixed,
        };
        // A last line with no line break after it counts as a line.
        let unended = usize::from(!text.is_empty() && !text.ends_with('\n'));
        TextFacts {
            name: file_name(path),
            path: path.to_string(),
            lines: (newlines + unended) as i64,
            words: text.split_whitespace().count() as i64,
            characters: characters(text.as_bytes()) as i64,
            bytes: text.len() as i64,
            line_ending,
        }
    }
```

The kernel dispatches to the widest body the CPU supports. On x86-64 that is AVX-512, else AVX2, else SSE2, which every x86-64 CPU has; on Arm it is NEON. The line count is the number of line feeds, plus one for a last line without one, the same count `text.lines()` gave before. [How To Use Instruction Sets](../../how-to/how-to-use-instruction-sets/#ready-made-kernels) lists every kernel.

## One function, every tier

For work no kernel covers, `#[pwrs::tiered]` compiles one function body once for each tier and dispatches to the widest copy the CPU supports. A new property, `Characters`, counts a file's characters. In UTF-8 a character outside ASCII takes two to four bytes, and every byte after the first is a continuation byte, from 0x80 to 0xBF. So the character count is the number of bytes that are not continuation bytes:

```rust
/// How many characters the UTF-8 text in `bytes` has: every byte except
/// the continuation bytes, 0x80 to 0xBF.
#[pwrs::tiered]
fn characters(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| !(0x80..=0xBF).contains(&b)).count()
}
```

The body is ordinary scalar Rust. Each copy is compiled with its tier's target features enabled, so the compiler may vectorize a wider tier's copy to test 16, 32 or 64 bytes per instruction. An integer count is the same in any order, so every copy returns the same count. `TextFacts` gains the property, and its default table view gains the column:

```rust
#[psclass(name = "FileFacts.TextFacts", columns = ["Name", "Lines", "Words", "Characters", "Bytes", "LineEnding"])]
```

```rust
    /// How many characters it has. A character outside ASCII takes two
    /// to four bytes.
    pub characters: i64,
```

## What it shows

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
[System.IO.File]::WriteAllText((Join-Path $PWD 'menu.txt'), "caf$([char]0xE9) cr$([char]0xE8)me`n")
Measure-FileFactsText fruit.txt, menu.txt
```

```text
Name      Lines Words Characters Bytes LineEnding
----      ----- ----- ---------- ----- ----------
fruit.txt     3     3         20    20 Lf
menu.txt      1     2         11    13 Lf
```

`[char]0xE9` is é and `[char]0xE8` is è. `[System.IO.File]::WriteAllText` writes UTF-8 without a BOM in both PowerShell 7 and Windows PowerShell 5.1, while `Set-Content` encodes non-ASCII text differently in each, so `menu.txt` has the same 13 bytes in both. Each of é and è takes two bytes, so its 11 characters are the 13 bytes less their two continuation bytes.

## Every tier gives the same count

A unit test compares both counts with Rust's `str::lines` and `str::chars`, over text long enough to fill every vector width:

```rust
    #[test]
    fn lines_and_characters_are_what_rust_counts() {
        let long = "caf\u{e9} cr\u{e8}me \u{1F34E}\r\n".repeat(40);
        for text in ["", "a", "a\n", "a\nb", "\n\n", "caf\u{e9}\n", long.as_str()] {
            let facts = TextFacts::of("a.txt", text);
            assert_eq!(facts.lines, text.lines().count() as i64, "the lines of {text:?}");
            assert_eq!(facts.characters, text.chars().count() as i64, "the characters of {text:?}");
        }
    }
```

A Pester test checks the counts for `menu.txt` through the cmdlet:

```ps1
        It 'counts fewer characters than bytes in UTF-8 text outside ASCII' {
            [System.IO.File]::WriteAllText((Join-Path $TestDrive 'menu.txt'), "caf$([char]0xE9) cr$([char]0xE8)me`n")
            $facts = Measure-FileFactsText menu.txt
            $facts.Lines | Should -Be 1
            $facts.Words | Should -Be 2
            $facts.Characters | Should -Be 11
            $facts.Bytes | Should -Be 13
        }
```

`cargo test` runs the unit test with the widest copies this CPU supports. `PWRS_CPU_MAX` caps the tier in any process the library runs in, `cargo test` included, so this runs the baseline copies:

```text
PS> $env:PWRS_CPU_MAX = 'x86-64'; cargo test; Remove-Item Env:PWRS_CPU_MAX
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.09s
     Running unittests src\lib.rs (target\debug\deps\file_facts-4c7b74dc955212c7.exe)

running 5 tests
test tests::lines_words_and_bytes_are_counted ... ok
test tests::line_endings_are_told_apart ... ok
test tests::the_index_finds_a_word_in_any_case ... ok
test tests::lines_and_characters_are_what_rust_counts ... ok
test tests::a_stop_before_the_first_read_gives_no_hash ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

`cargo pwrs test --cpu-tiers` runs the unit tests and the Pester suites in both hosts, then runs the suites again under each listed tier, with `PWRS_CPU_MAX` set to it. Its `pwrs` lines, without Pester's output between them, were:

```text
PS> cargo pwrs test --cpu-tiers x86-64-v4,x86-64-v3,x86-64
pwrs: file-facts declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses
pwrs: module folder C:\Temp\pwrs-tutorial\file-facts\target\pwrs\FileFacts
pwrs: Pester in pwsh
pwrs pester host=7.6.9 pester=6.2.0 passed=27 failed=0
pwrs: Pester in Windows PowerShell
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=27 failed=0
pwrs: Pester in pwsh under PWRS_CPU_MAX=x86-64-v4
pwrs pester host=7.6.9 pester=6.2.0 passed=27 failed=0
pwrs: Pester in Windows PowerShell under PWRS_CPU_MAX=x86-64-v4
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=27 failed=0
pwrs: Pester in pwsh under PWRS_CPU_MAX=x86-64-v3
pwrs pester host=7.6.9 pester=6.2.0 passed=27 failed=0
pwrs: Pester in Windows PowerShell under PWRS_CPU_MAX=x86-64-v3
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=27 failed=0
pwrs: Pester in pwsh under PWRS_CPU_MAX=x86-64
pwrs pester host=7.6.9 pester=6.2.0 passed=27 failed=0
pwrs: Pester in Windows PowerShell under PWRS_CPU_MAX=x86-64, Vector<T> unaccelerated
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=27 failed=0
pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process
```

A tier run also limits each host's JIT to the tier, so a module's C# stays within the tier too. For pwsh it sets .NET's `DOTNET_Enable*` switches to 0 for every extension above the tier, and refuses the run if the JIT still reports one. .NET Framework 4.8, under Windows PowerShell, has no such switches and uses AVX2 for `Vector<T>` wherever the CPU has it. So below `x86-64-v3` the run sets `COMPlus_FeatureSIMD=0`, which turns off `Vector<T>` acceleration, and its line says `Vector<T> unaccelerated`.

A `target-cpu=native` build is refused below the build machine's own tier, so a tier run needs the portable build from the start of the chapter. On Arm, `--cpu-tiers` takes `neon` through `sve2`. To run tiers on every `cargo pwrs test` without the flag, list them as `test-cpu-tiers` under `[package.metadata.pwrs]`. Each architecture refuses the other's tier names, so this suits a crate built on one architecture.

## Where to go from here

The next chapter, Shipping, packages FileFacts for the PowerShell Gallery. [How To Use Instruction Sets](../../how-to/how-to-use-instruction-sets/) covers the rest: every kernel in `pwrs::simd`, tiers written by hand with `#[target_feature]` and `pwrs::cpu::has`, and floating-point sums that give the same bits at every tier. [How To Add Hybrid C#](../../how-to/how-to-add-hybrid-csharp/#choosing-a-path-by-the-cpu) shows how the module's C# reads its Rust's tier from `Pwrs.Cpu`.

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
#[psclass(name = "FileFacts.TextFacts", columns = ["Name", "Lines", "Words", "Characters", "Bytes", "LineEnding"])]
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
    /// How many characters it has. A character outside ASCII takes two
    /// to four bytes.
    pub characters: i64,
    /// How many bytes it has.
    pub bytes: i64,
    /// How its lines end.
    pub line_ending: LineEnding,
}

impl TextFacts {
    /// The facts of the text read from `path`.
    fn of(path: &str, text: &str) -> TextFacts {
        let newlines = pwrs::simd::bytes::count(text.as_bytes(), b'\n');
        let crlf = text.matches("\r\n").count();
        let lf = newlines - crlf;
        let line_ending = match (lf, crlf) {
            (0, 0) => LineEnding::None,
            (_, 0) => LineEnding::Lf,
            (0, _) => LineEnding::CrLf,
            _ => LineEnding::Mixed,
        };
        // A last line with no line break after it counts as a line.
        let unended = usize::from(!text.is_empty() && !text.ends_with('\n'));
        TextFacts {
            name: file_name(path),
            path: path.to_string(),
            lines: (newlines + unended) as i64,
            words: text.split_whitespace().count() as i64,
            characters: characters(text.as_bytes()) as i64,
            bytes: text.len() as i64,
            line_ending,
        }
    }
}

/// How many characters the UTF-8 text in `bytes` has: every byte except
/// the continuation bytes, 0x80 to 0xBF.
#[pwrs::tiered]
fn characters(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| !(0x80..=0xBF).contains(&b)).count()
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

/// Measures text files: their lines, words, characters, bytes and line
/// endings.
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
    fn lines_and_characters_are_what_rust_counts() {
        let long = "caf\u{e9} cr\u{e8}me \u{1F34E}\r\n".repeat(40);
        for text in ["", "a", "a\n", "a\nb", "\n\n", "caf\u{e9}\n", long.as_str()] {
            let facts = TextFacts::of("a.txt", text);
            assert_eq!(facts.lines, text.lines().count() as i64, "the lines of {text:?}");
            assert_eq!(facts.characters, text.chars().count() as i64, "the characters of {text:?}");
        }
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
            $facts.Characters | Should -Be 20
            $facts.Bytes | Should -Be 20
            $facts.LineEnding | Should -Be 'Lf'
        }

        It 'counts fewer characters than bytes in UTF-8 text outside ASCII' {
            [System.IO.File]::WriteAllText((Join-Path $TestDrive 'menu.txt'), "caf$([char]0xE9) cr$([char]0xE8)me`n")
            $facts = Measure-FileFactsText menu.txt
            $facts.Lines | Should -Be 1
            $facts.Words | Should -Be 2
            $facts.Characters | Should -Be 11
            $facts.Bytes | Should -Be 13
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

---
title: How To Add Completers, Transforms And Dynamic Parameters
weight: 4
---

Tab completion computed in Rust, arguments changed before the binder coerces them, and parameters that exist only when other parameters have certain values. Source: `crates/pwrs/src/completer.rs`, `crates/pwrs/src/transform.rs`, `crates/pwrs-macros/src/completers.rs`, `crates/pwrs/src/runtime.rs` (`completer_invoke`, `transform_invoke`, `dynparams_invoke`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/CompleterBase.cs`, `TransformBase.cs` and `DynamicParametersBase.cs`.

## Argument completers

```rust
use pwrs::prelude::*;

const REGIONS: &[&str] = &["east", "west", "north"];

/// Completes -Region for Get-Site.
#[completer(cmdlet = "Get-Site", parameter = "Region")]
fn complete_region(ctx: &CompletionContext) -> PsResult<Vec<Completion>> {
    let prefix = ctx.word.to_lowercase();
    Ok(REGIONS
        .iter()
        .filter(|r| r.starts_with(&prefix))
        .map(|r| Completion::value(*r).with_tooltip(format!("the {r} region")))
        .collect())
}

pwrs::export_module! {
    name: "Sites",
    cmdlets: [GetSite],
    completers: [complete_region],
}
```

- `#[completer]` turns the function into a unit struct of the same name implementing `CompleterFn`; that name is what `export_module!` lists.
- `CompletionContext` carries `word` (the partial word under the cursor), `command` (the full command text), and `bound`, a `PsHashtable` of the parameters already bound on the line as untyped objects.
- `Completion::value(text)` makes a `ParameterValue` completion whose list item and tooltip are the text; `with_tooltip` changes the tooltip; the `kind` field takes any `CompletionKind` (`Text`, `Command`, `ProviderItem`, `ParameterName`, and the rest of `CompletionResultType`).
- The generated shell attaches `[ArgumentCompleter(typeof(<generated class>))]` to the parameter. The completer runs on the completion thread with no pipeline, so it receives no `Pipeline` token and cannot write streams.

An `Err` or a panic inside a completer crosses back as a failed status and the managed side raises it as a `PwrsException` out of `CompleteArgument`, so the completion produces nothing rather than a partial list.

## Argument transformations

```rust
#[transform(cmdlet = "Get-Size", parameter = "Size")]
fn as_bytes(value: &PsObject) -> PsResult<PsObject> {
    let text = String::from_ps(value)?;
    let trimmed = text.trim();
    let (digits, scale) = match trimmed.to_ascii_uppercase() {
        t if t.ends_with("KB") => (&trimmed[..trimmed.len() - 2], 1024_i64),
        t if t.ends_with("MB") => (&trimmed[..trimmed.len() - 2], 1024 * 1024),
        _no_suffix => return value.clone().into_ps(),
    };
    match digits.trim().parse::<i64>() {
        Ok(n) => (n * scale).into_ps(),
        Err(_not_a_number) => Err(PsError::new(ErrorCategory::InvalidArgument, "Size", format!("{trimmed} is not a size"))),
    }
}
```

```rust
pwrs::export_module! {
    name: "Demo",
    cmdlets: [GetSize],
    transforms: [as_bytes],
}
```

- The generated shell attaches an `ArgumentTransformationAttribute` to the parameter. The engine runs it **before** coercing the argument to the parameter's declared type and before validation, which is the only place this can happen: `-Size` stays a `long` and still accepts `2MB`.
- An `Err` becomes an `ArgumentTransformationMetadataException`, which the engine reports as a binding failure naming the parameter. The cmdlet body never runs. Converting in the body instead would produce an error record after the call had already started.
- Return the value unchanged for anything you do not recognize, as the `_no_suffix` arm does; the binder then coerces or rejects it as it would have without the transform.
- No cmdlet instance exists yet, so a transform takes no `Pipeline` and cannot write to a stream. One transform per parameter.
- Reach for it when the shape a caller wants to write is not a shape the parameter's type can hold. When the value is already of the right type, convert in the body: the transform buys the binder-time error, not the conversion.

## Dynamic parameters

```rust
/// Reads a value; -Unit exists only when -Kind is temperature.
#[cmdlet(verb = "Get", noun = "Reading")]
#[derive(Default)]
pub struct GetReading {
    #[param(mandatory, position = 0)]
    pub kind: String,
}

impl Cmdlet for GetReading {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let unit = if ps.parameter_is_bound("Unit") {
            String::from_ps(&ps.parameter("Unit")?)?
        } else {
            "none".to_string()
        };
        ps.write(format!("{}:{}", self.kind, unit))
    }
}

#[dynamic_params(cmdlet = GetReading)]
fn reading_dynamic_params(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>> {
    let kind = if bound.contains("Kind")? { String::from_ps(&bound.get("Kind")?)? } else { String::new() };
    if kind == "temperature" {
        Ok(vec![DynamicParam::string("Unit").with_validate_set(["C".to_string(), "F".to_string()])])
    } else {
        Ok(Vec::new())
    }
}

pwrs::export_module! {
    name: "Readings",
    cmdlets: [GetReading],
    dynamic_params: [GetReading],
}
```

- `#[dynamic_params(cmdlet = Type)]` makes the function the type's `DynamicParams` impl; list the cmdlet type under `dynamic_params` in `export_module!`, and the shell adds `IDynamicParameters` to it.
- The function runs before binding, on the pipeline thread, with the statically bound parameters as a `PsHashtable`: the engine's bound-parameter table plus every parameter whose setter ran (completion sets the properties without filling the table), names compared ignoring case, values unwrapped from `PSObject`. The table is built once per call, and a function that returns no parameters hands the engine `$null`, which it reads as no dynamic parameters.
- `DynamicParam::string(name)` builds a `System.String` parameter; the struct's fields are public, so `clr_type` (a CLR type name such as `System.Int32`), `mandatory`, `position` (`-1` for named), `set`, `help` and `validate_set` can all be set, and `mandatory()` and `with_validate_set(values)` are builders.
- Dynamic parameters are not fields of the struct. Read them inside a phase with `ps.parameter_is_bound(name)` and `ps.parameter(name)`, which consult the engine's bound-parameter table.

`examples/hello/tests/Completers.Tests.ps1` shows `-Unit` absent from `(Get-Command Get-RustReading).Parameters` until `-Kind temperature` is bound, rejected for another kind, and validated against its set.

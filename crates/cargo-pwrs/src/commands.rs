//! The cmdlets a module wraps, as the assemblies of each source declare
//! them: each PowerShell 7 release's packages for each platform, and
//! Windows PowerShell's own assemblies. `scripts/commands.ps1` reads them
//! in pwsh, through System.Reflection.Metadata, and writes JSON this reads
//! back.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{pwsh, Error};

const COMMANDS_PS1: &str = include_str!("../scripts/commands.ps1");

/// Every command read, for each source by its name, by the names the
/// request gave; `None` where a source declares no such cmdlet.
#[derive(Debug, Deserialize, Default)]
pub struct Read {
    pub sources: BTreeMap<String, BTreeMap<String, Option<Declared>>>,
}

/// One cmdlet as one source declares it. Every field is part of the
/// contract with `commands.ps1`.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Declared {
    /// Its class, by CLR name.
    pub class: String,
    /// The assembly that defines the class.
    pub assembly: String,
    /// Its `[Cmdlet]` attribute.
    pub cmdlet: Vec<Attribute>,
    /// Its `[OutputType]` attributes.
    pub output: Vec<Attribute>,
    /// The class or a base implements IDynamicParameters.
    pub dynamic: bool,
    /// The parameters of the class and its bases, each name's most
    /// derived declaration, in declaration order from the class down.
    pub parameters: Vec<Parameter>,
}

/// One parameter: a public property with `[Parameter]`.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    /// Every attribute on the property, `[Parameter]` included.
    pub attributes: Vec<Attribute>,
}

/// One attribute and its arguments, each typed.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Attribute {
    #[serde(rename = "type")]
    pub ty: TypeRef,
    #[serde(default)]
    pub args: Vec<Value>,
    #[serde(default)]
    pub named: Vec<Named>,
    /// Why its arguments could not be read, when they could not.
    #[serde(default)]
    pub error: Option<String>,
}

/// A named argument: a property or field the attribute sets.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Named {
    pub name: String,
    pub value: Value,
}

/// A type as a declaration names it.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TypeRef {
    /// The CLR name without the assembly, outer types joined by `+`.
    pub name: String,
    /// The C# text, every name rooted at `global::`.
    pub csharp: String,
    /// The defining assembly's simple name; empty for a primitive.
    pub assembly: String,
    /// The name `Type.GetType` resolves, where one can be written.
    #[serde(default)]
    pub runtime: Option<String>,
    /// A definition was found for it in the source.
    pub known: bool,
    /// Public, and nested only in public types.
    pub visible: bool,
    /// The source's compile set declares it, and every part of it, public.
    pub compile: bool,
    /// A pointer, a by-reference type or a type parameter.
    #[serde(default)]
    pub unsupported: bool,
    /// A value type: a primitive but string and object, an enum, a struct
    /// or a nullable one.
    #[serde(default, rename = "valuetype")]
    pub value_type: bool,
    #[serde(default, rename = "enum")]
    pub is_enum: bool,
}

/// An attribute argument.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Value {
    /// A null of the argument's type.
    Null {
        #[serde(rename = "type")]
        ty: TypeRef,
    },
    Bool {
        value: bool,
    },
    String {
        value: String,
    },
    Char {
        value: String,
    },
    /// A number, by its CLR type and its invariant text.
    Number {
        #[serde(rename = "type")]
        ty: String,
        text: String,
    },
    /// An enum value, by its underlying number's invariant text.
    Enum {
        #[serde(rename = "type")]
        ty: TypeRef,
        text: String,
    },
    /// A `System.Type`.
    Type {
        #[serde(rename = "type")]
        ty: TypeRef,
    },
    Array {
        element: TypeRef,
        items: Vec<Value>,
    },
}

/// One source of declarations: the assemblies searched for the commands,
/// those that only resolve the types they name, and the compile set,
/// by name, whose public types a shell can name.
pub struct Source<'a> {
    pub name: String,
    pub compile: &'a str,
    pub assemblies: &'a [PathBuf],
    pub context: &'a [PathBuf],
}

/// Declarations `installed.ps1` read from functions in a host, by command
/// name, whose types are completed against `context` and the compile set
/// named, as a source's are.
pub struct Raw<'a> {
    pub name: String,
    pub compile: &'a str,
    pub context: &'a [PathBuf],
    pub commands: serde_json::Value,
}

/// The request `commands.ps1` reads.
fn request_json(compile: &[(&str, &[PathBuf])], sources: &[Source<'_>], raws: &[Raw<'_>], commands: &[String]) -> String {
    let paths = |files: &[PathBuf]| -> Vec<String> { files.iter().map(|p| p.display().to_string()).collect() };
    let compile: Vec<serde_json::Value> =
        compile.iter().map(|(name, references)| serde_json::json!({ "name": name, "references": paths(references) })).collect();
    let sources: Vec<serde_json::Value> = sources
        .iter()
        .map(|s| serde_json::json!({ "name": s.name, "compile": s.compile, "assemblies": paths(s.assemblies), "context": paths(s.context) }))
        .collect();
    let raws: Vec<serde_json::Value> =
        raws.iter().map(|r| serde_json::json!({ "name": r.name, "compile": r.compile, "context": paths(r.context), "commands": r.commands })).collect();
    serde_json::json!({ "compile": compile, "sources": sources, "raw": raws, "commands": commands }).to_string()
}

/// Reads `commands` from each source, and completes each raw source's
/// declarations. The declarations are written to `work/commands` under a
/// stamp of the script and the request, so a build that makes the same
/// request reads them back without starting pwsh. Writing one removes those of
/// every other request.
pub fn read(work: &Path, compile: &[(&str, &[PathBuf])], sources: &[Source<'_>], raws: &[Raw<'_>], commands: &[String]) -> Result<Read, Error> {
    let request = request_json(compile, sources, raws, commands);
    let stamp = crate::generate::shell_stamp(&[COMMANDS_PS1, &request]);
    let dir = work.join("commands");
    let output = dir.join(format!("commands-{stamp}.json"));
    if !output.is_file() {
        let script = pwsh::materialize_script(&dir, "commands.ps1", COMMANDS_PS1)?;
        let request_path = dir.join(format!("commands-{stamp}.request.json"));
        std::fs::write(&request_path, &request).map_err(|e| Error::msg(format!("cannot write {}: {e}", request_path.display())))?;
        let partial = dir.join(format!("commands-{stamp}.json.{}.partial", std::process::id()));
        pwsh::run_pwsh_tool(&script, &["-Request".to_string(), request_path.display().to_string(), "-Output".to_string(), partial.display().to_string()])
            .map_err(|e| Error::msg(format!("reading the commands the module wraps:\n{e}")))?;
        std::fs::rename(&partial, &output).map_err(|e| Error::msg(format!("cannot move {} to {}: {e}", partial.display(), output.display())))?;
        remove_older(&dir, &stamp)?;
    }
    let text = std::fs::read_to_string(&output).map_err(|e| Error::msg(format!("cannot read {}: {e}", output.display())))?;
    serde_json::from_str(&text).map_err(|e| Error::msg(format!("{} is not what commands.ps1 writes: {e}", output.display())))
}

/// Removes the declarations and requests in `dir` stamped other than `stamp`.
fn remove_older(dir: &Path, stamp: &str) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("commands-") && !name.starts_with(&format!("commands-{stamp}")) {
            std::fs::remove_file(entry.path()).map_err(|e| Error::msg(format!("cannot remove {}: {e}", entry.path().display())))?;
        }
    }
    Ok(())
}

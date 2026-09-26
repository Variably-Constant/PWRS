//! The module descriptor: the JSON the proc macros embed, read back
//! from the built cdylib through `pwrs_module_descriptor`.
//!
//! Several structs carry `#[allow(dead_code)]`: a field the generator
//! does not read is still part of the contract with `pwrs-macros`.
//! `rust` is the Rust identifier, `Param::optional` is recorded though
//! boundness rides the block's bound word, and `Param::raw` though the
//! macro resolves it into `clr`.
//! `RUSTFLAGS="--force-warn dead_code"` lists them past the attribute.

use serde::Deserialize;
use std::path::Path;

use crate::Error;

#[derive(Debug, Deserialize)]
pub struct Module {
    pub abi: u32,
    pub name: String,
    pub cmdlets: Vec<Cmdlet>,
    #[serde(default)]
    pub classes: Vec<Class>,
    #[serde(default)]
    pub completers: Vec<Completer>,
    #[serde(default)]
    pub transforms: Vec<Transform>,
    #[serde(default)]
    pub providers: Vec<Provider>,
    /// Whether the module declared an import hook, which decides
    /// whether the shell implements `IModuleAssemblyInitializer`.
    #[serde(default)]
    pub on_import: bool,
    /// Whether the module declared a removal hook, which decides
    /// whether the shell implements `IModuleAssemblyCleanup`.
    #[serde(default)]
    pub on_remove: bool,
}

#[derive(Debug, Deserialize)]
pub struct Completer {
    pub id: u32,
    pub cmdlet: String,
    pub parameter: String,
}

/// One `#[transform]` function: the parameter whose argument it runs
/// over, and the id the generated attribute carries back.
#[derive(Debug, Deserialize)]
pub struct Transform {
    pub id: u32,
    pub cmdlet: String,
    pub parameter: String,
}

/// One provider as the descriptor JSON carries it. Every field is
/// part of the contract with `pwrs-macros`, read or not.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Provider {
    pub id: u32,
    pub name: String,
    pub rust: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct Class {
    pub id: u32,
    pub name: String,
    pub rust: String,
    /// `copied`, `proxy`, `psobject`, or `enum`.
    pub mode: String,
    /// A proxy class whose values report the native bytes they hold.
    #[serde(default)]
    pub native_bytes: bool,
    pub description: String,
    pub fields: Vec<Field>,
    /// Present for `enum` mode only.
    #[serde(default)]
    pub variants: Vec<Variant>,
    /// The `#[psmethods]` methods; only a proxy class may have any.
    #[serde(default)]
    pub methods: Vec<Method>,
}

/// One `#[psmethods]` method as the descriptor JSON carries it. Every
/// field is part of the contract with `pwrs-macros`, read or not.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Method {
    pub name: String,
    pub rust: String,
    pub index: u32,
    pub help: String,
    pub params: Vec<MethodParam>,
    /// `None` for a method returning nothing.
    pub ret: Option<MethodRet>,
    /// A method with no receiver: emitted as `static` on the class.
    #[serde(default, rename = "static")]
    pub is_static: bool,
    /// Takes `&mut self`: the generated method enters the object's gate
    /// exclusively. A `&self` method enters it as shared. A descriptor
    /// from a `pwrs-macros` that writes no `mutable` reads as exclusive
    /// for every method, since its methods run on a mutable reference
    /// whichever receiver they declare.
    #[serde(default = "exclusive_when_unstated")]
    pub mutable: bool,
    /// The static named `new` returning the class: emitted as the
    /// class's constructor, so a script reaches it as `[Type]::new()`.
    #[serde(default)]
    pub constructor: bool,
}

/// The receiver kind of a method whose descriptor states none.
fn exclusive_when_unstated() -> bool {
    true
}

/// One method argument as the descriptor JSON carries it.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MethodParam {
    pub name: String,
    pub rust: String,
    pub index: u32,
    pub clr: String,
    #[serde(default)]
    pub value_type: bool,
    pub slot: String,
    pub optional: bool,
}

/// A method's return type as the descriptor JSON carries it.
#[derive(Debug, Deserialize)]
pub struct MethodRet {
    pub clr: String,
    #[serde(default)]
    pub value_type: bool,
    pub optional: bool,
}

/// One member of a `#[psenum]`.
#[derive(Debug, Deserialize)]
pub struct Variant {
    pub name: String,
    pub value: i64,
    pub help: String,
}

/// One class field as the descriptor JSON carries it. Every field is
/// part of the contract with `pwrs-macros`, read or not.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Field {
    pub name: String,
    pub rust: String,
    pub index: u32,
    pub clr: String,
    /// The CLR type is a value type such as an enum, boxed without a
    /// null check.
    #[serde(default)]
    pub value_type: bool,
    pub slot: String,
    pub optional: bool,
    pub help: String,
}

#[derive(Debug, Deserialize)]
pub struct Cmdlet {
    pub id: u32,
    pub verb: String,
    pub noun: String,
    pub name: String,
    pub rust: String,
    pub should_process: bool,
    #[serde(default)]
    pub dynamic_params: bool,
    pub confirm_impact: Option<String>,
    pub default_set: Option<String>,
    pub aliases: Vec<String>,
    pub output_types: Vec<String>,
    pub synopsis: String,
    pub description: String,
    pub params: Vec<Param>,
}

/// One parameter as the descriptor JSON carries it. Every field is
/// part of the contract with `pwrs-macros`, read or not.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Param {
    pub name: String,
    pub rust: String,
    pub index: u32,
    pub clr: String,
    /// The CLR type is a value type such as an enum, boxed without a
    /// null check.
    #[serde(default)]
    pub value_type: bool,
    pub slot: String,
    pub optional: bool,
    pub mandatory: bool,
    pub position: Option<i64>,
    /// The parameter sets the parameter belongs to; empty for every set.
    #[serde(default)]
    pub sets: Vec<String>,
    /// The one set a descriptor from a pwrs-macros without `sets` names,
    /// folded into `sets` when the descriptor is read.
    #[serde(default)]
    pub set: Option<String>,
    pub pipeline: bool,
    pub pipeline_by_name: bool,
    pub remaining: bool,
    pub aliases: Vec<String>,
    pub help: String,
    pub validate_set: Vec<String>,
    pub validate_range: Option<(i64, i64)>,
    pub validate_pattern: Option<String>,
    pub not_null_or_empty: bool,
    pub dont_show: bool,
    pub literal_path: bool,
    /// Declared as `object`, so the engine's binder does not coerce
    /// the argument to the parameter's own type.
    #[serde(default)]
    pub raw: bool,
    /// Carries `[AllowEmptyCollection]`, so a mandatory collection
    /// parameter takes an empty one.
    #[serde(default)]
    pub allow_empty_collection: bool,
}

#[repr(C)]
struct RawDescriptor {
    json_utf8: *const u8,
    json_len: usize,
}

/// Loads the cdylib and reads its descriptor. The library exports
/// nothing that runs at load, so loading it here has no side effect.
pub fn read(cdylib: &Path) -> Result<Module, Error> {
    let lib = unsafe { libloading::Library::new(cdylib) }.map_err(|e| Error::msg(format!("cannot load {}: {e}", cdylib.display())))?;
    let json = unsafe {
        let f: libloading::Symbol<unsafe extern "C" fn() -> RawDescriptor> =
            lib.get(b"pwrs_module_descriptor\0").map_err(|e| Error::msg(format!("{} exports no pwrs_module_descriptor: {e}", cdylib.display())))?;
        let raw = f();
        if raw.json_utf8.is_null() {
            return Err(Error::msg("pwrs_module_descriptor returned a null pointer"));
        }
        let bytes = std::slice::from_raw_parts(raw.json_utf8, raw.json_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    let mut module: Module = serde_json::from_str(&json).map_err(|e| Error::msg(format!("descriptor is not valid JSON: {e}\n{json}")))?;
    if module.abi != 1 {
        return Err(Error::msg(format!("descriptor ABI {} is not supported by this cargo-pwrs", module.abi)));
    }
    fold_single_sets(&mut module);
    Ok(module)
}

/// Moves each parameter's `set`, the one set a descriptor from a
/// pwrs-macros without `sets` names, into `sets`, so everything after
/// the read consults `sets` alone.
fn fold_single_sets(module: &mut Module) {
    for p in module.cmdlets.iter_mut().flat_map(|c| c.params.iter_mut()) {
        if let Some(one) = p.set.take() {
            if p.sets.is_empty() {
                p.sets.push(one);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-cmdlet descriptor whose one parameter carries `sets_json`,
    /// the parameter's set keys as JSON members.
    fn with_sets(sets_json: &str) -> Module {
        let json = format!(
            r#"{{"abi": 1, "name": "Demo", "cmdlets": [{{
                "id": 0, "verb": "Get", "noun": "Thing", "name": "Get-Thing", "rust": "GetThing",
                "should_process": false, "confirm_impact": null, "default_set": null,
                "aliases": [], "output_types": [], "synopsis": "", "description": "",
                "params": [{{
                    "name": "Destination", "rust": "destination", "index": 0, "clr": "System.String",
                    "slot": "str16", "optional": true, "mandatory": false, "position": null, {sets_json}
                    "pipeline": false, "pipeline_by_name": false, "remaining": false, "aliases": [],
                    "help": "", "validate_set": [], "validate_range": null, "validate_pattern": null,
                    "not_null_or_empty": false, "dont_show": false, "literal_path": false
                }}]
            }}]}}"#
        );
        let mut module: Module = serde_json::from_str(&json).expect("descriptor");
        fold_single_sets(&mut module);
        module
    }

    #[test]
    fn a_parameter_reads_every_set_it_names_and_an_older_single_set_as_one() {
        assert_eq!(with_sets(r#""sets": ["Path", "LiteralPath"],"#).cmdlets[0].params[0].sets, ["Path", "LiteralPath"]);
        assert_eq!(with_sets(r#""set": "ByName","#).cmdlets[0].params[0].sets, ["ByName"]);
        assert!(with_sets(r#""set": null,"#).cmdlets[0].params[0].sets.is_empty());
        assert!(with_sets("").cmdlets[0].params[0].sets.is_empty());
    }
}

//! The .NET types a module's proxy classes derive from or implement, as
//! the reference assemblies of each of its shell's compiles describe
//! them. `scripts/members.ps1` reads them in pwsh, through
//! System.Reflection.Metadata, and writes JSON this reads back.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{pwsh, Error};

const MEMBERS_PS1: &str = include_str!("../scripts/members.ps1");

/// Every type read, by the name the module gave it, for each target
/// framework, by its folder name.
#[derive(Debug, Deserialize)]
pub struct Read {
    pub targets: BTreeMap<String, BTreeMap<String, TypeInfo>>,
}

/// One type, as one target's reference assemblies describe it. Every
/// field is part of the contract with `members.ps1`, read or not.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct TypeInfo {
    pub found: bool,
    /// Why the type cannot be read in full, when it cannot.
    #[serde(default)]
    pub problem: Option<String>,
    /// The type's C# name, with any type arguments.
    #[serde(default)]
    pub name: String,
    /// The reference assembly that defines it.
    #[serde(default)]
    pub assembly: String,
    /// `class`, `interface`, `struct`, `enum` or `delegate`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub sealed: bool,
    #[serde(default, rename = "abstract")]
    pub is_abstract: bool,
    /// A class's public and protected instance constructors.
    #[serde(default)]
    pub constructors: Vec<Constructor>,
    /// The public and protected members of a class and its bases, each
    /// signature once and its most derived declaration first; of an
    /// interface, its own and those of the interfaces it extends.
    #[serde(default)]
    pub members: Vec<Member>,
    /// Every interface it implements or extends, through its bases too.
    #[serde(default)]
    pub interfaces: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Constructor {
    /// `public` or `protected`.
    pub access: String,
    pub params: Vec<Param>,
}

/// One parameter of a constructor, a method or an indexer.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Param {
    pub name: String,
    /// Its C# type; a nullable value type ends in `?`.
    #[serde(rename = "type")]
    pub ty: String,
    /// The type is a value type.
    #[serde(default)]
    pub type_value: bool,
    /// The type is a pointer, a function pointer or a type parameter of
    /// the method, which no Rust type stands for.
    #[serde(default)]
    pub type_unsupported: bool,
    /// Empty, `ref`, `out` or `in`.
    pub mode: String,
    /// The parameter has a default value.
    pub optional: bool,
}

/// One member, as `members.ps1` writes it. Which fields it has depends on
/// its kind.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Member {
    /// `method`, `property`, `event`, `operator`, `field` or `type`.
    pub kind: String,
    pub name: String,
    /// The type that declares it, by its C# name with its type arguments.
    pub from: String,
    /// `public` or `protected`, for a method, an operator or an event.
    #[serde(default)]
    pub access: String,
    /// A class's method: `abstract`, `virtual`, `sealed`, `none` or
    /// `static`. An interface's: `abstract`, `default`, `static` or
    /// `static-abstract`.
    #[serde(default)]
    pub slot: String,
    /// How many type parameters the method has of its own.
    #[serde(default)]
    pub generic: u32,
    /// A method's parameters, or a property's index parameters.
    #[serde(default)]
    pub params: Vec<Param>,
    /// A method's return type, `void` for none.
    #[serde(default)]
    pub returns: String,
    #[serde(default)]
    pub returns_value: bool,
    #[serde(default)]
    pub returns_unsupported: bool,
    /// The method returns by reference.
    #[serde(default)]
    pub returns_ref: bool,
    /// A property's type.
    #[serde(default, rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub type_value: bool,
    #[serde(default)]
    pub type_unsupported: bool,
    /// A property's getter, when it has one a derived class can call.
    #[serde(default)]
    pub get: Option<Accessor>,
    /// A property's setter, when it has one a derived class can call.
    #[serde(default)]
    pub set: Option<Accessor>,
}

/// One accessor of a property.
#[derive(Debug, Deserialize, Clone)]
pub struct Accessor {
    /// `public` or `protected`.
    pub access: String,
    /// As a method's.
    pub slot: String,
    /// An init-only setter, which only an object initializer can call.
    #[serde(default)]
    pub init: bool,
}

/// The request `members.ps1` reads.
fn request_json(targets: &[(&str, &[PathBuf])], types: &[String]) -> String {
    let targets: Vec<serde_json::Value> = targets
        .iter()
        .map(|(name, references)| {
            let references: Vec<String> = references.iter().map(|p| p.display().to_string()).collect();
            serde_json::json!({ "name": name, "references": references })
        })
        .collect();
    serde_json::json!({ "targets": targets, "types": types }).to_string()
}

/// Reads `types` from each target's reference assemblies, named by the
/// target's folder name. The description is written to `work/members`
/// under a stamp of the script, the references and the types, so a build
/// that names the same ones reads it back without starting pwsh. Writing
/// one removes those of every other request.
pub fn read(work: &Path, targets: &[(&str, &[PathBuf])], types: &[String]) -> Result<Read, Error> {
    let request = request_json(targets, types);
    let stamp = crate::generate::shell_stamp(&[MEMBERS_PS1, &request]);
    let dir = work.join("members");
    let output = dir.join(format!("members-{stamp}.json"));
    if !output.is_file() {
        let script = pwsh::materialize_script(&dir, "members.ps1", MEMBERS_PS1)?;
        let request_path = dir.join(format!("members-{stamp}.request.json"));
        std::fs::write(&request_path, &request).map_err(|e| Error::msg(format!("cannot write {}: {e}", request_path.display())))?;
        let partial = dir.join(format!("members-{stamp}.json.{}.partial", std::process::id()));
        pwsh::run_pwsh_tool(&script, &["-Request".to_string(), request_path.display().to_string(), "-Output".to_string(), partial.display().to_string()])
            .map_err(|e| Error::msg(format!("reading the .NET types the classes name:\n{e}")))?;
        std::fs::rename(&partial, &output).map_err(|e| Error::msg(format!("cannot move {} to {}: {e}", partial.display(), output.display())))?;
        remove_older(&dir, &stamp)?;
    }
    let text = std::fs::read_to_string(&output).map_err(|e| Error::msg(format!("cannot read {}: {e}", output.display())))?;
    serde_json::from_str(&text).map_err(|e| Error::msg(format!("{} is not what members.ps1 writes: {e}", output.display())))
}

/// Removes the descriptions and requests in `dir` stamped other than `stamp`.
fn remove_older(dir: &Path, stamp: &str) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("members-") && !name.starts_with(&format!("members-{stamp}")) {
            std::fs::remove_file(entry.path()).map_err(|e| Error::msg(format!("cannot remove {}: {e}", entry.path().display())))?;
        }
    }
    Ok(())
}

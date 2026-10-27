//! C# shell, manifest, and bootstrap script generation from the
//! descriptor.

use crate::bases::ClassPlan;
use crate::descriptor::{Class, Cmdlet, Completer, Field, Method, MethodParam, MethodRet, Module, Param, Transform};
use crate::Error;
use std::collections::BTreeMap;

/// Rejects a descriptor the shell cannot express.
pub fn validate(m: &Module) -> Result<(), Error> {
    for (i, c) in m.classes.iter().enumerate() {
        if let Some(first) = m.classes[..i].iter().find(|other| other.name.eq_ignore_ascii_case(&c.name)) {
            return Err(Error::msg(format!(
                "{} and {} are both declared as {}; PowerShell allows one type per name, compared without regard to case, so give one of them another name",
                rust_type(first),
                rust_type(c),
                c.name
            )));
        }
    }
    for c in &m.classes {
        match c.mode.as_str() {
            "proxy" => {}
            // A copied object has its fields and no Rust value, so only
            // what runs against no value can be declared on it: its
            // constructors and its other statics.
            "copied" => {
                if let Some(method) = c.methods.iter().find(|method| !method.is_static) {
                    return Err(Error::msg(format!(
                        "class {} is copied mode, so {} cannot take &self: a copied object has no Rust value to run it against. Declare it without a receiver, or make the class mode = proxy",
                        c.name, method.rust
                    )));
                }
            }
            other if !c.methods.is_empty() => {
                return Err(Error::msg(format!(
                    "class {} declares #[psmethods] but is {other} mode; methods need mode = proxy, or statics on mode = copied",
                    c.name
                )));
            }
            _no_methods => {}
        }
        check_roles(c)?;
        // An operator returns what its method returns, and takes the class
        // as its left operand: the object an instance method is called on,
        // or a static method's first argument, which C# requires of one.
        for (key, name) in [("add", &c.add), ("sub", &c.sub), ("mul", &c.mul), ("div", &c.div), ("rem", &c.rem)] {
            let Some(m) = role(c, name) else { continue };
            let takes = m.params.len() + usize::from(!m.is_static);
            let left_is_class = !m.is_static || m.params.first().is_some_and(|p| p.clr == c.name);
            if m.ret.is_none() || takes != 2 || !left_is_class || m.params.iter().any(|p| p.slot == "task" || p.optional) {
                return Err(Error::msg(format!(
                    "class {}'s {key} names {}, which must return a value and take two operands, the class's own object first: the object an instance method is called on, or a static method's first argument. An optional or PsTask argument is not an operand",
                    c.name, m.rust
                )));
            }
        }
        for m in &c.methods {
            if m.params.iter().any(|p| p.slot == "task")
                && let Some(p) = m.params.iter().find(|p| p.name == "cancellationToken")
            {
                return Err(Error::msg(format!(
                    "class {}'s method {} takes a PsTask, so it takes the caller's CancellationToken as cancellationToken, and its argument {} has that name too; rename the argument",
                    c.name, m.rust, p.rust
                )));
            }
        }
    }
    // The macro refuses a number or bool; an enum or another value type
    // is known as one only here, from the descriptor.
    for c in &m.cmdlets {
        if let Some(p) = c.params.iter().find(|p| (p.allow_null || p.validate_not_null) && !is_reference_type(&p.clr, p.value_type)) {
            let key = if p.allow_null { "allow_null" } else { "validate_not_null" };
            return Err(Error::msg(format!(
                "{}'s parameter -{} declares {key}, and its type {} cannot be null; declare it on a parameter whose type can be, such as a string, an object or an array",
                c.name, p.name, p.clr
            )));
        }
    }
    Ok(())
}

/// The class's Rust type by its path, or by its name where the
/// descriptor has no path.
fn rust_type(c: &Class) -> &str {
    if c.path.is_empty() { &c.rust } else { &c.path }
}

/// The method a role names, which the macro has checked is a
/// `#[psmethods]` method of the class.
fn role<'a>(c: &'a Class, name: &Option<String>) -> Option<&'a Method> {
    let name = name.as_ref()?;
    c.methods.iter().find(|m| &m.name == name)
}

/// Refuses a list whose `Count` or indexer would collide with a method
/// of the same PowerShell name: a C# class can have only one member of
/// a name.
fn check_roles(c: &Class) -> Result<(), Error> {
    if c.item.is_none() {
        return Ok(());
    }
    for taken in ["Count", "Item"] {
        if let Some(m) = c.methods.iter().find(|m| m.name == taken) {
            return Err(Error::msg(format!(
                "class {} is a list, which declares {taken} through count and item, and its method {} is named {taken} in PowerShell as well; rename the method",
                c.name, m.rust
            )));
        }
    }
    Ok(())
}

/// C# verbatim string literal.
fn cs_str(s: &str) -> String {
    format!("@\"{}\"", s.replace('"', "\"\""))
}

fn cs_ident(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// The words C# reserves, which no name can be without an `@` before it.
const CS_KEYWORDS: &[&str] = &[
    "abstract", "as", "base", "bool", "break", "byte", "case", "catch", "char", "checked", "class", "const", "continue", "decimal", "default", "delegate", "do",
    "double", "else", "enum", "event", "explicit", "extern", "false", "finally", "fixed", "float", "for", "foreach", "goto", "if", "implicit", "in", "int",
    "interface", "internal", "is", "lock", "long", "namespace", "new", "null", "object", "operator", "out", "override", "params", "private", "protected",
    "public", "readonly", "ref", "return", "sbyte", "sealed", "short", "sizeof", "stackalloc", "static", "string", "struct", "switch", "this", "throw", "true",
    "try", "typeof", "uint", "ulong", "unchecked", "unsafe", "ushort", "using", "virtual", "void", "volatile", "while",
];

/// A descriptor name as C# writes it as a name: with an `@` before a word
/// C# reserves. The `@` is not part of the name, so reflection and
/// PowerShell read the name as it is in Rust.
pub(crate) fn cs_name(name: &str) -> String {
    if CS_KEYWORDS.contains(&name) { format!("@{name}") } else { name.to_string() }
}

/// Property types that may be null when unbound: everything but the
/// CLR primitives and the descriptor entries marked `value_type`.
pub(crate) fn is_reference_type(clr: &str, value_type: bool) -> bool {
    const PRIMITIVES: &[&str] = &["bool", "sbyte", "short", "int", "long", "byte", "ushort", "uint", "ulong", "float", "double", "char", "SwitchParameter"];
    !value_type && !PRIMITIVES.contains(&clr)
}

/// A descriptor CLR name as a C# type expression. A dotted name gets
/// the `global::` prefix, so it resolves the same way inside every
/// generated namespace.
pub(crate) fn cs_type(clr: &str) -> String {
    if clr.contains('.') {
        format!("global::{clr}")
    } else {
        clr.to_string()
    }
}

/// The 64-bit words a mask of `n` items takes, one bit each and never
/// fewer than one word, as the macros size the Rust side of each block.
fn mask_words(n: usize) -> usize {
    n.div_ceil(64).max(1)
}

/// The C# name of word `word` of a mask whose first word is `first`. The
/// first word keeps the name it always had, so a block of 64 items or
/// fewer is generated as it always was, and each later word adds its
/// number.
fn word_name(first: &str, word: usize) -> String {
    if word == 0 {
        first.to_string()
    } else {
        format!("{first}{word}")
    }
}

/// The word of a mask that item `i`'s bit is in, and the bit in it.
fn mask_bit(i: u32) -> (usize, u32) {
    ((i / 64) as usize, i % 64)
}

fn slot_cs(slot: &str) -> &'static str {
    match slot {
        "bool" => "byte",
        "i8" => "sbyte",
        "i16" => "short",
        "i32" => "int",
        "i64" => "long",
        "u8" => "byte",
        "u16" => "ushort",
        "u32" => "uint",
        "u64" => "ulong",
        "f32" => "float",
        "f64" => "double",
        "str16" => "Pwrs.PsStr16",
        "task" => "Pwrs.PsTaskSlot",
        _handle => "System.IntPtr",
    }
}

fn param_attributes(p: &Param) -> String {
    let mut head = Vec::new();
    if p.mandatory {
        head.push("Mandatory = true".to_string());
    }
    if let Some(pos) = p.position {
        head.push(format!("Position = {pos}"));
    }
    let mut parts = Vec::new();
    if p.pipeline {
        parts.push("ValueFromPipeline = true".to_string());
    }
    // A literal-path parameter binds from a piped object's PSPath.
    if p.pipeline_by_name || p.literal_path {
        parts.push("ValueFromPipelineByPropertyName = true".to_string());
    }
    if p.remaining {
        parts.push("ValueFromRemainingArguments = true".to_string());
    }
    if !p.help.is_empty() {
        parts.push(format!("HelpMessage = {}", cs_str(&p.help)));
    }
    if p.dont_show {
        parts.push("DontShow = true".to_string());
    }
    let mut out = String::new();
    // One [Parameter] per set the parameter belongs to, each with the
    // same arguments, and one naming no set for a parameter in every set.
    if p.sets.is_empty() {
        let all: Vec<&str> = head.iter().chain(parts.iter()).map(String::as_str).collect();
        out.push_str(&format!("        [Parameter({})]\n", all.join(", ")));
    }
    for set in &p.sets {
        let named = format!("ParameterSetName = {}", cs_str(set));
        let all: Vec<&str> = head
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(named.as_str()))
            .chain(parts.iter().map(String::as_str))
            .collect();
        out.push_str(&format!("        [Parameter({})]\n", all.join(", ")));
    }
    let mut aliases: Vec<String> = p.aliases.iter().map(|a| cs_str(a)).collect();
    if p.literal_path {
        // The two aliases a literal-path parameter gets. A name the
        // author already gave is not added twice.
        for named in ["PSPath", "LP"] {
            if !p.aliases.iter().any(|a| a.eq_ignore_ascii_case(named)) {
                aliases.push(cs_str(named));
            }
        }
    }
    if !aliases.is_empty() {
        out.push_str(&format!("        [Alias({})]\n", aliases.join(", ")));
    }
    if !p.validate_set.is_empty() {
        let list: Vec<String> = p.validate_set.iter().map(|a| cs_str(a)).collect();
        out.push_str(&format!("        [ValidateSet({})]\n", list.join(", ")));
    }
    if let Some((min, max)) = p.validate_range {
        out.push_str(&format!("        [ValidateRange({min}L, {max}L)]\n"));
    }
    if let Some(pat) = &p.validate_pattern {
        out.push_str(&format!("        [ValidatePattern({})]\n", cs_str(pat)));
    }
    if p.not_null_or_empty {
        out.push_str("        [ValidateNotNullOrEmpty]\n");
    }
    if p.allow_empty_collection {
        out.push_str("        [AllowEmptyCollection]\n");
    }
    if p.allow_empty_string {
        out.push_str("        [AllowEmptyString]\n");
    }
    if let Some((min, max)) = p.validate_length {
        out.push_str(&format!("        [ValidateLength({min}, {max})]\n"));
    }
    if let Some((min, max)) = p.validate_count {
        out.push_str(&format!("        [ValidateCount({min}, {max})]\n"));
    }
    if let Some(script) = &p.validate_script {
        out.push_str(&format!("        [Pwrs.ValidateScriptText({})]\n", cs_str(script)));
    }
    if p.validate_not_null {
        out.push_str("        [ValidateNotNull]\n");
    }
    if p.allow_null {
        out.push_str("        [AllowNull]\n");
    }
    if p.supports_wildcards {
        out.push_str("        [SupportsWildcards]\n");
    }
    if p.clr.ends_with("[]") {
        out.push_str("        [Pwrs.UnwrapArray]\n");
    }
    out
}

/// The generated completer class name for a completer id.
fn completer_class(id: u32) -> String {
    format!("PwrsCompleter{id}")
}

fn transform_class(id: u32) -> String {
    format!("PwrsTransform{id}")
}

/// The C# class a generated cmdlet or provider takes: its Rust name and
/// `suffix`, with its id after them when another of its kind in the
/// module shares the Rust name, since a namespace can have only one type
/// of a name.
fn type_class<'a>(rust: &str, id: u32, suffix: &str, kin: impl Iterator<Item = &'a str>) -> String {
    let base = cs_ident(rust);
    if kin.filter(|other| cs_ident(other) == base).count() > 1 {
        format!("{base}{suffix}{id}")
    } else {
        format!("{base}{suffix}")
    }
}

fn cmdlet_class(class: &str, module_class: &str, module_ns: &str, c: &Cmdlet, completers: &[Completer], transforms: &[Transform]) -> String {
    let mut s = String::new();
    let mut cmdlet_args = vec![cs_str(&c.verb), cs_str(&c.noun)];
    if c.should_process {
        cmdlet_args.push("SupportsShouldProcess = true".to_string());
    }
    if let Some(ci) = &c.confirm_impact {
        cmdlet_args.push(format!("ConfirmImpact = ConfirmImpact.{ci}"));
    }
    if let Some(ds) = &c.default_set {
        cmdlet_args.push(format!("DefaultParameterSetName = {}", cs_str(ds)));
    }
    s.push_str(&format!("    [Cmdlet({})]\n", cmdlet_args.join(", ")));
    if !c.aliases.is_empty() {
        let list: Vec<String> = c.aliases.iter().map(|a| cs_str(a)).collect();
        s.push_str(&format!("    [Alias({})]\n", list.join(", ")));
    }
    if !c.output_types.is_empty() {
        let list: Vec<String> = c.output_types.iter().map(|a| cs_str(a)).collect();
        s.push_str(&format!("    [OutputType({})]\n", list.join(", ")));
    }
    let interfaces = if c.dynamic_params { " : Pwrs.RustCmdlet, IDynamicParameters" } else { " : Pwrs.RustCmdlet" };
    s.push_str(&format!("    public sealed class {class}{interfaces}\n    {{\n"));
    // The binder assigns a property only when the parameter is
    // supplied, so the setter records that the parameter was bound,
    // and the call path does no dictionary lookup. A cmdlet with no
    // parameters has nothing to record and declares neither word.
    let has_params = !c.params.is_empty();
    let words = mask_words(c.params.len());
    if has_params {
        for w in 0..words {
            s.push_str(&format!("        private ulong {};\n", word_name("_bound", w)));
        }
        for w in 0..words {
            s.push_str(&format!("        private ulong {};\n", word_name("_dirty", w)));
        }
        s.push('\n');
    }
    for p in &c.params {
        let clr = if is_reference_type(&p.clr, p.value_type) { format!("{}?", cs_type(&p.clr)) } else { cs_type(&p.clr) };
        s.push_str(&format!("        private {clr} _p{};\n", p.index));
        s.push_str(&param_attributes(p));
        if let Some(cmp) = completers.iter().find(|cm| cm.cmdlet == c.name && cm.parameter == p.name) {
            s.push_str(&format!("        [ArgumentCompleter(typeof(global::{module_ns}.{}))]\n", completer_class(cmp.id)));
        }
        // The transformation runs before the binder converts the
        // argument to this property's type, so it is declared on the
        // property beside the parameter attributes.
        if let Some(t) = transforms.iter().find(|tr| tr.cmdlet == c.name && tr.parameter == p.name) {
            s.push_str(&format!("        [global::{module_ns}.{}]\n", transform_class(t.id)));
        }
        let (word, bit) = mask_bit(p.index);
        s.push_str(&format!(
            "        public {clr} {name} {{ get => _p{i}; set {{ _p{i} = value; {bound} |= 1UL << {bit}; {dirty} |= 1UL << {bit}; }} }}\n\n",
            name = p.name,
            i = p.index,
            bound = word_name("_bound", word),
            dirty = word_name("_dirty", word)
        ));
    }
    s.push_str(&format!("        protected override uint CmdletId => {};\n", c.id));
    s.push_str(&format!("        protected override Pwrs.NativeModule Module => {module_class}.Native;\n\n"));
    if c.dynamic_params {
        // Completion sets these properties without filling
        // MyInvocation.BoundParameters, so the hook also receives every
        // parameter whose setter ran. The table built here is the one
        // the hook reads, with each value out of the PSObject the
        // binder may have wrapped it in.
        s.push_str("        public object? GetDynamicParameters()\n        {\n");
        s.push_str("            var bound = new System.Collections.Hashtable(System.StringComparer.OrdinalIgnoreCase);\n");
        s.push_str("            foreach (System.Collections.DictionaryEntry e in (System.Collections.IDictionary)MyInvocation.BoundParameters) bound[e.Key] = Pwrs.DynamicParametersBuilder.Bare(e.Value);\n");
        for p in &c.params {
            let (word, bit) = mask_bit(p.index);
            s.push_str(&format!(
                "            if (({bound} & (1UL << {bit})) != 0 && !bound.ContainsKey(\"{name}\")) bound[\"{name}\"] = Pwrs.DynamicParametersBuilder.Bare(_p{i});\n",
                i = p.index,
                name = p.name,
                bound = word_name("_bound", word)
            ));
        }
        s.push_str("            return Pwrs.DynamicParametersBuilder.Build(Module, CmdletId, bound);\n        }\n\n");
    }

    s.push_str("        [StructLayout(LayoutKind.Sequential)]\n        private struct Block\n        {\n");
    for w in 0..words {
        s.push_str(&format!("            public ulong {};\n", word_name("Dirty", w)));
    }
    for w in 0..words {
        s.push_str(&format!("            public ulong {};\n", word_name("Bound", w)));
    }
    for p in &c.params {
        s.push_str(&format!("            public {} P{};\n", slot_cs(&p.slot), p.index));
    }
    s.push_str("        }\n\n");

    s.push_str("        private unsafe void Run(uint phase)\n        {\n");
    s.push_str("            long t0 = Pwrs.Trace.Enabled ? Pwrs.Trace.Now() : 0;\n");
    s.push_str("            Block b = default;\n");
    if has_params {
        for w in 0..words {
            let (dirty, bound) = (word_name("_dirty", w), word_name("_bound", w));
            s.push_str(&format!(
                "            b.{} = {dirty};\n            {dirty} = 0;\n            b.{} = {bound};\n",
                word_name("Dirty", w),
                word_name("Bound", w)
            ));
        }
    }
    let mut strings = Vec::new();
    let mut handles = Vec::new();
    for p in &c.params {
        match p.slot.as_str() {
            "bool" => s.push_str(&format!("            b.P{} = {}.IsPresent ? (byte)1 : (byte)0;\n", p.index, p.name)),
            // A string given as $null is passed as a null pointer, so an
            // Option<String> reads it as None and a String as empty.
            "str16" => {
                s.push_str(&format!("            string? s{} = {};\n", p.index, p.name));
                strings.push(p.index);
            }
            "handle" if p.value_type => {
                s.push_str(&format!("            GCHandle h{i} = GCHandle.Alloc((object){n});\n", i = p.index, n = p.name));
                handles.push(p.index);
            }
            "handle" => {
                s.push_str(&format!(
                    "            GCHandle h{i} = {n} == null ? default : GCHandle.Alloc({n});\n",
                    i = p.index,
                    n = p.name
                ));
                handles.push(p.index);
            }
            _number => s.push_str(&format!("            b.P{} = {};\n", p.index, p.name)),
        }
    }
    s.push_str("            try\n            {\n");
    for i in &handles {
        s.push_str(&format!("                b.P{i} = h{i}.IsAllocated ? GCHandle.ToIntPtr(h{i}) : IntPtr.Zero;\n"));
    }
    let mut indent = "                ".to_string();
    for i in &strings {
        s.push_str(&format!("{indent}fixed (char* c{i} = s{i})\n{indent}{{\n"));
        indent.push_str("    ");
        s.push_str(&format!("{indent}b.P{i} = new Pwrs.PsStr16 {{ Ptr = (ushort*)c{i}, Len = s{i} == null ? 0 : (nuint)s{i}.Length }};\n"));
    }
    s.push_str(&format!("{indent}Invoke(phase, &b);\n"));
    for _ in &strings {
        indent.truncate(indent.len() - 4);
        s.push_str(&format!("{indent}}}\n"));
    }
    s.push_str("            }\n            finally\n            {\n");
    for i in &handles {
        s.push_str(&format!("                if (h{i}.IsAllocated) h{i}.Free();\n"));
    }
    s.push_str("                if (Pwrs.Trace.Enabled) Pwrs.Trace.Phase(t0);\n");
    s.push_str("            }\n        }\n\n");
    // A value given as an argument is assigned before `begin`, so its
    // dirty bits belong to that phase. A `begin` found empty is
    // skipped, and its bits are cleared instead, or the first record's
    // phase would count those arguments as piped. Binding reads every
    // bound parameter, so the first record's phase still binds them.
    if has_params {
        let clear = if words == 1 {
            "_dirty = 0;".to_string()
        } else {
            let each: Vec<String> = (0..words).map(|w| format!("{} = 0;", word_name("_dirty", w))).collect();
            format!("{{ {} }}", each.join(" "))
        };
        s.push_str(&format!("        protected override void BeginProcessing() {{ if (NeedsPhase(Pwrs.Native.PhaseMaskBegin)) Run(Pwrs.Native.PhaseBegin); else {clear} }}\n"));
    } else {
        s.push_str("        protected override void BeginProcessing() { if (NeedsPhase(Pwrs.Native.PhaseMaskBegin)) Run(Pwrs.Native.PhaseBegin); }\n");
    }
    s.push_str("        protected override void ProcessRecord() => Run(Pwrs.Native.PhaseProcess);\n");
    s.push_str("        protected override void EndProcessing() { if (NeedsPhase(Pwrs.Native.PhaseMaskEnd)) Run(Pwrs.Native.PhaseEnd); }\n");
    s.push_str("    }\n\n");
    s
}

/// `Hello.Person` splits into namespace `Hello` and type `Person`; a
/// bare name goes in the module namespace.
fn split_type_name<'a>(full: &'a str, module_ns: &'a str) -> (&'a str, &'a str) {
    match full.rfind('.') {
        Some(i) => (&full[..i], &full[i + 1..]),
        None => (module_ns, full),
    }
}

/// Property type for an output field.
fn field_property_type(f: &Field) -> String {
    if is_reference_type(&f.clr, f.value_type) || f.optional {
        format!("{}?", cs_type(&f.clr))
    } else {
        cs_type(&f.clr)
    }
}

/// Expression reading field `f` out of the block `b` in a copied
/// class factory.
fn field_read_expr(f: &Field) -> String {
    let i = f.index;
    let (word, bit) = mask_bit(i);
    let present = format!("(b->{} & (1UL << {bit})) != 0", word_name("Mask", word));
    match f.slot.as_str() {
        "bool" => {
            if f.optional {
                format!("{present} ? (bool?)(b->F{i} != 0) : null")
            } else {
                format!("b->F{i} != 0")
            }
        }
        "str16" => format!("b->F{i}.Ptr == null ? null : b->F{i}.ToString()"),
        "handle" if f.value_type => {
            if f.optional {
                format!("{present} ? ({}?)Pwrs.Native.TargetOf(b->F{i}) : null", cs_type(&f.clr))
            } else {
                format!("({})Pwrs.Native.TargetOf(b->F{i})!", cs_type(&f.clr))
            }
        }
        "handle" => {
            if f.clr == "object" {
                format!("Pwrs.Native.TargetOf(b->F{i})")
            } else if f.clr.ends_with("[]") {
                // The block has the array IntoPs built, whose
                // element type follows the Rust element's tag; the
                // engine converts it to the declared element type.
                format!("({0}?)LanguagePrimitives.ConvertTo(Pwrs.Native.TargetOf(b->F{i}), typeof({0}))", cs_type(&f.clr))
            } else {
                format!("({}?)Pwrs.Native.TargetOf(b->F{i})", cs_type(&f.clr))
            }
        }
        _number => {
            if f.optional {
                format!("{present} ? b->F{i} : ({}?)null", cs_type(&f.clr))
            } else {
                format!("b->F{i}")
            }
        }
    }
}

/// C# parameter type of a method argument: nullable when optional or
/// a reference type. An optional string is `object?`, because the
/// engine's method binder turns `$null` and an omitted argument into
/// an empty `string`, which would make `None` unreachable.
pub(crate) fn arg_type(p: &MethodParam) -> String {
    if p.slot == "str16" && p.optional {
        "object?".to_string()
    } else if is_reference_type(&p.clr, p.value_type) || p.optional {
        format!("{}?", cs_type(&p.clr))
    } else {
        cs_type(&p.clr)
    }
}

/// CLR types whose value may arrive boxed as something wider. A
/// library built with an older `pwrs` widens every narrow integer to
/// `long` and `float` to `double`, and an unboxing cast to the
/// declared type throws on such a box, so these are converted rather
/// than unboxed and read either box.
fn boxed_wider(clr: &str) -> bool {
    matches!(clr, "sbyte" | "short" | "int" | "byte" | "ushort" | "uint" | "float")
}

/// Reads the object `expr` as `clr`, nullable or not: an unboxing
/// cast where the boxed value is already that type, a conversion where
/// it is the wider one.
pub(crate) fn read_boxed(expr: &str, clr: &str, nullable: bool) -> String {
    let t = cs_type(clr);
    if boxed_wider(clr) {
        let target = if nullable { format!("{t}?") } else { t };
        format!("LanguagePrimitives.ConvertTo<{target}>({expr})")
    } else if nullable {
        format!("({t}?){expr}")
    } else {
        format!("({t}){expr}!")
    }
}

/// Whether a method's declared return type is nullable.
fn return_nullable(r: &MethodRet) -> bool {
    is_reference_type(&r.clr, r.value_type) || r.optional
}

/// The C# type a method declares it returns.
pub(crate) fn return_cs_type(r: &MethodRet) -> String {
    if return_nullable(r) { format!("{}?", cs_type(&r.clr)) } else { cs_type(&r.clr) }
}

/// The .NET interfaces a proxy class implements through the methods its
/// roles name, and the members that implement them. Each member calls
/// the generated method of the same role, so it calls into Rust as
/// that method does. Members PowerShell reaches by name, the list's
/// `Count` and indexer and the `Equals` and `GetHashCode` overrides,
/// are public; the rest are explicit implementations, so a method of
/// the class may have the interface member's name.
fn interface_members(c: &Class, ty: &str) -> (Vec<String>, String) {
    const GENERIC: &str = "global::System.Collections.Generic";
    let mut interfaces = Vec::new();
    let mut s = String::new();
    if let (Some(count), Some(item)) = (role(c, &c.count), role(c, &c.item)) {
        let t = item.ret.as_ref().map_or_else(|| "object?".to_string(), return_cs_type);
        interfaces.push(format!("{GENERIC}.IReadOnlyList<{t}>"));
        s.push_str(&format!("\n        /// <summary>The number of elements, from {}.</summary>\n", count.name));
        s.push_str(&format!("        public int Count => {}();\n\n", count.name));
        s.push_str(&format!("        /// <summary>The element at an index, from {}.</summary>\n", item.name));
        match role(c, &c.set_item) {
            Some(set) => {
                interfaces.push(format!("{GENERIC}.IList<{t}>"));
                s.push_str(&format!("        public {t} this[int index]\n        {{\n            get => {}(index);\n            set => {}(index, value);\n        }}\n\n", item.name, set.name));
            }
            None => s.push_str(&format!("        public {t} this[int index] => {}(index);\n\n", item.name)),
        }
        s.push_str(&format!(
            "        {GENERIC}.IEnumerator<{t}> {GENERIC}.IEnumerable<{t}>.GetEnumerator()\n        {{\n            int count = Count;\n            for (int i = 0; i < count; i++) yield return this[i];\n        }}\n\n"
        ));
        if c.set_item.is_some() {
            // The size is fixed, so IsReadOnly is true, as an array's is
            // through ICollection<T>: elements are set through the indexer.
            s.push_str(&format!("        bool {GENERIC}.ICollection<{t}>.IsReadOnly => true;\n\n"));
            s.push_str(&format!(
                "        int {GENERIC}.IList<{t}>.IndexOf({t} item)\n        {{\n            int count = Count;\n            for (int i = 0; i < count; i++)\n            {{\n                if ({GENERIC}.EqualityComparer<{t}>.Default.Equals(this[i], item)) return i;\n            }}\n            return -1;\n        }}\n\n"
            ));
            s.push_str(&format!("        bool {GENERIC}.ICollection<{t}>.Contains({t} item) => (({GENERIC}.IList<{t}>)this).IndexOf(item) >= 0;\n\n"));
            s.push_str(&format!(
                "        void {GENERIC}.ICollection<{t}>.CopyTo({t}[] array, int arrayIndex)\n        {{\n            if (array is null) throw new ArgumentNullException(nameof(array));\n            int count = Count;\n            for (int i = 0; i < count; i++) array[arrayIndex + i] = this[i];\n        }}\n\n"
            ));
            let fixed = format!("throw new NotSupportedException({})", cs_str(&format!("a {ty} has a fixed size; set its elements through the indexer")));
            s.push_str(&format!("        void {GENERIC}.IList<{t}>.Insert(int index, {t} item) => {fixed};\n"));
            s.push_str(&format!("        void {GENERIC}.IList<{t}>.RemoveAt(int index) => {fixed};\n"));
            s.push_str(&format!("        void {GENERIC}.ICollection<{t}>.Add({t} item) => {fixed};\n"));
            s.push_str(&format!("        bool {GENERIC}.ICollection<{t}>.Remove({t} item) => {fixed};\n"));
            s.push_str(&format!("        void {GENERIC}.ICollection<{t}>.Clear() => {fixed};\n\n"));
        }
        s.push_str(&format!("        global::System.Collections.IEnumerator global::System.Collections.IEnumerable.GetEnumerator() => (({GENERIC}.IEnumerable<{t}>)this).GetEnumerator();\n"));
    }
    if let Some(next) = role(c, &c.next) {
        let (t, value) = match &next.ret {
            Some(r) if is_reference_type(&r.clr, r.value_type) => (cs_type(&r.clr), "next"),
            Some(r) => (cs_type(&r.clr), "next.Value"),
            None => ("object".to_string(), "next"),
        };
        interfaces.push(format!("{GENERIC}.IEnumerable<{t}>"));
        s.push_str(&format!(
            "\n        /// <summary>The stream's elements, from {0} until it returns null. The\n        /// stream is the object's own, so a second enumeration continues it.</summary>\n        {GENERIC}.IEnumerator<{t}> {GENERIC}.IEnumerable<{t}>.GetEnumerator()\n        {{\n            while (true)\n            {{\n                var next = {0}();\n                if (next is null) yield break;\n                yield return {value};\n            }}\n        }}\n\n",
            next.name
        ));
        s.push_str(&format!("        global::System.Collections.IEnumerator global::System.Collections.IEnumerable.GetEnumerator() => (({GENERIC}.IEnumerable<{t}>)this).GetEnumerator();\n"));
    }
    if let Some(compare) = role(c, &c.compare) {
        interfaces.push("global::System.IComparable".to_string());
        interfaces.push(format!("global::System.IComparable<{ty}>"));
        s.push_str(&format!(
            "\n        /// <summary>The order of this object and another, from {0}; null is first.</summary>\n        int global::System.IComparable<{ty}>.CompareTo({ty}? other)\n        {{\n            if (other is null) return 1;\n            if (ReferenceEquals(other, this)) return 0;\n            return {0}(other);\n        }}\n\n",
            compare.name
        ));
        s.push_str(&format!(
            "        int global::System.IComparable.CompareTo(object? obj)\n        {{\n            if (obj is PSObject wrapped) obj = wrapped.BaseObject;\n            if (obj is null) return 1;\n            if (obj is {ty} other) return ((global::System.IComparable<{ty}>)this).CompareTo(other);\n            throw new ArgumentException({}, nameof(obj));\n        }}\n",
            cs_str(&format!("a {ty} is ordered only against another {ty}"))
        ));
    }
    if let (Some(equals), Some(hash)) = (role(c, &c.equals), role(c, &c.hash)) {
        interfaces.push(format!("global::System.IEquatable<{ty}>"));
        s.push_str(&format!(
            "\n        /// <summary>Whether another object is equal to this one, from {0}.</summary>\n        bool global::System.IEquatable<{ty}>.Equals({ty}? other)\n        {{\n            if (other is null) return false;\n            return ReferenceEquals(other, this) || {0}(other);\n        }}\n\n",
            equals.name
        ));
        s.push_str(&format!(
            "        /// <summary>Whether an object is one of this class equal to this one.</summary>\n        public override bool Equals(object? obj)\n        {{\n            if (obj is PSObject wrapped) obj = wrapped.BaseObject;\n            return obj is {ty} other && ((global::System.IEquatable<{ty}>)this).Equals(other);\n        }}\n\n"
        ));
        s.push_str(&format!(
            "        /// <summary>The hash {0} returns, folded to 32 bits.</summary>\n        public override int GetHashCode()\n        {{\n            long hash = {0}();\n            return unchecked((int)hash ^ (int)(hash >> 32));\n        }}\n",
            hash.name
        ));
    }
    (interfaces, s)
}

/// The C# operators a class declares through the methods its operator
/// keys name, which PowerShell's `+`, `-`, `*`, `/` and `%` call. Each
/// calls the generated method, so it calls into Rust as that method
/// does: an instance method on the left operand, or a static one with
/// both operands. `validate` has checked each method's operands and that
/// it returns a value.
fn operator_members(c: &Class, ty: &str) -> String {
    let mut s = String::new();
    for (op, name) in [("+", &c.add), ("-", &c.sub), ("*", &c.mul), ("/", &c.div), ("%", &c.rem)] {
        let Some(m) = role(c, name) else { continue };
        let ret = m.ret.as_ref().map_or_else(|| "void".to_string(), return_cs_type);
        let (left, right, call) = if m.is_static {
            (arg_type(&m.params[0]), arg_type(&m.params[1]), format!("{}(left, right)", m.name))
        } else {
            (ty.to_string(), arg_type(&m.params[0]), format!("left.{}(right)", m.name))
        };
        s.push_str(&format!("\n        /// <summary>{op}, from {}.</summary>\n        public static {ret} operator {op}({left} left, {right} right) => {call};\n", m.name));
    }
    s
}

/// Expression converting the object `result` a proxy call returned to
/// the method's declared return type.
fn return_expr(r: &MethodRet, result: &str) -> String {
    let t = cs_type(&r.clr);
    if r.clr == "object" {
        result.to_string()
    } else if r.clr.ends_with("[]") {
        format!("({t}?)LanguagePrimitives.ConvertTo({result}, typeof({t}))")
    } else {
        read_boxed(result, &r.clr, return_nullable(r))
    }
}

/// How a class's generated methods reach its Rust value.
#[derive(Clone, Copy, Default)]
struct Reach<'a> {
    /// Through the class's own `__pwrsCore` field, for a class with a
    /// base of its own; through Pwrs.ProxyBase's methods otherwise.
    core: bool,
    /// For a class with a base of its own: the base call of the
    /// constructor `new` makes, for `NET` and for netstandard2.0.
    new_base: Option<&'a (String, String)>,
}

/// One `#[psmethods]` method on a proxy class: a private argument
/// block, then the method packing its arguments the way a cmdlet's
/// `Run` packs parameters and making one `Call`.
///
/// A method with no receiver is `static` and calls through the
/// module's native table rather than through the object. The static
/// named `new` is the class's constructor: a public constructor that
/// chains to the internal one with the pointer a private static gets
/// from Rust, so a script reaches it as `[Type]::new(...)`. On a class
/// with a base of its own it calls the base's constructor instead, then
/// makes the core around that pointer. A method backing a member is the
/// private helper `__Pwrs<index>`, which the member calls.
fn proxy_method(module_ns: &str, module_class: &str, c: &Class, m: &Method, reach: Reach<'_>) -> String {
    let (_, ty) = split_type_name(&c.name, module_ns);
    let native = format!("global::{module_ns}.{module_class}.Native");
    let block = format!("{}Args", cs_ident(&m.name));
    // A method taking a PsTask returns the task it passes to Rust, of the
    // task's element type, and takes the caller's cancellation token.
    let task = m.params.iter().find(|p| p.slot == "task");
    let task_value = task.map(|t| if t.clr == "void" { "object?".to_string() } else { arg_type(t) });
    let ret_ty = match (&task, &task_value, &m.ret) {
        (Some(t), _, _) if t.clr == "void" => "global::System.Threading.Tasks.Task".to_string(),
        (Some(_), Some(value), _) => format!("global::System.Threading.Tasks.Task<{value}>"),
        (_, _, None) => "void".to_string(),
        (_, _, Some(r)) if return_nullable(r) => format!("{}?", cs_type(&r.clr)),
        (_, _, Some(r)) => cs_type(&r.clr),
    };
    let mut params: Vec<String> = m
        .params
        .iter()
        .filter(|p| p.slot != "task")
        .map(|p| if p.optional { format!("{} {} = null", arg_type(p), cs_name(&p.name)) } else { format!("{} {}", arg_type(p), cs_name(&p.name)) })
        .collect();
    if task.is_some() {
        params.push("global::System.Threading.CancellationToken cancellationToken = default".to_string());
    }
    let mut s = String::new();
    s.push_str(&format!("        [StructLayout(LayoutKind.Sequential)]\n        private struct {block}\n        {{\n"));
    for w in 0..mask_words(m.params.len()) {
        s.push_str(&format!("            public ulong {};\n", word_name("Bound", w)));
    }
    for p in &m.params {
        s.push_str(&format!("            public {} P{};\n", slot_cs(&p.slot), p.index));
    }
    s.push_str("        }\n\n");
    let helper = crate::bases::is_helper(m);
    if !m.help.is_empty() && !helper {
        s.push_str(&format!("        /// <summary>{}</summary>\n", m.help));
    }
    // A proxy's constructor adopts the pointer of the value Rust made;
    // a copied class's copies the fields of the object Rust built.
    let copied = c.mode == "copied";
    if m.constructor {
        let names: Vec<String> = m.params.iter().map(|p| cs_name(&p.name)).collect();
        if copied {
            let chain = format!("this(default(global::Pwrs.FromFields), Construct{}({}))", m.index, names.join(", "));
            s.push_str(&format!("        public {ty}({}) : {chain} {{ }}\n\n", params.join(", ")));
            s.push_str(&format!("        private static unsafe {ty} Construct{}({})\n        {{\n", m.index, params.join(", ")));
        } else if reach.core {
            // The base is made first, so a refusing Rust constructor
            // leaves an object with no core, and the core is the last
            // thing made.
            let bytes = if c.native_bytes { "true" } else { "false" };
            let core = format!("__pwrsCore = new global::Pwrs.ProxyCore(this, {native}, {}, Construct{}({}), {bytes});", c.id, m.index, names.join(", "));
            let ctor = |base_call: &str| format!("        public {ty}({}) : {base_call}\n        {{\n            {core}\n        }}\n\n", params.join(", "));
            match reach.new_base {
                Some((net, standard)) if net == standard => s.push_str(&ctor(net)),
                Some((net, standard)) => s.push_str(&format!("#if NET\n{}#else\n{}#endif\n", ctor(net), ctor(standard))),
                None => s.push_str(&ctor("base()")),
            }
            s.push_str(&format!("        private static unsafe IntPtr Construct{}({})\n        {{\n", m.index, params.join(", ")));
        } else {
            let chain = format!("this(Construct{}({}))", m.index, names.join(", "));
            s.push_str(&format!("        public {ty}({}) : {chain} {{ }}\n\n", params.join(", ")));
            s.push_str(&format!("        private static unsafe IntPtr Construct{}({})\n        {{\n", m.index, params.join(", ")));
        }
    } else if m.is_static {
        s.push_str(&format!("        public static unsafe {ret_ty} {}({})\n        {{\n", m.name, params.join(", ")));
    } else if helper {
        s.push_str(&format!("        private unsafe {ret_ty} __Pwrs{}({})\n        {{\n", m.index, params.join(", ")));
    } else {
        s.push_str(&format!("        public unsafe {ret_ty} {}({})\n        {{\n", m.name, params.join(", ")));
    }
    // The body's own locals start with two underscores, which no argument
    // name can: an argument's C# name is its Rust name in lower camel
    // case, and that drops every underscore.
    // The task exists before anything can fail, so every failure on the
    // way to Rust faults it rather than leaving it to wait.
    if let Some(value) = &task_value {
        s.push_str(&format!("            var __task = new global::Pwrs.TaskSource<{value}>(cancellationToken);\n            try\n            {{\n"));
    }
    s.push_str(&format!("            {block} __args = default;\n"));
    let mut strings = Vec::new();
    let mut handles = Vec::new();
    for p in &m.params {
        let i = p.index;
        let n = cs_name(&p.name);
        let (word, shift) = mask_bit(i);
        let bit = format!("__args.{} |= 1UL << {shift};", word_name("Bound", word));
        match p.slot.as_str() {
            "bool" if p.optional => s.push_str(&format!("            if ({n}.HasValue) {{ __args.P{i} = {n}.Value ? (byte)1 : (byte)0; {bit} }}\n")),
            "bool" => s.push_str(&format!("            __args.P{i} = {n} ? (byte)1 : (byte)0;\n            {bit}\n")),
            "str16" if p.optional => {
                s.push_str(&format!(
                    "            string? __t{i} = {n} == null ? null : LanguagePrimitives.ConvertTo<string>({n});\n            string __s{i} = __t{i} ?? string.Empty;\n            if (__t{i} != null) {{ {bit} }}\n"
                ));
                strings.push(i);
            }
            "str16" => {
                s.push_str(&format!("            string __s{i} = {n} ?? string.Empty;\n            if ({n} != null) {{ {bit} }}\n"));
                strings.push(i);
            }
            "handle" if p.value_type && p.optional => {
                s.push_str(&format!("            GCHandle __h{i} = {n}.HasValue ? GCHandle.Alloc((object){n}.Value) : default;\n            if ({n}.HasValue) {{ {bit} }}\n"));
                handles.push(i);
            }
            "handle" if p.value_type => {
                s.push_str(&format!("            GCHandle __h{i} = GCHandle.Alloc((object){n});\n            {bit}\n"));
                handles.push(i);
            }
            "handle" => {
                s.push_str(&format!("            GCHandle __h{i} = {n} == null ? default : GCHandle.Alloc({n});\n            if ({n} != null) {{ {bit} }}\n"));
                handles.push(i);
            }
            // Another object of the class: entered beside this one by the
            // call, which writes its value pointer into the slot.
            "peer" => s.push_str(&format!("            if ({n} is null) throw new ArgumentNullException(nameof({n}));\n            {bit}\n")),
            // The task source, reached through a handle for the call and
            // passed with its cancellation byte.
            "task" => {
                s.push_str(&format!("            GCHandle __h{i} = GCHandle.Alloc(__task);\n            {bit}\n"));
                handles.push(i);
            }
            _number if p.optional => s.push_str(&format!("            if ({n}.HasValue) {{ __args.P{i} = {n}.Value; {bit} }}\n")),
            _number => s.push_str(&format!("            __args.P{i} = {n};\n            {bit}\n")),
        }
    }
    // A static has no object to call through, so it names the module's
    // native table and the class id itself. A method with a receiver
    // enters the object's gate exclusively when it takes `&mut self`
    // and as shared when it takes `&self`.
    let invoke = if m.is_static {
        format!("global::Pwrs.StaticCall.Invoke({native}, {}, {}, &__args)", c.id, m.index)
    } else if let Some(peer) = m.params.iter().find(|p| p.slot == "peer") {
        if reach.core {
            format!("__pwrsCore.CallWith({}, &__args, {}, {}.__pwrsCore, &__args.P{})", m.index, m.mutable, cs_name(&peer.name), peer.index)
        } else {
            format!("base.PwrsCallWith({}, &__args, {}, {}, &__args.P{})", m.index, m.mutable, cs_name(&peer.name), peer.index)
        }
    } else if reach.core {
        format!("__pwrsCore.Call({}, &__args, {})", m.index, m.mutable)
    } else {
        format!("base.PwrsCall({}, &__args, {})", m.index, m.mutable)
    };
    let call = match &m.ret {
        Some(_returns) => {
            s.push_str("            object? __result;\n");
            format!("__result = {invoke};")
        }
        None => format!("{invoke};"),
    };
    // Handles are freed in a finally; a method without any needs no
    // try block.
    let guarded = !handles.is_empty();
    let mut indent = if guarded { "                ".to_string() } else { "            ".to_string() };
    if guarded {
        s.push_str("            try\n            {\n");
    }
    for i in &handles {
        if task.is_some_and(|t| t.index == *i) {
            s.push_str(&format!("{indent}__args.P{i} = new global::Pwrs.PsTaskSlot {{ Source = GCHandle.ToIntPtr(__h{i}), Cancelled = __task.CancelFlag }};\n"));
        } else {
            s.push_str(&format!("{indent}__args.P{i} = __h{i}.IsAllocated ? GCHandle.ToIntPtr(__h{i}) : IntPtr.Zero;\n"));
        }
    }
    for i in &strings {
        s.push_str(&format!("{indent}fixed (char* __c{i} = __s{i})\n{indent}{{\n"));
        indent.push_str("    ");
        s.push_str(&format!("{indent}__args.P{i} = new Pwrs.PsStr16 {{ Ptr = (ushort*)__c{i}, Len = (nuint)__s{i}.Length }};\n"));
    }
    s.push_str(&format!("{indent}{call}\n"));
    for _ in &strings {
        indent.truncate(indent.len() - 4);
        s.push_str(&format!("{indent}}}\n"));
    }
    if guarded {
        s.push_str("            }\n            finally\n            {\n");
        for i in &handles {
            s.push_str(&format!("                if (__h{i}.IsAllocated) __h{i}.Free();\n"));
        }
        s.push_str("            }\n");
    }
    if task.is_some() {
        s.push_str("            }\n            catch (Exception __failed)\n            {\n                __task.Discard(__failed);\n                throw;\n            }\n            return __task.Task;\n");
    }
    if m.constructor && copied {
        s.push_str(&format!("            return ({ty})__result!;\n"));
    } else if m.constructor {
        // Rust returns the new value's pointer as a long.
        s.push_str("            return (IntPtr)(long)__result!;\n");
    } else if let Some(r) = &m.ret {
        s.push_str(&format!("            return {};\n", return_expr(r, "__result")));
    }
    s.push_str("        }\n\n");
    s
}

/// The constructors and statics of a copied class.
///
/// The factory builds through a constructor of its own, selected by
/// `Pwrs.FromFields`, so it never runs one the module declared. When
/// the module declares no `new`, the class also has the public
/// parameterless constructor C# would have supplied, which fills CLR
/// zeros. When it declares any, that constructor is not emitted, and a
/// script reaches only the constructors Rust declared: a parameterless
/// one then comes from a `new()` of the module's own, starting from
/// whatever it returns.
fn copied_constructors(module_class: &str, module_ns: &str, c: &Class) -> String {
    let (_, ty) = split_type_name(&c.name, module_ns);
    let mut s = String::new();
    s.push_str(&format!("\n        internal {ty}(global::Pwrs.FromFields _) {{ }}\n"));
    if c.methods.iter().any(|m| m.constructor) {
        // A declared constructor returns the object the factory built;
        // this copies it field by field into the one being made.
        s.push_str(&format!("\n        private {ty}(global::Pwrs.FromFields _, {ty} made)\n        {{\n"));
        for f in &c.fields {
            s.push_str(&format!("            {0} = made.{0};\n", f.name));
        }
        s.push_str("        }\n");
    } else {
        s.push_str(&format!("\n        public {ty}() {{ }}\n"));
    }
    if !c.methods.is_empty() {
        s.push('\n');
    }
    for m in &c.methods {
        s.push_str(&proxy_method(module_ns, module_class, c, m, Reach::default()));
    }
    s
}

/// `ToString()` for a copied class that declares `show`: the format's
/// text with each `{Property}` replaced by that property's value, and
/// `{{` and `}}` standing for braces. The macro has checked every name
/// against the class's properties and refused a stray brace.
fn show_override(show: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut chars = show.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                if !text.is_empty() {
                    parts.push(cs_str(&std::mem::take(&mut text)));
                }
                parts.push(format!("this.{name}"));
            }
            other => text.push(other),
        }
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(cs_str(&text));
    }
    format!(
        "\n        /// <summary>The text the class declares it shows.</summary>\n        public override string ToString() => string.Concat(new object?[] {{ {} }});\n",
        parts.join(", ")
    )
}

/// The public type for one class, in its own namespace, with `plan` for a
/// class that names a base, interfaces or members of its own.
fn class_type(module_class: &str, module_ns: &str, c: &Class, plan: Option<&ClassPlan>) -> String {
    let (ns, ty) = split_type_name(&c.name, module_ns);
    let mut s = String::new();
    match c.mode.as_str() {
        "copied" => {
            s.push_str(&format!("namespace {ns}\n{{\n"));
            if !c.description.is_empty() {
                s.push_str(&format!("    /// <summary>{}</summary>\n", c.description.replace('\n', " ")));
            }
            s.push_str(&format!("    public sealed class {ty}\n    {{\n"));
            for f in &c.fields {
                if !f.help.is_empty() {
                    s.push_str(&format!("        /// <summary>{}</summary>\n", f.help));
                }
                s.push_str(&format!("        public {} {} {{ get; set; }}\n", field_property_type(f), f.name));
            }
            s.push_str(&copied_constructors(module_class, module_ns, c));
            s.push_str(&operator_members(c, ty));
            if let Some(show) = &c.show {
                s.push_str(&show_override(show));
            }
            s.push_str("    }\n}\n\n");
        }
        "proxy" => {
            s.push_str(&format!("namespace {ns}\n{{\n"));
            if !c.description.is_empty() {
                s.push_str(&format!("    /// <summary>{}</summary>\n", c.description.replace('\n', " ")));
            }
            // A class with a base of its own derives from it and has its
            // Pwrs.ProxyCore in a field; any other derives from
            // Pwrs.ProxyBase, which has the core.
            let own_base = plan.and_then(|p| p.base.as_ref());
            let (interfaces, members) = interface_members(c, ty);
            let mut bases: Vec<String> = match own_base {
                Some(base) => vec![base.clone(), "global::Pwrs.IProxy".to_string()],
                None => vec!["Pwrs.ProxyBase".to_string()],
            };
            bases.extend(interfaces);
            if let Some(p) = plan {
                bases.extend(p.interfaces.iter().cloned());
            }
            s.push_str(&format!("    public sealed class {ty} : {}\n    {{\n", bases.join(", ")));
            let reach = Reach { core: own_base.is_some(), new_base: plan.and_then(|p| p.new_base.as_ref()) };
            match plan {
                Some(p) if p.has_base() => {
                    s.push_str("        private readonly global::Pwrs.ProxyCore __pwrsCore;\n\n");
                    s.push_str("        global::Pwrs.ProxyCore global::Pwrs.IProxy.PwrsCore => __pwrsCore;\n\n");
                    s.push_str(&p.factory);
                }
                _proxy_base => {
                    let reports_bytes = if c.native_bytes { "true" } else { "false" };
                    s.push_str(&format!(
                        "        internal {ty}(IntPtr instance) : base(global::{module_ns}.{module_class}.Native, {}, instance, {reports_bytes}) {{ }}\n",
                        c.id
                    ));
                }
            }
            for f in &c.fields {
                if !f.help.is_empty() {
                    s.push_str(&format!("        /// <summary>{}</summary>\n", f.help));
                }
                // Through base, or through the core's own field, so a
                // module method of the same name cannot take the call: C#
                // stops looking at the first type with an applicable
                // method.
                let prop_ty = field_property_type(f);
                let read = if reach.core { format!("__pwrsCore.Get({})", f.index) } else { format!("base.PwrsGet({})", f.index) };
                let cast = if f.clr.ends_with("[]") {
                    format!("({prop_ty})LanguagePrimitives.ConvertTo({read}, typeof({}))", cs_type(&f.clr))
                } else {
                    read_boxed(&read, &f.clr, prop_ty.ends_with('?') || is_reference_type(&f.clr, f.value_type))
                };
                s.push_str(&format!("        public {prop_ty} {} => {cast};\n", f.name));
            }
            if !c.methods.is_empty() {
                s.push('\n');
            }
            // A #[base] method is called only by the internal constructor,
            // through the plan's own helper.
            for m in c.methods.iter().filter(|m| !m.base) {
                s.push_str(&proxy_method(module_ns, module_class, c, m, reach));
            }
            s.push_str(&members);
            if let Some(p) = plan {
                s.push_str(&p.members);
                s.push_str(&p.dispose);
            }
            s.push_str(&operator_members(c, ty));
            s.push_str("    }\n}\n\n");
        }
        "enum" => {
            s.push_str(&format!("namespace {ns}\n{{\n"));
            if !c.description.is_empty() {
                s.push_str(&format!("    /// <summary>{}</summary>\n", c.description.replace('\n', " ")));
            }
            s.push_str(&format!("    public enum {ty} : long\n    {{\n"));
            for v in &c.variants {
                if !v.help.is_empty() {
                    s.push_str(&format!("        /// <summary>{}</summary>\n", v.help));
                }
                s.push_str(&format!("        {} = {},\n", cs_name(&v.name), v.value));
            }
            s.push_str("    }\n}\n\n");
        }
        _psobject => {}
    }
    s
}

/// Block struct and factory registration for one class, inside the
/// module namespace.
fn class_factory(module_ns: &str, c: &Class) -> (String, String) {
    let (ns, ty) = split_type_name(&c.name, module_ns);
    let full = format!("global::{ns}.{ty}");
    match c.mode.as_str() {
        "copied" => {
            // One block per class id: two classes may share a Rust name.
            let block = format!("{}Fields{}", cs_ident(&c.rust), c.id);
            let mut s = String::new();
            s.push_str(&format!("    [StructLayout(LayoutKind.Sequential)]\n    internal struct {block}\n    {{\n"));
            for f in &c.fields {
                s.push_str(&format!("        public {} F{};\n", slot_cs(&f.slot), f.index));
            }
            for w in 0..mask_words(c.fields.len()) {
                s.push_str(&format!("        public ulong {};\n", word_name("Mask", w)));
            }
            s.push_str("    }\n\n");
            let mut reg = String::new();
            reg.push_str(&format!("            Native.Factories.Register({}, fields =>\n            {{\n", c.id));
            reg.push_str(&format!("                var b = ({block}*)fields;\n"));
            reg.push_str(&format!("                return new {full}(default(global::Pwrs.FromFields))\n                {{\n"));
            for f in &c.fields {
                reg.push_str(&format!("                    {} = {},\n", f.name, field_read_expr(f)));
            }
            reg.push_str("                };\n            });\n");
            (s, reg)
        }
        "proxy" => (String::new(), format!("            Native.Factories.Register({}, instance => new {full}(instance));\n", c.id)),
        "enum" => (String::new(), format!("            Native.Factories.Register({}, value => (object)({full})(*(long*)value));\n", c.id)),
        _psobject => (String::new(), String::new()),
    }
}

/// The C# path of the module's `Pwrs.NativeModule`, which every generated
/// class reaches its native library through.
pub fn native_path(m: &Module) -> String {
    format!("global::Pwrs.Modules.{}.PwrsModule.Native", cs_ident(&m.name))
}

/// The whole shell source for a module. `plans` has what
/// `bases::plan` made of each class with a base, interfaces or members
/// of its own, by class id.
pub fn shell_source(m: &Module, native_base_name: &str, plans: &BTreeMap<u32, ClassPlan>) -> String {
    let ns = format!("Pwrs.Modules.{}", cs_ident(&m.name));
    let module_class = "PwrsModule";
    let mut s = String::new();
    s.push_str("using System;\nusing System.IO;\nusing System.Management.Automation;\nusing System.Management.Automation.Provider;\nusing System.Runtime.CompilerServices;\nusing System.Runtime.InteropServices;\n\n");
    // Stack frames here are not zeroed on entry. No generated method
    // reads a local before assigning it and none uses stackalloc. The
    // attribute is .NET only; netstandard2.0 has no such type.
    s.push_str("#if NET\n[module: SkipLocalsInit]\n#endif\n\n");
    for c in &m.classes {
        s.push_str(&class_type(module_class, &ns, c, plans.get(&c.id)));
    }
    s.push_str(&format!("namespace {ns}\n{{\n"));
    // A type of its own, so the script setting the field does not run
    // the module class's type initializer, which is what reads it.
    s.push_str("    /// <summary>The module folder, set by the module's own script before\n");
    s.push_str("    /// the shell is imported.</summary>\n");
    s.push_str("    public static class PwrsModuleRoot\n    {\n");
    s.push_str("        public static string? Value;\n");
    s.push_str("    }\n\n");
    s.push_str(&format!("    public static class {module_class}\n    {{\n"));
    // A shell runs from a staged copy, so its own location is the
    // staging folder. The folder its script gave it comes first:
    // the loader maps staging folder to module in one table for the
    // process, and two modules whose folders hash alike share an entry.
    s.push_str("        private static string Root()\n        {\n");
    s.push_str("            string? handed = PwrsModuleRoot.Value;\n");
    s.push_str("            if (handed != null)\n            {\n                return handed;\n            }\n");
    s.push_str(&format!("            string here = typeof({module_class}).Assembly.Location;\n"));
    s.push_str("            return Pwrs.Bootstrap.Loader.ModuleRootOf(here);\n");
    s.push_str("        }\n\n");
    s.push_str(&format!(
        "        internal static readonly Pwrs.NativeModule Native = new Pwrs.NativeModule(Root(), {});\n\n",
        cs_str(native_base_name)
    ));
    // The bootstrap script calls this on every import, so a rebuilt
    // body is picked up by `Import-Module -Force` with nothing more for
    // the author to do.
    s.push_str("        /// <summary>Points the module at its native library again when\n");
    s.push_str("        /// the file has changed since this session loaded it, and returns\n");
    s.push_str("        /// whether it did. A value made before the change is refused by\n");
    s.push_str("        /// its proxy rather than read against the new body.\n");
    s.push_str("        /// </summary>\n");
    s.push_str("        public static bool ReloadNative() => Native.ReloadIfChanged();\n\n");
    let mut blocks = String::new();
    let mut regs = String::new();
    for c in &m.classes {
        let (block, reg) = class_factory(&ns, c);
        blocks.push_str(&block);
        regs.push_str(&reg);
    }
    s.push_str(&format!("        static {module_class}()\n        {{\n            RegisterFactories();\n        }}\n\n"));
    s.push_str("        private static unsafe void RegisterFactories()\n        {\n");
    s.push_str(&regs);
    s.push_str("        }\n    }\n\n");
    s.push_str(&blocks);
    for c in &m.cmdlets {
        let class = type_class(&c.rust, c.id, "Command", m.cmdlets.iter().map(|o| o.rust.as_str()));
        s.push_str(&cmdlet_class(&class, module_class, &ns, c, &m.completers, &m.transforms));
    }
    for cmp in &m.completers {
        s.push_str(&completer_class_source(module_class, cmp));
    }
    for t in &m.transforms {
        s.push_str(&transform_class_source(module_class, t));
    }
    for p in &m.providers {
        let class = type_class(&p.rust, p.id, "Provider", m.providers.iter().map(|o| o.rust.as_str()));
        s.push_str(&provider_class_source(&class, module_class, p));
    }
    s.push_str(&lifecycle_class_source(module_class, m));
    s.push_str("}\n");
    s
}

/// The class the engine calls at import, present only when the module
/// declares a lifecycle hook, so a module without hooks has no such
/// class and is not called. Its import hook runs the import in the
/// runtime, which reloads a rebuilt library first. The module script
/// calls its two statics: one after the import, for the errors that
/// did not fail it, and one from the script's removal block.
fn lifecycle_class_source(module_class: &str, m: &Module) -> String {
    if !m.has_hooks() {
        return String::new();
    }
    let mut s = String::new();
    s.push_str("    public sealed class PwrsModuleLifecycle : IModuleAssemblyInitializer\n    {\n");
    s.push_str(&format!("        public void OnImport() => {module_class}.Native.Imported();\n\n"));
    // Found by reflection, because on Windows PowerShell every module in
    // the process uses the runtime loaded first, and one from an earlier
    // cargo-pwrs has no such method: a direct call would fail the
    // script's catch with a missing method and hide the import's own
    // error. That runtime made no import errors, so there are none.
    s.push_str("        /// <summary>The errors this thread's last import returned without\n");
    s.push_str("        /// failing, for the module script to write; none from a runtime\n");
    s.push_str("        /// without import errors.</summary>\n");
    s.push_str("        public static ErrorRecord[] TakeImportErrors()\n        {\n");
    s.push_str("            System.Reflection.MethodInfo? take = typeof(Pwrs.NativeModule).GetMethod(\"TakeImportErrors\", System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static);\n");
    s.push_str("            return take?.Invoke(null, null) as ErrorRecord[] ?? Array.Empty<ErrorRecord>();\n");
    s.push_str("        }\n\n");
    s.push_str("        /// <summary>Removes the import of the runspace the module script's\n");
    s.push_str("        /// removal block runs in, and returns its hooks' errors.</summary>\n");
    s.push_str(&format!("        public static ErrorRecord[] Removed() => {module_class}.Native.Removed();\n"));
    s.push_str("    }\n\n");
    s
}

/// The NavigationCmdletProvider subclass for one provider. Its
/// capabilities are the attribute's constructor argument, the declared
/// ones named as ProviderCapabilities flags beside ShouldProcess: the
/// attribute's property of that name has no setter, so it takes no
/// named argument.
fn provider_class_source(class: &str, module_class: &str, p: &crate::descriptor::Provider) -> String {
    let flags: Vec<String> = std::iter::once("ShouldProcess")
        .chain(p.capabilities.iter().map(String::as_str))
        .map(|c| format!("ProviderCapabilities.{c}"))
        .collect();
    let mut s = String::new();
    s.push_str(&format!("    [CmdletProvider({}, {})]\n", cs_str(&p.name), flags.join(" | ")));
    s.push_str(&format!("    public sealed class {class} : Pwrs.ProviderBase\n    {{\n"));
    s.push_str(&format!("        protected override Pwrs.NativeModule Module => {module_class}.Native;\n"));
    s.push_str(&format!("        protected override uint ProviderId => {};\n", p.id));
    s.push_str("    }\n\n");
    s
}

/// The ArgumentTransformationAttribute subclass for one transform id.
fn transform_class_source(module_class: &str, t: &Transform) -> String {
    let class = transform_class(t.id);
    let mut s = String::new();
    s.push_str(&format!("    public sealed class {class} : Pwrs.TransformBase\n    {{\n"));
    s.push_str(&format!("        protected override Pwrs.NativeModule Module => {module_class}.Native;\n"));
    s.push_str(&format!("        protected override uint TransformId => {};\n", t.id));
    s.push_str("    }\n\n");
    s
}

/// The IArgumentCompleter subclass for one completer id.
fn completer_class_source(module_class: &str, cmp: &Completer) -> String {
    let class = completer_class(cmp.id);
    let mut s = String::new();
    s.push_str(&format!("    public sealed class {class} : Pwrs.CompleterBase\n    {{\n"));
    s.push_str(&format!("        protected override Pwrs.NativeModule Module => {module_class}.Native;\n"));
    s.push_str(&format!("        protected override uint CompleterId => {};\n", cmp.id));
    s.push_str("    }\n\n");
    s
}

/// Deterministic GUID from the module name, so a rebuilt manifest has
/// the same identity.
fn module_guid(name: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let mut g: u64 = 0x9e3779b97f4a7c15;
    for b in name.bytes().rev() {
        g ^= b as u64;
        g = g.wrapping_mul(0x100000001b3);
    }
    let bytes: Vec<u8> = h.to_le_bytes().iter().chain(g.to_le_bytes().iter()).copied().collect();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:01x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6] & 0x0f,
        bytes[7],
        (bytes[8] & 0x3f) | 0x80,
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

/// A name for the shell assembly that changes with the managed source
/// it is built from.
///
/// PowerShell resolves a cmdlet against a type it has cached by name,
/// so a second load of one assembly identity gives the binder two
/// types with one name and it refuses the cast between them. The
/// assembly the surface produces is therefore named after the surface,
/// and a session that has run the old cmdlet takes the new one.
///
/// The name is stamped rather than the namespace because a binding
/// failure names the type in `FullyQualifiedErrorId`, which scripts
/// match on, and has no assembly name.
pub fn shell_stamp(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        // A separator, so two parts that differ only in where the
        // break falls do not hash alike.
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// The stamp of Pwrs.Bootstrap's source, which names the folder a
/// module's script stages the bootstrap into. A process identifier is
/// reused once its process has exited, so a copy the script finds
/// staged under its process id can be an earlier process's; under the
/// stamp it was built from the same source.
pub fn bootstrap_stamp() -> String {
    shell_stamp(&crate::build::BOOTSTRAP_SOURCES.iter().map(|(_, text)| *text).collect::<Vec<_>>())
}

fn ps_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// PowerShell's classes of verb constants. Each field's name is its
/// value, so `VerbsCommon.Get` reads as `Get`.
const VERB_CLASSES: [&str; 7] = ["VerbsCommon", "VerbsCommunications", "VerbsData", "VerbsDiagnostic", "VerbsLifecycle", "VerbsOther", "VerbsSecurity"];

/// A string literal's text, or the name of one of PowerShell's verb
/// constants such as `VerbsCommon.Get`, with or without its namespace.
/// Any other constant gives no word: its name need not be its value,
/// and the scan does not look the value up.
fn attribute_word(arg: &str) -> Option<String> {
    let arg = arg.trim();
    if let Some(inner) = arg.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(inner.to_string());
    }
    let mut parts = arg.rsplit('.');
    let name = parts.next()?;
    let class = parts.next()?;
    if VERB_CLASSES.contains(&class) && !name.is_empty() && name.bytes().all(is_ident_byte) { Some(name.to_string()) } else { None }
}

/// One hand-written C# cmdlet: the `Verb-Noun` its `[Cmdlet]`
/// attribute declares and the names its `[Alias]` attribute adds.
pub struct HybridCmdlet {
    pub name: String,
    pub aliases: Vec<String>,
}

/// What one hand-written C# file yields.
pub struct HybridScan {
    pub cmdlets: Vec<HybridCmdlet>,
    /// `[Cmdlet(...)]` attributes in the file that produced no entry
    /// in `cmdlets`, because they are not on a class declaration, or
    /// their verb or noun is neither a string literal nor one of
    /// PowerShell's verb constants. A cmdlet behind one of these is not
    /// exported.
    pub unclaimed: usize,
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'@'
}

/// Whether `word` sits at `i` with a non-identifier byte on each side.
fn is_word_at(b: &[u8], i: usize, word: &[u8]) -> bool {
    if i + word.len() > b.len() || &b[i..i + word.len()] != word {
        return false;
    }
    let after = i + word.len();
    (i == 0 || !is_ident_byte(b[i - 1])) && (after >= b.len() || !is_ident_byte(b[after]))
}

/// The index just past the string or character literal at `i`.
fn skip_literal(b: &[u8], i: usize) -> usize {
    let mut i = i;
    if b[i] == b'@' {
        // A verbatim string: no escapes, and `""` is one quote.
        i += 2;
        while i < b.len() {
            if b[i] == b'"' {
                if i + 1 < b.len() && b[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                return i + 1;
            }
            i += 1;
        }
        return i;
    }
    let quote = b[i];
    i += 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// The span of every top-level `[...]` group and the offset of every
/// `class` keyword. Comments and literals are skipped, so neither can
/// be read as code.
fn attribute_groups_and_classes(source: &str) -> (Vec<(usize, usize)>, Vec<usize>) {
    let b = source.as_bytes();
    let mut groups = Vec::new();
    let mut classes = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else if b[i] == b'"' || b[i] == b'\'' || (b[i] == b'@' && i + 1 < b.len() && b[i + 1] == b'"') {
            i = skip_literal(b, i);
        } else if b[i] == b'[' {
            if depth == 0 {
                start = i;
            }
            depth += 1;
            i += 1;
        } else if b[i] == b']' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                groups.push((start, i + 1));
            }
            i += 1;
        } else if depth == 0 && is_word_at(b, i, b"class") {
            classes.push(i);
            i += "class".len();
        } else {
            i += 1;
        }
    }
    (groups, classes)
}

/// Whether the text between an attribute group and the declaration it
/// may belong to contains only whitespace and type modifiers.
fn only_modifiers(gap: &str) -> bool {
    const MODIFIERS: &[&str] =
        &["public", "internal", "private", "protected", "sealed", "static", "abstract", "partial", "unsafe", "new", "file"];
    gap.split_whitespace().all(|w| MODIFIERS.contains(&w))
}

/// The attribute groups belonging to the declaration at `decl`: the
/// run immediately before it, in source order. A group further back
/// than the first break belongs to something else, which is what
/// leaves a parameter's own attributes out.
fn attributes_of(source: &str, groups: &[(usize, usize)], decl: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut next = decl;
    for &(start, end) in groups.iter().rev() {
        if end > next {
            continue;
        }
        if !only_modifiers(&source[end..next]) {
            break;
        }
        out.push((start, end));
        next = start;
    }
    out.reverse();
    out
}

/// The parts of `s` separated by commas outside any bracket or
/// literal.
fn split_top_level(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'"' || b[i] == b'\'' || (b[i] == b'@' && i + 1 < b.len() && b[i + 1] == b'"') {
            i = skip_literal(b, i);
        } else if matches!(b[i], b'(' | b'[' | b'{') {
            depth += 1;
            i += 1;
        } else if matches!(b[i], b')' | b']' | b'}') {
            depth = depth.saturating_sub(1);
            i += 1;
        } else if b[i] == b',' && depth == 0 {
            out.push(&s[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&s[start..]);
    out
}

/// One attribute's bare name and its argument text: a qualified name
/// is cut to its last segment, and the `Attribute` suffix C# allows is
/// dropped.
fn attribute_parts(attr: &str) -> Option<(String, &str)> {
    let attr = attr.trim();
    let (name, args) = match attr.find('(') {
        Some(p) => (&attr[..p], attr[p + 1..].trim_end().strip_suffix(')')?),
        None => (attr, ""),
    };
    let last = name.trim().rsplit('.').next()?.trim();
    let bare = last.strip_suffix("Attribute").unwrap_or(last);
    if bare.is_empty() {
        return None;
    }
    Some((bare.to_string(), args))
}

/// The text of a C# string literal.
fn string_literal(lit: &str) -> Option<String> {
    if let Some(inner) = lit.strip_prefix("@\"").and_then(|s| s.strip_suffix('"')) {
        return Some(inner.replace("\"\"", "\""));
    }
    let inner = lit.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// Every string literal in an attribute's arguments, whether they are
/// written as separate arguments or as one array.
fn string_literals(args: &str) -> Vec<String> {
    let b = args.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'"' || (b[i] == b'@' && i + 1 < b.len() && b[i + 1] == b'"') {
            let end = skip_literal(b, i);
            if let Some(text) = string_literal(&args[i..end]) {
                out.push(text);
            }
            i = end;
        } else if b[i] == b'\'' {
            i = skip_literal(b, i);
        } else {
            i += 1;
        }
    }
    out
}

/// Every hand-written C# cmdlet in one file: a class whose own
/// attribute list has `[Cmdlet(verb, noun, ...)]`, with the aliases
/// that list declares. The verb and noun are read as string literals
/// or PowerShell's verb constants, such as `VerbsCommon.Get`.
/// Attributes inside the class body, a parameter's `[Alias]` among
/// them, belong to the members they are on and are not collected here.
pub fn hybrid_cmdlets(source: &str) -> HybridScan {
    let (groups, classes) = attribute_groups_and_classes(source);
    let mut out = Vec::new();
    for decl in classes {
        let mut name = None;
        let mut aliases = Vec::new();
        for (start, end) in attributes_of(source, &groups, decl) {
            for attr in split_top_level(&source[start + 1..end - 1]) {
                let (attr_name, args) = match attribute_parts(attr) {
                    Some(parts) => parts,
                    None => continue,
                };
                match attr_name.as_str() {
                    "Cmdlet" => {
                        let positional = split_top_level(args);
                        if positional.len() >= 2
                            && let (Some(verb), Some(noun)) = (attribute_word(positional[0]), attribute_word(positional[1]))
                        {
                            name = Some(format!("{verb}-{noun}"));
                        }
                    }
                    "Alias" => aliases.extend(string_literals(args)),
                    _other => {}
                }
            }
        }
        if let Some(name) = name {
            out.push(HybridCmdlet { name, aliases });
        }
    }
    let mut declared = 0usize;
    for &(start, end) in &groups {
        for attr in split_top_level(&source[start + 1..end - 1]) {
            if let Some((attr_name, _args)) = attribute_parts(attr)
                && attr_name == "Cmdlet"
            {
                declared += 1;
            }
        }
    }
    HybridScan { unclaimed: declared.saturating_sub(out.len()), cmdlets: out }
}

/// What the manifest takes from the crate's own metadata.
#[derive(Default)]
pub struct Package<'a> {
    pub version: &'a str,
    pub author: &'a str,
    /// The module's description: `[package.metadata.pwrs] description`
    /// when the crate sets it, otherwise the package's `description`.
    /// Without either the module falls back to its name.
    pub description: Option<&'a str>,
    pub repository: Option<&'a str>,
    /// The gallery's tags after `pwrs`: `[package.metadata.pwrs] tags`
    /// when the crate sets it, otherwise the package's `keywords`.
    pub tags: &'a [String],
    /// A link to the module's license, which a gallery shows beside the
    /// listing. Cargo's own `license` is an SPDX name rather than a
    /// link, so this comes from `[package.metadata.pwrs] license-uri`.
    pub license_uri: Option<&'a str>,
    /// What changed in this version. A cargo manifest has no field for
    /// it; `[package.metadata.pwrs] release-notes` is where it lives.
    pub release_notes: Option<&'a str>,
    /// A link to the image a gallery shows beside the listing, from
    /// `[package.metadata.pwrs] icon-uri`. It is fetched over the
    /// network by whoever renders the listing, so it points at a file on
    /// the web rather than a path inside the module.
    pub icon_uri: Option<&'a str>,
    /// The rest of the module manifest, each from the
    /// `[package.metadata.pwrs]` key of the same name in kebab case. A
    /// cargo manifest has no field for any of them, and a field left
    /// unset is not written, so a manifest has no key without a value.
    pub company: Option<&'a str>,
    pub copyright: Option<&'a str>,
    /// The suffix that marks a version as not yet final, `beta` and the
    /// like. A gallery hides a prerelease from a plain install.
    pub prerelease: Option<&'a str>,
    /// Whether installing prompts for the license first. Written only
    /// when true, because false is what its absence already means.
    pub require_license_acceptance: bool,
    pub external_module_dependencies: &'a [String],
    /// The oldest host the module runs on. Without one the manifest
    /// says 5.1, which is what the generated shells target.
    pub powershell_version: Option<&'a str>,
    /// Without any, the manifest names both editions, which is what the
    /// two generated shells are for.
    pub compatible_ps_editions: &'a [String],
    pub powershell_host_name: Option<&'a str>,
    pub powershell_host_version: Option<&'a str>,
    /// Read by Windows PowerShell only; PowerShell Core ignores both.
    pub dotnet_framework_version: Option<&'a str>,
    pub clr_version: Option<&'a str>,
    pub processor_architecture: Option<&'a str>,
    pub help_info_uri: Option<&'a str>,
    /// A prefix inserted into every exported name at import, for a
    /// caller who has to live beside a module that uses the same ones.
    pub default_command_prefix: Option<&'a str>,
    pub required_modules: &'a [String],
    pub required_assemblies: &'a [String],
    pub scripts_to_process: &'a [String],
    pub types_to_process: &'a [String],
    pub nested_modules: &'a [String],
    pub dsc_resources_to_export: &'a [String],
    pub module_list: &'a [String],
    pub file_list: &'a [String],
}

/// The module manifest. `hybrid` are the cmdlets declared by
/// hand-written C# under `src/csharp/`, exported beside the Rust ones
/// with their own aliases.
pub fn manifest(m: &Module, pkg: &Package<'_>, formats_file: Option<&str>, hybrid: &[HybridCmdlet]) -> String {
    let cmdlets: Vec<String> = m.cmdlets.iter().map(|c| ps_str(&c.name)).chain(hybrid.iter().map(|c| ps_str(&c.name))).collect();
    let aliases: Vec<String> = m
        .cmdlets
        .iter()
        .flat_map(|c| c.aliases.iter())
        .chain(hybrid.iter().flat_map(|c| c.aliases.iter()))
        .map(|a| ps_str(a))
        .collect();
    let description = match pkg.description {
        Some(d) => d.to_string(),
        None => format!("{} (built with pwrs)", m.name),
    };
    // `pwrs` stays in the tag list whatever the crate declares, and a
    // gallery tag may not contain whitespace.
    let mut tags: Vec<String> = vec!["pwrs".to_string()];
    for k in pkg.tags {
        let tag = k.replace(char::is_whitespace, "-");
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    let tag_list: Vec<String> = tags.iter().map(|t| ps_str(t)).collect();
    // A key is written only where there is a value for it, so the
    // manifest has no empty one for a gallery to read as set.
    fn opt(lines: &mut Vec<String>, name: &str, v: Option<&str>) {
        if let Some(s) = v {
            lines.push(format!("    {name} = {}", ps_str(s)));
        }
    }
    fn list(lines: &mut Vec<String>, name: &str, xs: &[String]) {
        if !xs.is_empty() {
            let quoted: Vec<String> = xs.iter().map(|x| ps_str(x)).collect();
            lines.push(format!("    {name} = @({})", quoted.join(", ")));
        }
    }

    // The two generated shells target both editions and 5.1, which is
    // what a module says when its author names nothing else.
    let editions: Vec<String> =
        if pkg.compatible_ps_editions.is_empty() { vec!["Desktop".to_string(), "Core".to_string()] } else { pkg.compatible_ps_editions.to_vec() };
    let formats: Vec<String> = match formats_file {
        Some(f) => vec![f.to_string()],
        None => Vec::new(),
    };

    // PSData has what a gallery reads. Tags is always written because
    // the tag list always includes `pwrs`.
    let mut psdata: Vec<String> = vec![format!("Tags = @({})", tag_list.join(", "))];
    for (name, value) in [
        ("LicenseUri", pkg.license_uri),
        ("ProjectUri", pkg.repository),
        ("IconUri", pkg.icon_uri),
        ("ReleaseNotes", pkg.release_notes),
        ("Prerelease", pkg.prerelease),
    ] {
        if let Some(s) = value {
            psdata.push(format!("{name} = {}", ps_str(s)));
        }
    }
    if pkg.require_license_acceptance {
        psdata.push("RequireLicenseAcceptance = $true".to_string());
    }
    if !pkg.external_module_dependencies.is_empty() {
        let quoted: Vec<String> = pkg.external_module_dependencies.iter().map(|x| ps_str(x)).collect();
        psdata.push(format!("ExternalModuleDependencies = @({})", quoted.join(", ")));
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("    RootModule = {}", ps_str(&format!("{}.psm1", m.name))));
    lines.push(format!("    ModuleVersion = {}", ps_str(pkg.version)));
    list(&mut lines, "CompatiblePSEditions", &editions);
    lines.push(format!("    GUID = {}", ps_str(&module_guid(&m.name))));
    lines.push(format!("    Author = {}", ps_str(pkg.author)));
    opt(&mut lines, "CompanyName", pkg.company);
    opt(&mut lines, "Copyright", pkg.copyright);
    lines.push(format!("    Description = {}", ps_str(&description)));
    lines.push(format!("    PowerShellVersion = {}", ps_str(pkg.powershell_version.unwrap_or("5.1"))));
    opt(&mut lines, "PowerShellHostName", pkg.powershell_host_name);
    opt(&mut lines, "PowerShellHostVersion", pkg.powershell_host_version);
    opt(&mut lines, "DotNetFrameworkVersion", pkg.dotnet_framework_version);
    opt(&mut lines, "ClrVersion", pkg.clr_version);
    opt(&mut lines, "ProcessorArchitecture", pkg.processor_architecture);
    list(&mut lines, "RequiredModules", pkg.required_modules);
    list(&mut lines, "RequiredAssemblies", pkg.required_assemblies);
    list(&mut lines, "ScriptsToProcess", pkg.scripts_to_process);
    list(&mut lines, "TypesToProcess", pkg.types_to_process);
    list(&mut lines, "FormatsToProcess", &formats);
    list(&mut lines, "NestedModules", pkg.nested_modules);
    lines.push("    FunctionsToExport = @()".to_string());
    lines.push(format!("    CmdletsToExport = @({})", cmdlets.join(", ")));
    lines.push("    VariablesToExport = @()".to_string());
    lines.push(format!("    AliasesToExport = @({})", aliases.join(", ")));
    list(&mut lines, "DscResourcesToExport", pkg.dsc_resources_to_export);
    list(&mut lines, "ModuleList", pkg.module_list);
    list(&mut lines, "FileList", pkg.file_list);
    lines.push(format!("    PrivateData = @{{ PSData = @{{ {} }} }}", psdata.join("; ")));
    opt(&mut lines, "HelpInfoURI", pkg.help_info_uri);
    opt(&mut lines, "DefaultCommandPrefix", pkg.default_command_prefix);

    format!("@{{\n{}\n}}\n", lines.join("\n"))
}

/// A module bundled inside this one, and whether a failed import of it
/// writes a warning and lets this module's import go on, instead of
/// failing it.
pub struct Bundled<'a> {
    pub name: &'a str,
    pub warn_on_failure: bool,
}

/// The step of `module`'s script that imports the modules bundled inside it,
/// in order, before its own shell: each from `<Name>/<Name>.psd1` beside
/// the script, leaving one already loaded in the session as it is. A failed
/// import fails `module`'s import with that error; for a module that
/// warns on failure, a warning names it and the error instead, and the
/// import goes on without it. Empty when nothing is bundled.
///
/// A module already loaded under the name is the one that stays, whichever
/// folder it came from: PowerShell 7 refuses a second folder of a loaded
/// module, since its cmdlet names are taken. -Global puts the import
/// where a user's own Import-Module would, so the bundled cmdlets are
/// callable from the session and not only from this script, and
/// -DisableNameChecking leaves the bundled module's unapproved-verb
/// warnings, addressed to its author, out of this module's import.
pub fn bundled_step(module: &str, bundled: &[Bundled]) -> String {
    if bundled.is_empty() {
        return String::new();
    }
    let names: Vec<String> = bundled.iter().map(|b| ps_str(b.name)).collect();
    let warned: Vec<String> = bundled.iter().filter(|b| b.warn_on_failure).map(|b| ps_str(b.name)).collect();
    if warned.is_empty() {
        return format!(
            "foreach ($bundled in @({names})) {{\n\
    if (-not (Get-Module -Name $bundled)) {{\n\
        Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop\n\
    }}\n\
}}\n",
            names = names.join(", ")
        );
    }
    format!(
        "foreach ($bundled in @({names})) {{\n\
    if (-not (Get-Module -Name $bundled)) {{\n\
        try {{\n\
            Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop\n\
        }}\n\
        catch {{\n\
            if (@({warned}) -notcontains $bundled) {{ throw }}\n\
            Microsoft.PowerShell.Utility\\Write-Warning ({module} + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)\n\
        }}\n\
    }}\n\
}}\n",
        names = names.join(", "),
        warned = warned.join(", "),
        module = ps_str(module)
    )
}

/// The end of the module script's catch around the shell import: the
/// first PwrsException inside what was caught, or else the innermost
/// exception, thrown as its own record when it has one.
const RETHROW: &str = "$inner = $_.Exception\n\
while ($null -ne $inner.InnerException -and $inner.GetType().FullName -ne 'Pwrs.PwrsException') { $inner = $inner.InnerException }\n\
if ($inner -is [System.Management.Automation.IContainsErrorRecord]) { throw $inner.ErrorRecord }\n\
throw $inner\n";

/// The module's script. When the bootstrap's loader cannot be reached on
/// a PowerShell 7 whose .NET is older than the .NET 8 the `net10.0`
/// assemblies reference, it names the PowerShell the module needs in
/// place of the missing-type error, which names neither; a load that
/// succeeds never evaluates the check. `desktop_dependencies` names
/// files the build placed in `netstandard2.0` for Windows PowerShell to
/// load beside the shell; the script copies each next to the staged
/// shell, where .NET Framework looks for a shell's dependencies, and
/// PowerShell 7 skips the step. `bundled` names the modules bundled inside
/// this one, which [`bundled_step`] imports before the shell.
///
/// Every command the script runs is in Microsoft.PowerShell.Core, and the
/// bundled step's warning is called by its module-qualified name: a
/// command from a module not yet loaded makes PowerShell analyze every
/// module on PSModulePath that is not in its analysis cache.
pub fn bootstrap_psm1(m: &Module, hybrid: &[HybridCmdlet], desktop_dependencies: &[&str], bundled: &[Bundled]) -> String {
    let names: Vec<String> = m.cmdlets.iter().map(|c| ps_str(&c.name)).chain(hybrid.iter().map(|c| ps_str(&c.name))).collect();
    let cmdlets: String = if names.is_empty() { "@()".to_string() } else { names.join(", ") };
    let bundled_step = bundled_step(&m.name, bundled);
    // The engine creates a cmdlet's [Alias] members inside the nested
    // binary module, so the root module has to export them by name or
    // they never reach the session. A hand-written C# cmdlet declares
    // its aliases the same way and needs the same treatment.
    let alias_names: Vec<String> = m
        .cmdlets
        .iter()
        .flat_map(|c| c.aliases.iter())
        .chain(hybrid.iter().flat_map(|c| c.aliases.iter()))
        .map(|a| ps_str(a))
        .collect();
    let aliases: String = if alias_names.is_empty() { "@()".to_string() } else { alias_names.join(", ") };
    // Each dependency is copied under a temporary name and renamed into
    // place, so a file another import finds there is whole; a rename that
    // fails counts only when the file is still not there.
    let desktop_step = if desktop_dependencies.is_empty() {
        String::new()
    } else {
        let deps: Vec<String> = desktop_dependencies.iter().map(|d| ps_str(d)).collect();
        format!(
            "if ($tfm -eq 'netstandard2.0') {{\n\
$staged = [System.IO.Path]::GetDirectoryName($shell.Location)\n\
foreach ($dep in @({deps})) {{\n\
$to = [System.IO.Path]::Combine($staged, $dep)\n\
if (-not [System.IO.File]::Exists($to)) {{\n\
$partial = $to + '.' + [System.Guid]::NewGuid().ToString('N') + '.partial'\n\
[System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, $dep), $partial)\n\
try {{ [System.IO.File]::Move($partial, $to) }}\n\
catch [System.IO.IOException] {{\n\
[System.IO.File]::Delete($partial)\n\
if (-not [System.IO.File]::Exists($to)) {{ throw }}\n\
}}\n\
}}\n\
}}\n\
}}\n",
            deps = deps.join(", ")
        )
    };
    // The native library loads in the shell's type initializer, which the
    // lifecycle class's import hook reaches while the engine imports the
    // shell and ReloadNative reaches otherwise, so a refusal there, such
    // as the CPU check's, arrives wrapped in a TypeInitializationException.
    // The script rethrows the first PwrsException inside what it caught,
    // or else the innermost exception, whose message is the reason. It
    // throws an exception's own record when it has one: `throw` on a .NET
    // exception makes a record whose id is the message, which would lose
    // the id and category a Rust error gives its record.
    //
    // For a module with lifecycle hooks the import hook reloads a rebuilt
    // library itself. The removal hooks run from the script module's
    // removal block rather than from IModuleAssemblyCleanup: an error the
    // block writes reaches Remove-Module, which goes ahead, while an
    // exception from a binary module's removal hook stops Remove-Module
    // and leaves the module imported. The errors an import returns
    // without failing, and the block's, are written by Write-Error's
    // module-qualified name, which loads Microsoft.PowerShell.Utility
    // only when there is an error to write, and with -ErrorAction
    // Continue: under an error preference of Stop, a write without it
    // throws, and in the block that stops Remove-Module and leaves the
    // module imported.
    let import_step = if m.has_hooks() {
        format!(
            "$lifecycle = $shell.GetType({lifecycle_type}, $true)\n\
try {{\n\
Import-Module -Assembly $shell -Force\n\
}}\n\
catch {{\n\
foreach ($e in $lifecycle::TakeImportErrors()) {{ Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue }}\n\
{rethrow}\
}}\n\
foreach ($e in $lifecycle::TakeImportErrors()) {{ Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue }}\n\
$ExecutionContext.SessionState.Module.OnRemove = {{ foreach ($e in $lifecycle::Removed()) {{ Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue }} }}\n",
            lifecycle_type = ps_str(&format!("Pwrs.Modules.{}.PwrsModuleLifecycle", cs_ident(&m.name))),
            rethrow = RETHROW,
        )
    } else {
        format!(
            "try {{\n\
Import-Module -Assembly $shell -Force\n\
$reload = $null\n\
foreach ($type in $shell.GetTypes()) {{ if ($type.Name -eq 'PwrsModule') {{ $reload = $type; break }} }}\n\
if ($null -ne $reload) {{ $null = $reload::ReloadNative() }}\n\
}}\n\
catch {{\n\
{rethrow}\
}}\n",
            rethrow = RETHROW,
        )
    };
    // Runspaces importing at the same instant run the script side by
    // side. The bootstrap is staged in a folder named for the process,
    // the stamp of its source and the edition, under a temporary name
    // renamed into place, so a copy another import finds there is whole
    // and built from the same source; LoadFrom returns the assembly
    // already loaded for a second copy of it. Its .pdb, when the build
    // wrote one, goes in first by the same rule, so a staged bootstrap
    // has its symbols beside it.
    format!(
        "{bundled_step}\
$root = $PSScriptRoot\n\
$tfm = if ($PSVersionTable.PSEdition -eq 'Core') {{ 'net10.0' }} else {{ 'netstandard2.0' }}\n\
if (-not ('Pwrs.Bootstrap.Loader' -as [type])) {{\n\
    $stage = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'pwrs-load', [string]$PID, 'boot', {boot_stamp}, $tfm)\n\
    $null = [System.IO.Directory]::CreateDirectory($stage)\n\
    $boot = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.dll')\n\
    if (-not [System.IO.File]::Exists($boot)) {{\n\
        $pdb = [System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.pdb')\n\
        $bootPdb = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.pdb')\n\
        if ([System.IO.File]::Exists($pdb) -and -not [System.IO.File]::Exists($bootPdb)) {{\n\
            $partial = $bootPdb + '.' + [System.Guid]::NewGuid().ToString('N') + '.partial'\n\
            [System.IO.File]::Copy($pdb, $partial)\n\
            try {{ [System.IO.File]::Move($partial, $bootPdb) }}\n\
            catch [System.IO.IOException] {{\n\
                [System.IO.File]::Delete($partial)\n\
                if (-not [System.IO.File]::Exists($bootPdb)) {{ throw }}\n\
            }}\n\
        }}\n\
        $partial = $boot + '.' + [System.Guid]::NewGuid().ToString('N') + '.partial'\n\
        [System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.dll'), $partial)\n\
        try {{ [System.IO.File]::Move($partial, $boot) }}\n\
        catch [System.IO.IOException] {{\n\
            [System.IO.File]::Delete($partial)\n\
            if (-not [System.IO.File]::Exists($boot)) {{ throw }}\n\
        }}\n\
    }}\n\
    $null = [System.Reflection.Assembly]::LoadFrom($boot)\n\
}}\n\
try {{ $shell = [Pwrs.Bootstrap.Loader]::Load($root, {name}, $tfm) }}\n\
catch {{\n\
    if ($tfm -eq 'net10.0' -and [Environment]::Version.Major -lt 8) {{\n\
        throw ({name} + ' needs PowerShell 7.4 or later: its PowerShell 7 half is built against .NET 8, and this is PowerShell ' + $PSVersionTable.PSVersion + ' on .NET ' + [Environment]::Version + '.')\n\
    }}\n\
    throw\n\
}}\n\
$shell.GetType({root_type}, $true).GetField('Value').SetValue($null, $root)\n\
{desktop_step}\
foreach ($m in Get-Module -All) {{\n\
    if ($m.ModuleType -eq 'Binary' -and ($m.Path -eq $shell.Location -or $m.Name -like {shell_glob})) {{\n\
        Remove-Module -ModuleInfo $m -Force -ErrorAction SilentlyContinue\n\
    }}\n\
}}\n\
{import_step}\
Export-ModuleMember -Cmdlet {cmdlets} -Alias {aliases}\n",
        bundled_step = bundled_step,
        import_step = import_step,
        boot_stamp = ps_str(&bootstrap_stamp()),
        shell_glob = ps_str(&format!("*{}.Shell*", m.name)),
        name = ps_str(&m.name),
        root_type = ps_str(&format!("Pwrs.Modules.{}.PwrsModuleRoot", cs_ident(&m.name))),
        desktop_step = desktop_step,
        cmdlets = cmdlets,
        aliases = aliases,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shell of a module none of whose classes names a base,
    /// interfaces or members.
    fn unplanned(module: &Module, native_base_name: &str) -> String {
        shell_source(module, native_base_name, &BTreeMap::new())
    }

    fn found(source: &str) -> Vec<(String, Vec<String>)> {
        hybrid_cmdlets(source).cmdlets.into_iter().map(|c| (c.name, c.aliases)).collect()
    }

    #[test]
    fn a_cmdlet_attribute_that_yields_no_cmdlet_is_counted_rather_than_dropped() {
        let orphan = "[Cmdlet(VerbsCommon.Get, \"Thing\")]\npublic enum NotAClass { A }";
        let scan = hybrid_cmdlets(orphan);
        assert!(scan.cmdlets.is_empty(), "nothing to export");
        assert_eq!(scan.unclaimed, 1, "the attribute is on no class");

        // Named arguments the verb and noun reader does not accept.
        let unreadable = "[Cmdlet(verb: VerbsCommon.Get, noun: \"Thing\")]\npublic sealed class C : PSCmdlet { }";
        let scan = hybrid_cmdlets(unreadable);
        assert!(scan.cmdlets.is_empty(), "no name could be read");
        assert_eq!(scan.unclaimed, 1, "the attribute is on a class but yielded nothing");

        // A module's own constant: its name is not its value, which
        // the source does not show here, so no name is made from it.
        let own_constant = "[Cmdlet(MyVerbs.Fetch, \"Thing\")]\npublic sealed class C : PSCmdlet { }";
        let scan = hybrid_cmdlets(own_constant);
        assert!(scan.cmdlets.is_empty(), "Fetch-Thing is not the cmdlet's name: {:?}", found(own_constant));
        assert_eq!(scan.unclaimed, 1, "the attribute is counted, so the build warns about it");

        let own_noun = "[Cmdlet(VerbsCommon.Get, Names.Thing)]\npublic sealed class C : PSCmdlet { }";
        let scan = hybrid_cmdlets(own_noun);
        assert!(scan.cmdlets.is_empty(), "a noun constant is not read: {:?}", found(own_noun));
        assert_eq!(scan.unclaimed, 1);

        let ordinary = "[Cmdlet(VerbsCommon.Get, \"Thing\")]\n[Alias(\"gt\")]\npublic sealed class C : PSCmdlet\n{\n    [Parameter]\n    [Alias(\"n\")]\n    public string Name { get; set; } = string.Empty;\n}";
        let scan = hybrid_cmdlets(ordinary);
        assert_eq!(scan.cmdlets.len(), 1);
        assert_eq!(scan.unclaimed, 0, "a parameter's attributes are not miscounted");
    }

    #[test]
    fn reads_the_cmdlet_name_from_literals_and_verb_constants() {
        let source = r#"
            [Cmdlet(VerbsCommon.Get, "Thing")]
            public sealed class GetThingCommand : PSCmdlet { }

            [Cmdlet("Set", "Thing", SupportsShouldProcess = true)]
            public class SetThingCommand : PSCmdlet { }

            [Cmdlet(System.Management.Automation.VerbsData.ConvertTo, "Thing")]
            public class ConvertToThingCommand : PSCmdlet { }
        "#;
        assert_eq!(
            found(source),
            vec![("Get-Thing".to_string(), vec![]), ("Set-Thing".to_string(), vec![]), ("ConvertTo-Thing".to_string(), vec![])]
        );
    }

    #[test]
    fn collects_class_aliases_before_or_after_the_cmdlet_attribute() {
        let after = "[Cmdlet(VerbsCommon.Get, \"Thing\")]\n[Alias(\"gt\", \"getit\")]\npublic sealed class C : PSCmdlet { }";
        let before = "[Alias(\"gt\", \"getit\")]\n[Cmdlet(VerbsCommon.Get, \"Thing\")]\npublic sealed class C : PSCmdlet { }";
        let together = "[Cmdlet(VerbsCommon.Get, \"Thing\"), Alias(\"gt\", \"getit\")]\npublic sealed class C : PSCmdlet { }";
        let expected = vec![("Get-Thing".to_string(), vec!["gt".to_string(), "getit".to_string()])];
        assert_eq!(found(after), expected, "alias after");
        assert_eq!(found(before), expected, "alias before");
        assert_eq!(found(together), expected, "one group");
    }

    #[test]
    fn leaves_a_parameter_alias_to_its_parameter() {
        let source = r#"
            [Cmdlet(VerbsCommon.Get, "Thing")]
            [Alias("gt")]
            public sealed class GetThingCommand : PSCmdlet
            {
                [Parameter(Mandatory = true)]
                [Alias("n", "who")]
                public string Name { get; set; } = string.Empty;
            }
        "#;
        assert_eq!(found(source), vec![("Get-Thing".to_string(), vec!["gt".to_string()])]);
    }

    #[test]
    fn a_second_class_does_not_inherit_the_first_ones_attributes() {
        let source = r#"
            [Cmdlet(VerbsCommon.Get, "One")]
            [Alias("one")]
            public sealed class GetOneCommand : PSCmdlet
            {
                public string[] Names { get; set; } = new string[0];
            }

            public sealed class Helper { }

            [Cmdlet(VerbsCommon.Get, "Two")]
            public sealed class GetTwoCommand : PSCmdlet { }
        "#;
        assert_eq!(found(source), vec![("Get-One".to_string(), vec!["one".to_string()]), ("Get-Two".to_string(), vec![])]);
    }

    #[test]
    fn ignores_a_cmdlet_attribute_in_a_comment_or_a_string() {
        let source = r#"
            // [Cmdlet(VerbsCommon.Get, "Commented")]
            /* [Cmdlet(VerbsCommon.Get, "Blocked")] */
            public sealed class Note
            {
                public string Text = "[Cmdlet(VerbsCommon.Get, \"Quoted\")]";
                public string Verbatim = @"[Cmdlet(VerbsCommon.Get, ""Verbatim"")]";
            }

            [Cmdlet(VerbsCommon.Get, "Real")]
            public sealed class GetRealCommand : PSCmdlet { }
        "#;
        assert_eq!(found(source), vec![("Get-Real".to_string(), vec![])]);
    }

    #[test]
    fn takes_aliases_written_as_an_array_and_qualified_attribute_names() {
        let source = "[CmdletAttribute(VerbsCommon.Get, \"Thing\")]\n[System.Management.Automation.Alias(new[] { \"gt\" })]\npublic sealed class C : PSCmdlet { }";
        assert_eq!(found(source), vec![("Get-Thing".to_string(), vec!["gt".to_string()])]);
    }

    #[test]
    fn a_class_without_a_cmdlet_attribute_is_not_a_cmdlet() {
        let source = "[Alias(\"nope\")]\npublic sealed class Plain { }";
        assert!(found(source).is_empty());
    }

    #[test]
    fn hybrid_aliases_reach_the_manifest_and_the_bootstrap_module() {
        let module = Module {
            abi: 1,
            name: "Demo".to_string(),
            cmdlets: Vec::new(),
            classes: Vec::new(),
            completers: Vec::new(),
            transforms: Vec::new(),
            providers: Vec::new(),
            on_import: false,
            on_remove: false,
            on_load: false,
            on_unload: false,
        };
        let pkg = Package { version: "0.1.0", author: "a", ..Package::default() };
        let hybrid = vec![HybridCmdlet { name: "Get-Thing".to_string(), aliases: vec!["gt".to_string()] }];
        let psd1 = manifest(&module, &pkg, None, &hybrid);
        assert!(psd1.contains("CmdletsToExport = @('Get-Thing')"), "{psd1}");
        assert!(psd1.contains("AliasesToExport = @('gt')"), "{psd1}");
        // A field the crate did not set leaves no empty key behind.
        for absent in ["LicenseUri", "ReleaseNotes", "IconUri"] {
            assert!(!psd1.contains(absent), "{absent} written for a crate that set none: {psd1}");
        }

        let described = Package {
            version: "0.1.0",
            author: "a",
            repository: Some("https://example.invalid/thing"),
            license_uri: Some("https://example.invalid/thing/blob/main/license"),
            release_notes: Some("First release."),
            icon_uri: Some("https://example.invalid/thing/raw/main/icon.png"),
            ..Package::default()
        };
        let psd1 = manifest(&module, &described, None, &hybrid);
        assert!(psd1.contains("LicenseUri = 'https://example.invalid/thing/blob/main/license'"), "{psd1}");
        assert!(psd1.contains("ReleaseNotes = 'First release.'"), "{psd1}");
        assert!(psd1.contains("ProjectUri = 'https://example.invalid/thing'"), "{psd1}");
        assert!(psd1.contains("IconUri = 'https://example.invalid/thing/raw/main/icon.png'"), "{psd1}");
        let psm1 = bootstrap_psm1(&module, &hybrid, &[], &[]);
        assert!(psm1.contains("Export-ModuleMember -Cmdlet 'Get-Thing' -Alias 'gt'"), "{psm1}");
    }

    /// The script passes its shell the module folder after the loader
    /// returns the shell and before the shell is imported, and the shell
    /// reads that folder before it looks in the loader's map.
    #[test]
    fn the_script_passes_its_shell_the_module_root_before_import() {
        let module = demo_module();
        let psm1 = bootstrap_psm1(&module, &[], &[], &[]);
        let load = psm1.find("$shell = [Pwrs.Bootstrap.Loader]::Load(").expect("the load line");
        let hand = psm1
            .find("$shell.GetType('Pwrs.Modules.Demo.PwrsModuleRoot', $true).GetField('Value').SetValue($null, $root)")
            .expect("the handoff line");
        let import = psm1.find("Import-Module -Assembly $shell").expect("the import line");
        assert!(load < hand && hand < import, "{psm1}");

        let shell = unplanned(&module, "demo");
        assert!(shell.contains("public static class PwrsModuleRoot"), "{shell}");
        let handed = shell.find("string? handed = PwrsModuleRoot.Value;").expect("the root from the script");
        let mapped = shell.find("Pwrs.Bootstrap.Loader.ModuleRootOf(here)").expect("the loader's map");
        assert!(handed < mapped, "{shell}");
    }

    /// A `begin` found empty is skipped, and the shell of a cmdlet with
    /// parameters clears its dirty word in its place, so the first
    /// record's phase does not count the arguments as piped. A cmdlet
    /// with none has no dirty word.
    #[test]
    fn a_skipped_begin_clears_the_dirty_word_of_a_cmdlet_with_parameters() {
        let cmdlet = |id: u32, noun: &str, params: &str| {
            format!(
                r#"{{"id": {id}, "verb": "Get", "noun": "{noun}", "name": "Get-{noun}", "rust": "Get{noun}",
                    "should_process": false, "confirm_impact": null, "default_set": null,
                    "aliases": [], "output_types": [], "synopsis": "", "description": "", "params": [{params}]}}"#
            )
        };
        let param = r#"{"name": "Name", "rust": "name", "index": 0, "clr": "System.String",
            "slot": "str16", "optional": false, "mandatory": true, "position": 0, "set": null,
            "pipeline": true, "pipeline_by_name": false, "remaining": false, "aliases": [],
            "help": "", "validate_set": [], "validate_range": null, "validate_pattern": null,
            "not_null_or_empty": false, "dont_show": false, "literal_path": false}"#;
        let json = format!(r#"{{"abi": 1, "name": "Demo", "cmdlets": [{}, {}]}}"#, cmdlet(0, "Named", param), cmdlet(1, "Bare", ""));
        let module: Module = serde_json::from_str(&json).expect("descriptor");
        let shell = unplanned(&module, "demo");
        let begins: Vec<&str> = shell.lines().filter(|l| l.contains("protected override void BeginProcessing()")).collect();
        assert_eq!(begins.len(), 2, "{shell}");
        assert!(begins[0].contains("Run(Pwrs.Native.PhaseBegin); else _dirty = 0; }"), "{}", begins[0]);
        assert!(!begins[1].contains("_dirty"), "{}", begins[1]);
    }

    /// A module of one cmdlet whose `n` parameters, `A0` to `A{n-1}`, are
    /// each a long.
    fn wide_cmdlet(n: u32) -> Module {
        let params: Vec<String> = (0..n)
            .map(|i| {
                format!(
                    r#"{{"name": "A{i}", "rust": "a{i}", "index": {i}, "clr": "long", "slot": "i64", "optional": false,
                        "mandatory": false, "position": null, "set": null, "pipeline": false, "pipeline_by_name": false,
                        "remaining": false, "aliases": [], "help": "", "validate_set": [], "validate_range": null,
                        "validate_pattern": null, "not_null_or_empty": false, "dont_show": false, "literal_path": false}}"#
                )
            })
            .collect();
        let json = format!(
            r#"{{"abi": 1, "name": "Demo", "cmdlets": [{{"id": 0, "verb": "Measure", "noun": "Wide", "name": "Measure-Wide", "rust": "MeasureWide",
                "should_process": false, "confirm_impact": null, "default_set": null, "aliases": [], "output_types": [],
                "synopsis": "", "description": "", "params": [{}]}}]}}"#,
            params.join(", ")
        );
        serde_json::from_str(&json).expect("a descriptor")
    }

    /// Past 64 parameters each mask adds a word, named with its number,
    /// which the setters, the block, `Run` and a skipped `begin` all use; a
    /// cmdlet of 64 declares the one word of each it always had.
    #[test]
    fn a_cmdlet_past_64_parameters_has_a_word_of_each_mask_per_64() {
        let shell = unplanned(&wide_cmdlet(64), "demo");
        for later in ["private ulong _bound1;", "private ulong _dirty1;", "public ulong Dirty1;", "public ulong Bound1;"] {
            assert!(!shell.contains(later), "64 parameters declare {later}: {shell}");
        }
        assert!(shell.contains("public long A63 { get => _p63; set { _p63 = value; _bound |= 1UL << 63; _dirty |= 1UL << 63; } }"), "{shell}");
        assert!(shell.contains("Run(Pwrs.Native.PhaseBegin); else _dirty = 0; }"), "{shell}");

        let shell = unplanned(&wide_cmdlet(130), "demo");
        let fields = "        private ulong _bound;\n        private ulong _bound1;\n        private ulong _bound2;\n        private ulong _dirty;\n        private ulong _dirty1;\n        private ulong _dirty2;\n\n";
        assert!(shell.contains(fields), "{shell}");
        assert!(!shell.contains("_bound3"), "{shell}");
        for (i, word, bit) in [(0, "", 0), (63, "", 63), (64, "1", 0), (65, "1", 1), (127, "1", 63), (128, "2", 0), (129, "2", 1)] {
            let setter = format!("public long A{i} {{ get => _p{i}; set {{ _p{i} = value; _bound{word} |= 1UL << {bit}; _dirty{word} |= 1UL << {bit}; }} }}");
            assert!(shell.contains(&setter), "no `{setter}`: {shell}");
        }
        let block = "            public ulong Dirty;\n            public ulong Dirty1;\n            public ulong Dirty2;\n            public ulong Bound;\n            public ulong Bound1;\n            public ulong Bound2;\n            public long P0;\n";
        assert!(shell.contains(block), "{shell}");
        assert!(shell.contains("            b.Dirty2 = _dirty2;\n            _dirty2 = 0;\n            b.Bound2 = _bound2;\n"), "{shell}");
        assert!(shell.contains("Run(Pwrs.Native.PhaseBegin); else { _dirty = 0; _dirty1 = 0; _dirty2 = 0; } }"), "{shell}");
    }

    /// Each validation key writes its attribute on the property, the
    /// script as a verbatim string, and `validate` refuses a null key on
    /// a parameter whose type is a value type.
    #[test]
    fn the_validation_keys_write_their_attributes_and_a_null_key_needs_a_reference_type() {
        let param = |extra: &str, clr: &str, value_type: bool| {
            format!(
                r#"{{"name": "Size", "rust": "size", "index": 0, "clr": "{clr}", "value_type": {value_type},
                    "slot": "handle", "optional": false, "mandatory": true, "position": null,
                    "pipeline": false, "pipeline_by_name": false, "remaining": false, "aliases": [],
                    "help": "", "validate_set": [], "validate_range": null, "validate_pattern": null,
                    "not_null_or_empty": false, "dont_show": false, "literal_path": false{extra}}}"#
            )
        };
        let every = param(
            r#", "validate_length": [1, 8], "validate_count": [1, 3], "validate_script": "$_ -ne \"x\"", "validate_not_null": true, "supports_wildcards": true"#,
            "string[]",
            false,
        );
        let attrs = param_attributes(&serde_json::from_str::<Param>(&every).expect("a parameter"));
        for line in ["[ValidateLength(1, 8)]", "[ValidateCount(1, 3)]", "[Pwrs.ValidateScriptText(@\"$_ -ne \"\"x\"\"\")]", "[ValidateNotNull]", "[SupportsWildcards]"] {
            assert!(attrs.contains(line), "{line} is missing from:\n{attrs}");
        }
        assert!(!attrs.contains("[AllowNull]"), "{attrs}");
        let module = |p: &str| -> Module {
            serde_json::from_str(&format!(
                r#"{{"abi": 1, "name": "Demo", "cmdlets": [{{"id": 0, "verb": "Get", "noun": "Thing", "name": "Get-Thing", "rust": "GetThing",
                    "should_process": false, "confirm_impact": null, "default_set": null,
                    "aliases": [], "output_types": [], "synopsis": "", "description": "", "params": [{p}]}}]}}"#
            ))
            .expect("descriptor")
        };
        validate(&module(&param(r#", "allow_null": true"#, "string", false))).expect("allow_null on a string");
        let refused = validate(&module(&param(r#", "allow_null": true"#, "Demo.Color", true))).expect_err("allow_null on an enum");
        assert!(refused.to_string().contains("Get-Thing's parameter -Size declares allow_null, and its type Demo.Color cannot be null"), "{refused}");
    }

    /// A file the build ships for Windows PowerShell is copied beside
    /// the staged shell after the handoff and before the import, on
    /// that edition only; a module shipping none has no such step.
    #[test]
    fn the_script_stages_desktop_dependencies_beside_the_shell() {
        let module = demo_module();
        let none = bootstrap_psm1(&module, &[], &[], &[]);
        assert!(!none.contains("$dep"), "{none}");

        let psm1 = bootstrap_psm1(&module, &[], &["System.Numerics.Vectors.dll"], &[]);
        let hand = psm1.find("GetField('Value').SetValue($null, $root)").expect("the handoff line");
        let step = psm1.find("if ($tfm -eq 'netstandard2.0') {").expect("the desktop step");
        let list = psm1.find("foreach ($dep in @('System.Numerics.Vectors.dll')) {").expect("the dependency list");
        let import = psm1.find("Import-Module -Assembly $shell").expect("the import line");
        assert!(hand < step && step < list && list < import, "{psm1}");
        assert!(psm1.contains("$staged = [System.IO.Path]::GetDirectoryName($shell.Location)"), "{psm1}");
    }

    /// A PowerShell 7 host on a .NET older than 8 is told the PowerShell
    /// the module needs when the loader cannot be reached; the check sits
    /// in the catch of the loader call, so a load that succeeds never
    /// reads it, and anything else that fails there is thrown unchanged.
    #[test]
    fn the_script_names_the_floor_only_when_the_loader_fails() {
        let psm1 = bootstrap_psm1(&demo_module(), &[], &[], &[]);
        let boot = psm1.find("'Pwrs.Bootstrap.dll'").expect("the bootstrap load");
        let call = psm1.find("try { $shell = [Pwrs.Bootstrap.Loader]::Load($root, 'Demo', $tfm) }").expect("the guarded loader call");
        let catch = psm1.find("catch {\n").expect("the catch");
        let floor = psm1.find("if ($tfm -eq 'net10.0' -and [Environment]::Version.Major -lt 8) {").expect("the floor check");
        let rethrow = psm1.find("\nthrow\n}").expect("the rethrow");
        let hand = psm1.find("GetField('Value').SetValue($null, $root)").expect("the handoff line");
        assert!(boot < call && call < catch && catch < floor && floor < rethrow && rethrow < hand, "{psm1}");
        assert_eq!(psm1.matches("[Environment]::Version.Major").count(), 1, "{psm1}");
        assert!(psm1.contains("throw ('Demo' + ' needs PowerShell 7.4 or later: "), "{psm1}");
    }

    /// Every catch clause of a script whose block has only whitespace,
    /// from `catch` to its closing brace.
    fn empty_catches(script: &str) -> Vec<&str> {
        let mut found = Vec::new();
        for (at, _) in script.match_indices("catch") {
            let mut tail = script[at + "catch".len()..].trim_start();
            if let Some(typed) = tail.strip_prefix('[') {
                let Some(close) = typed.find(']') else { continue };
                tail = typed[close + 1..].trim_start();
            }
            let Some(body) = tail.strip_prefix('{') else { continue };
            let inner = body.trim_start();
            if inner.starts_with('}') {
                found.push(&script[at..script.len() - inner.len() + 1]);
            }
        }
        found
    }

    /// The script takes no lock and has no empty catch, so it needs no
    /// param block and no suppression of PSScriptAnalyzer's empty-catch
    /// rule, with the desktop step and a bundled module that warns in place.
    #[test]
    fn the_script_takes_no_lock_and_has_no_empty_catch() {
        let bundled = [Bundled { name: "Calc", warn_on_failure: false }, Bundled { name: "Flynnel", warn_on_failure: true }];
        for psm1 in [bootstrap_psm1(&demo_module(), &[], &[], &[]), bootstrap_psm1(&demo_module(), &[], &["System.Numerics.Vectors.dll"], &bundled)] {
            for absent in ["Mutex", "WaitOne", "SuppressMessageAttribute", "param()"] {
                assert!(!psm1.contains(absent), "{absent} in {psm1}");
            }
            assert_eq!(empty_catches(&psm1), Vec::<&str>::new(), "{psm1}");
        }
        assert_eq!(empty_catches("try { x } catch { }\ntry { y } catch [A.B] {\n}\ntry { z } catch { w }"), ["catch { }", "catch [A.B] {\n}"]);
    }

    /// The bootstrap is staged under the process, the stamp of its source
    /// and the edition, copied under a temporary name and renamed into
    /// place, and loaded from there when its loader is not loaded yet; a
    /// rename that fails counts only when the file is still not there.
    #[test]
    fn the_script_stages_the_bootstrap_whole_under_the_stamp_of_its_source() {
        let psm1 = bootstrap_psm1(&demo_module(), &[], &[], &[]);
        let check = psm1.find("if (-not ('Pwrs.Bootstrap.Loader' -as [type])) {").expect("the loaded check");
        let stage = psm1.find(&format!("'pwrs-load', [string]$PID, 'boot', '{}', $tfm)", bootstrap_stamp())).expect("the stamped folder");
        let copy = psm1.find("[System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.dll'), $partial)").expect("the copy");
        let rename = psm1.find("try { [System.IO.File]::Move($partial, $boot) }").expect("the rename");
        let missing = psm1.find("if (-not [System.IO.File]::Exists($boot)) { throw }").expect("the failed rename's check");
        let load = psm1.find("$null = [System.Reflection.Assembly]::LoadFrom($boot)").expect("the load");
        assert!(check < stage && stage < copy && copy < rename && rename < missing && missing < load, "{psm1}");
        assert_eq!(bootstrap_stamp().len(), 16);
        assert_eq!(bootstrap_stamp(), bootstrap_stamp());
    }

    /// The bootstrap's .pdb, when the build wrote one, is staged before
    /// the bootstrap, under a temporary name renamed into place, so a
    /// staged bootstrap that exists has its symbols beside it.
    #[test]
    fn the_script_stages_the_bootstraps_pdb_before_the_bootstrap() {
        let psm1 = bootstrap_psm1(&demo_module(), &[], &[], &[]);
        let unstaged = psm1.find("if (-not [System.IO.File]::Exists($boot)) {").expect("the staging step");
        let shipped = psm1.find("$pdb = [System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.pdb')").expect("the shipped pdb");
        let guard = psm1.find("if ([System.IO.File]::Exists($pdb) -and -not [System.IO.File]::Exists($bootPdb)) {").expect("the pdb's guard");
        let rename = psm1.find("try { [System.IO.File]::Move($partial, $bootPdb) }").expect("the pdb's rename");
        let missing = psm1.find("if (-not [System.IO.File]::Exists($bootPdb)) { throw }").expect("the failed rename's check");
        let copy = psm1.find("[System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.dll'), $partial)").expect("the bootstrap's copy");
        assert!(unstaged < shipped && shipped < guard && guard < rename && rename < missing && missing < copy, "{psm1}");
    }

    /// A bundled module is imported into the session ahead of the
    /// shell, from its folder beside the script, and one already loaded
    /// in the session is left as it is; a module bundling none has no
    /// such step.
    #[test]
    fn the_script_imports_bundled_modules_before_its_shell() {
        let module = demo_module();
        let none = bootstrap_psm1(&module, &[], &[], &[]);
        assert!(!none.contains("$bundled"), "{none}");

        let psm1 = bootstrap_psm1(&module, &[], &[], &[Bundled { name: "Calc", warn_on_failure: false }, Bundled { name: "Tls", warn_on_failure: false }]);
        let list = psm1.find("foreach ($bundled in @('Calc', 'Tls')) {").expect("the bundled list");
        let held = psm1.find("if (-not (Get-Module -Name $bundled)) {").expect("the check for a loaded module");
        let import = psm1
            .find("Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop")
            .expect("the bundled import");
        let root = psm1.find("$root = $PSScriptRoot").expect("the root line");
        assert!(list < held && held < import && import < root, "{psm1}");
        assert!(!psm1.contains("imports without its bundled module"), "{psm1}");
    }

    /// A module whose entry warns on failure is imported inside a try
    /// whose catch rethrows for every other module and, for it, writes a
    /// warning naming the bundling module, the bundled one and the error.
    #[test]
    fn a_bundled_module_that_warns_is_imported_under_a_catch_that_names_it() {
        let step = bundled_step("Demo", &[Bundled { name: "Calc", warn_on_failure: false }, Bundled { name: "Flynnel", warn_on_failure: true }]);
        let list = step.find("foreach ($bundled in @('Calc', 'Flynnel')) {").expect("the bundled list");
        let attempt = step.find("try {\nImport-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop\n}").expect("the import under try");
        let rethrow = step.find("catch {\nif (@('Flynnel') -notcontains $bundled) { throw }\n").expect("the rethrow for the rest");
        let warning = step
            .find("Microsoft.PowerShell.Utility\\Write-Warning ('Demo' + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)")
            .expect("the warning");
        assert!(list < attempt && attempt < rethrow && rethrow < warning, "{step}");
        assert_eq!(bundled_step("Demo", &[]), "");
    }

    /// The step as generated, run in pwsh, and in Windows PowerShell on
    /// Windows, as the script of a module `Outer` whose bundled `Broken`
    /// cannot import: a plain entry fails Outer's import with Broken's
    /// error, and an entry that warns lets Outer import with one warning
    /// naming Broken and that error.
    #[test]
    fn a_failed_bundled_import_stops_or_warns_as_its_entry_says() {
        if let Err(e) = pwrs_build::pwsh::pshome() {
            eprintln!("skipped: no pwsh to import the modules in ({e})");
            return;
        }
        let root = std::env::temp_dir().join(format!("pwrs-bundled-step-{}", std::process::id()));
        let manifest = |root_module: &str, guid: &str, exports: &str| {
            format!("@{{ RootModule = '{root_module}'; ModuleVersion = '1.0.0'; GUID = '{guid}'; FunctionsToExport = @({exports}) }}\n")
        };
        for (kind, warn) in [("stop", false), ("warn", true)] {
            let outer = root.join(kind).join("Outer");
            std::fs::create_dir_all(outer.join("Broken")).expect("create the module folders");
            std::fs::write(outer.join("Broken").join("Broken.psd1"), manifest("Missing.psm1", "5a1f0c8e-3d7b-4c55-9d0e-2b6f7a8c9d01", ""))
                .expect("write the bundled manifest");
            std::fs::write(outer.join("Outer.psd1"), manifest("Outer.psm1", "6b2e1d9f-4e8c-4d66-8e1f-3c7a8b9d0e12", "'Get-OuterMark'"))
                .expect("write the outer manifest");
            let script = format!(
                "{}function Get-OuterMark {{ 'outer' }}\nExport-ModuleMember -Function Get-OuterMark\n",
                bundled_step("Outer", &[Bundled { name: "Broken", warn_on_failure: warn }])
            );
            std::fs::write(outer.join("Outer.psm1"), script).expect("write the outer script");
        }
        let probe = pwrs_build::pwsh::materialize_script(
            &root,
            "probe.ps1",
            "param([string] $Manifest)\n\
             try {\n\
                 $out = @(Import-Module $Manifest -ErrorAction Stop 3>&1)\n\
                 'imported=' + (Get-OuterMark)\n\
                 foreach ($w in @($out | Where-Object { $_ -is [System.Management.Automation.WarningRecord] })) { 'warning=' + $w.Message }\n\
             } catch {\n\
                 'refused=' + $_.Exception.Message\n\
             }\n",
        )
        .expect("write the probe");
        type RunScript = fn(&std::path::Path, &[String]) -> Result<String, pwrs_build::Error>;
        let mut hosts: Vec<(&str, RunScript)> = vec![("pwsh", pwrs_build::pwsh::run_pwsh_tool)];
        if cfg!(windows) {
            hosts.push(("powershell", pwrs_build::pwsh::run_winps_script));
        }
        for (host, run) in hosts {
            let import = |kind: &str| -> Vec<String> {
                let outer = root.join(kind).join("Outer").join("Outer.psd1");
                run(&probe, &[outer.display().to_string()]).expect("run the probe").lines().map(String::from).collect()
            };
            let stopped = import("stop");
            assert!(stopped.iter().any(|l| l.starts_with("refused=") && l.contains("Broken.psd1")), "{host}: {stopped:?}");
            assert!(!stopped.iter().any(|l| l.starts_with("imported=")), "{host}: {stopped:?}");
            let warned = import("warn");
            assert!(warned.contains(&"imported=outer".to_string()), "{host}: {warned:?}");
            let warnings: Vec<&String> = warned.iter().filter(|l| l.starts_with("warning=")).collect();
            assert_eq!(warnings.len(), 1, "{host}: {warned:?}");
            assert!(warnings[0].starts_with("warning=Outer imports without its bundled module Broken, whose import failed: "), "{host}: {warned:?}");
            assert!(warnings[0].contains("Broken.psd1"), "{host}: {warned:?}");
        }
        std::fs::remove_dir_all(&root).expect("remove the scratch folder");
    }

    /// A module whose one proxy class has `roles`, JSON members naming
    /// its methods, and the methods `methods`, a JSON array.
    fn class_with_roles(roles: &str, methods: &str) -> Module {
        let json = format!(
            r#"{{"abi": 1, "name": "Demo", "cmdlets": [], "classes": [{{
                "id": 0, "name": "Demo.Row", "rust": "Row", "mode": "proxy", {roles}
                "description": "", "fields": [], "methods": {methods}
            }}]}}"#
        );
        serde_json::from_str(&json).expect("a descriptor")
    }

    /// A `#[psmethods]` method as the descriptor describes it.
    fn method_json(index: u32, name: &str, params: &str, ret: &str, mutable: bool) -> String {
        format!(r#"{{"name": "{name}", "rust": "{name}", "index": {index}, "help": "", "params": [{params}], "ret": {ret}, "static": false, "mutable": {mutable}, "constructor": false}}"#)
    }

    const INDEX: &str = r#"{"name": "index", "rust": "index", "index": 0, "clr": "int", "slot": "i32", "optional": false}"#;
    const LONG: &str = r#"{"clr": "long", "optional": false}"#;

    #[test]
    fn a_list_class_implements_its_interfaces_through_the_methods_its_roles_name() {
        let value = r#"{"name": "value", "rust": "value", "index": 1, "clr": "long", "slot": "i64", "optional": false}"#;
        let methods = format!(
            "[{}, {}, {}]",
            method_json(0, "Len", "", r#"{"clr": "int", "optional": false}"#, false),
            method_json(1, "At", INDEX, LONG, false),
            method_json(2, "Put", &format!("{INDEX}, {value}"), "null", true)
        );
        let module = class_with_roles(r#""count": "Len", "item": "At", "set_item": "Put","#, &methods);
        validate(&module).expect("a list whose members do not collide");
        let shell = unplanned(&module, "demo");
        let declared = "public sealed class Row : Pwrs.ProxyBase, global::System.Collections.Generic.IReadOnlyList<long>, global::System.Collections.Generic.IList<long>";
        assert!(shell.contains(declared), "{shell}");
        assert!(shell.contains("public int Count => Len();"), "{shell}");
        assert!(shell.contains("get => At(index);") && shell.contains("set => Put(index, value);"), "{shell}");
        assert!(shell.contains("for (int i = 0; i < count; i++) yield return this[i];"), "{shell}");
        assert!(shell.contains("ICollection<long>.IsReadOnly => true;"), "{shell}");
        assert!(shell.contains("ICollection<long>.Add(long item) => throw new NotSupportedException("), "{shell}");
    }

    #[test]
    fn a_list_refuses_a_method_whose_name_is_one_its_members_take() {
        let methods = format!("[{}, {}]", method_json(0, "Count", "", r#"{"clr": "int", "optional": false}"#, false), method_json(1, "At", INDEX, LONG, false));
        let module = class_with_roles(r#""count": "Count", "item": "At","#, &methods);
        let refused = validate(&module).expect_err("a method named Count beside the list's Count");
        assert!(refused.to_string().contains("which declares Count through count and item, and its method Count is named Count"), "{refused}");
    }

    /// Two classes the module lists under one PowerShell name, compared
    /// without regard to case, stop the build naming both Rust types by
    /// path; distinct names pass.
    #[test]
    fn two_classes_under_one_name_are_refused_naming_both_types() {
        let class = |id: u32, name: &str, rust: &str, path: &str| {
            format!(r#"{{"id": {id}, "name": "{name}", "rust": "{rust}", "path": "{path}", "mode": "copied", "description": "", "fields": []}}"#)
        };
        let module = |classes: &[String]| -> Module {
            serde_json::from_str(&format!(r#"{{"abi": 1, "name": "Trex", "cmdlets": [], "classes": [{}]}}"#, classes.join(", "))).expect("a descriptor")
        };
        let refused = validate(&module(&[
            class(0, "Trex.Segment", "Segment", "trex::recurrence::Segment"),
            class(1, "Trex.Tiling", "Tiling", "trex::grammar::Tiling"),
            class(2, "trex.segment", "Segment", "trex::grammar::Segment"),
        ]))
        .expect_err("two classes named Trex.Segment");
        assert!(
            refused.to_string().contains("trex::recurrence::Segment and trex::grammar::Segment are both declared as trex.segment"),
            "{refused}"
        );
        validate(&module(&[class(0, "Trex.Segment", "Segment", "trex::recurrence::Segment"), class(1, "Trex.Tiling", "Tiling", "trex::grammar::Tiling")]))
            .expect("two classes under two names");
    }

    /// Two copied classes whose Rust types share a name, from different
    /// modules, each get a field block of their own in the shell.
    #[test]
    fn two_copied_classes_of_one_rust_name_get_a_block_each() {
        let field = r#"{"name": "N", "rust": "n", "index": 0, "clr": "long", "slot": "i64", "optional": false, "help": ""}"#;
        let class = |id: u32, name: &str| {
            format!(r#"{{"id": {id}, "name": "{name}", "rust": "Segment", "mode": "copied", "description": "", "fields": [{field}]}}"#)
        };
        let json = format!(r#"{{"abi": 1, "name": "Trex", "cmdlets": [], "classes": [{}, {}]}}"#, class(0, "Trex.Segment"), class(1, "Trex.Tiling"));
        let module: Module = serde_json::from_str(&json).expect("a descriptor");
        validate(&module).expect("two names");
        let shell = unplanned(&module, "trex");
        assert_eq!(shell.matches("internal struct SegmentFields0").count(), 1, "{shell}");
        assert_eq!(shell.matches("internal struct SegmentFields1").count(), 1, "{shell}");
        assert!(shell.contains("var b = (SegmentFields1*)fields;"), "{shell}");
    }

    /// Past 64 arguments a method's block adds a bound word, named with its
    /// number, and each argument sets its bit in its own word; a method of
    /// 64 has the one word it always had.
    #[test]
    fn a_method_past_64_arguments_has_a_bound_word_per_64() {
        let shell_of = |n: u32| {
            let args: Vec<String> = (0..n)
                .map(|i| format!(r#"{{"name": "a{i}", "rust": "a{i}", "index": {i}, "clr": "long", "slot": "i64", "optional": true}}"#))
                .collect();
            let methods = format!("[{}]", method_json(0, "Wide", &args.join(", "), LONG, false));
            unplanned(&class_with_roles("", &methods), "demo")
        };
        let shell = shell_of(64);
        assert!(!shell.contains("public ulong Bound1;"), "{shell}");
        assert!(shell.contains("if (a63.HasValue) { __args.P63 = a63.Value; __args.Bound |= 1UL << 63; }"), "{shell}");

        let shell = shell_of(130);
        let block = "        private struct WideArgs\n        {\n            public ulong Bound;\n            public ulong Bound1;\n            public ulong Bound2;\n            public long P0;\n";
        assert!(shell.contains(block), "{shell}");
        for (i, word, bit) in [(0, "", 0), (63, "", 63), (64, "1", 0), (65, "1", 1), (127, "1", 63), (128, "2", 0), (129, "2", 1)] {
            let set = format!("if (a{i}.HasValue) {{ __args.P{i} = a{i}.Value; __args.Bound{word} |= 1UL << {bit}; }}");
            assert!(shell.contains(&set), "no `{set}`: {shell}");
        }
    }

    /// Past 64 fields a copied class's block adds a mask word, named with
    /// its number, and the factory reads each optional field's bit from its
    /// own word; a class of 64 has the one word it always had.
    #[test]
    fn a_copied_class_past_64_fields_has_a_mask_word_per_64() {
        let shell_of = |n: u32| {
            let fields: Vec<String> = (0..n)
                .map(|i| format!(r#"{{"name": "A{i}", "rust": "a{i}", "index": {i}, "clr": "long", "slot": "i64", "optional": true, "help": ""}}"#))
                .collect();
            let json = format!(
                r#"{{"abi": 1, "name": "Demo", "cmdlets": [], "classes": [{{"id": 0, "name": "Demo.Record", "rust": "Record", "mode": "copied", "description": "", "fields": [{}]}}]}}"#,
                fields.join(", ")
            );
            let module: Module = serde_json::from_str(&json).expect("a descriptor");
            unplanned(&module, "demo")
        };
        let shell = shell_of(64);
        assert!(!shell.contains("public ulong Mask1;"), "{shell}");
        assert!(shell.contains("A63 = (b->Mask & (1UL << 63)) != 0 ? b->F63 : (long?)null,"), "{shell}");

        let shell = shell_of(130);
        assert!(shell.contains("        public long F129;\n        public ulong Mask;\n        public ulong Mask1;\n        public ulong Mask2;\n    }\n"), "{shell}");
        for (i, word, bit) in [(0, "", 0), (63, "", 63), (64, "1", 0), (65, "1", 1), (127, "1", 63), (128, "2", 0), (129, "2", 1)] {
            let read = format!("A{i} = (b->Mask{word} & (1UL << {bit})) != 0 ? b->F{i} : (long?)null,");
            assert!(shell.contains(&read), "no `{read}`: {shell}");
        }
    }

    /// Cmdlets and providers whose Rust types share a name, from
    /// different modules, each get a class of their own, the id after
    /// the name; a Rust name no other shares gets the plain class name.
    #[test]
    fn cmdlets_and_providers_of_one_rust_name_get_a_class_each() {
        let cmdlet = |id: u32, noun: &str, rust: &str| {
            format!(
                r#"{{"id": {id}, "verb": "Get", "noun": "{noun}", "name": "Get-{noun}", "rust": "{rust}", "should_process": false, "confirm_impact": null, "default_set": null, "aliases": [], "output_types": [], "synopsis": "", "description": "", "params": []}}"#
            )
        };
        let provider = |id: u32, name: &str| format!(r#"{{"id": {id}, "name": "{name}", "rust": "Store", "description": ""}}"#);
        let json = format!(
            r#"{{"abi": 1, "name": "Trex", "cmdlets": [{}, {}, {}], "providers": [{}, {}]}}"#,
            cmdlet(0, "Segment", "Get"),
            cmdlet(1, "Tiling", "Get"),
            cmdlet(2, "Orbit", "GetOrbit"),
            provider(0, "TrexA"),
            provider(1, "TrexB")
        );
        let module: Module = serde_json::from_str(&json).expect("a descriptor");
        let shell = unplanned(&module, "trex");
        assert!(shell.contains("public sealed class GetCommand0 : Pwrs.RustCmdlet"), "{shell}");
        assert!(shell.contains("public sealed class GetCommand1 : Pwrs.RustCmdlet"), "{shell}");
        assert!(shell.contains("public sealed class GetOrbitCommand : Pwrs.RustCmdlet"), "{shell}");
        assert!(!shell.contains("public sealed class GetCommand :"), "{shell}");
        assert!(shell.contains("public sealed class StoreProvider0 : Pwrs.ProviderBase"), "{shell}");
        assert!(shell.contains("public sealed class StoreProvider1 : Pwrs.ProviderBase"), "{shell}");
    }

    /// A provider's declared capabilities join ShouldProcess in the
    /// attribute's constructor argument; one declaring none has
    /// ShouldProcess alone.
    #[test]
    fn a_providers_capabilities_are_the_attributes_constructor_argument() {
        let json = r#"{"abi": 1, "name": "Demo", "cmdlets": [], "providers": [
            {"id": 0, "name": "Filtered", "rust": "Filtered", "capabilities": ["Filter", "Include"], "description": ""},
            {"id": 1, "name": "Plain", "rust": "Plain", "description": ""}]}"#;
        let module: Module = serde_json::from_str(json).expect("a descriptor");
        let shell = unplanned(&module, "demo");
        assert!(
            shell.contains(r#"[CmdletProvider(@"Filtered", ProviderCapabilities.ShouldProcess | ProviderCapabilities.Filter | ProviderCapabilities.Include)]"#),
            "{shell}"
        );
        assert!(shell.contains(r#"[CmdletProvider(@"Plain", ProviderCapabilities.ShouldProcess)]"#), "{shell}");
        assert!(!shell.contains("ProviderCapabilities ="), "{shell}");
    }

    #[test]
    fn a_stream_class_enumerates_until_its_next_method_returns_null() {
        let methods = format!("[{}]", method_json(0, "Take", "", r#"{"clr": "long", "optional": true}"#, true));
        let shell = unplanned(&class_with_roles(r#""next": "Take","#, &methods), "demo");
        assert!(shell.contains("public sealed class Row : Pwrs.ProxyBase, global::System.Collections.Generic.IEnumerable<long>"), "{shell}");
        assert!(shell.contains("var next = Take();") && shell.contains("yield return next.Value;"), "{shell}");
    }

    #[test]
    fn an_ordered_and_equatable_class_passes_the_other_object_through_its_peer_slot() {
        let other = r#"{"name": "other", "rust": "other", "index": 0, "clr": "Demo.Row", "slot": "peer", "optional": false}"#;
        let methods = format!(
            "[{}, {}, {}]",
            method_json(0, "Order", other, r#"{"clr": "int", "optional": false}"#, false),
            method_json(1, "Same", other, r#"{"clr": "bool", "optional": false}"#, false),
            method_json(2, "Hash", "", LONG, false)
        );
        let shell = unplanned(&class_with_roles(r#""compare": "Order", "equals": "Same", "hash": "Hash","#, &methods), "demo");
        assert!(shell.contains("public sealed class Row : Pwrs.ProxyBase, global::System.IComparable, global::System.IComparable<Row>, global::System.IEquatable<Row>"), "{shell}");
        assert!(shell.contains("if (other is null) throw new ArgumentNullException(nameof(other));"), "{shell}");
        assert!(shell.contains("base.PwrsCallWith(0, &__args, false, other, &__args.P0)"), "{shell}");
        assert!(shell.contains("public override int GetHashCode()") && shell.contains("long hash = Hash();"), "{shell}");
        assert!(shell.contains("public override bool Equals(object? obj)"), "{shell}");
    }

    /// Each operator key declares a C# operator that calls the method it
    /// names on the left operand, and an operator whose method returns
    /// nothing is refused.
    #[test]
    fn an_operator_calls_its_method_and_one_returning_nothing_is_refused() {
        let other = r#"{"name": "other", "rust": "other", "index": 0, "clr": "Demo.Row", "slot": "peer", "optional": false}"#;
        let amount = r#"{"name": "amount", "rust": "amount", "index": 0, "clr": "long", "slot": "i64", "optional": false}"#;
        let row = r#"{"clr": "Demo.Row", "optional": false}"#;
        let methods = format!("[{}, {}]", method_json(0, "Plus", other, row, false), method_json(1, "Left", amount, LONG, false));
        let module = class_with_roles(r#""add": "Plus", "rem": "Left","#, &methods);
        validate(&module).expect("two operators");
        let shell = unplanned(&module, "demo");
        for line in [
            "public static global::Demo.Row? operator +(Row left, global::Demo.Row? right) => left.Plus(right);",
            "public static long operator %(Row left, long right) => left.Left(right);",
        ] {
            assert!(shell.contains(line), "{line} is missing from:\n{shell}");
        }
        let void = format!("[{}]", method_json(0, "Plus", other, "null", false));
        let refused = validate(&class_with_roles(r#""add": "Plus","#, &void)).expect_err("an operator returning nothing");
        assert!(refused.to_string().contains("class Demo.Row's add names Plus, which must return a value"), "{refused}");
    }

    #[test]
    fn a_method_taking_a_task_returns_it_and_takes_the_callers_cancellation_token() {
        let ms = r#"{"name": "ms", "rust": "ms", "index": 0, "clr": "long", "slot": "i64", "optional": false}"#;
        let task = r#"{"name": "task", "rust": "task", "index": 1, "clr": "long", "slot": "task", "optional": false}"#;
        let unit = r#"{"name": "task", "rust": "task", "index": 0, "clr": "void", "slot": "task", "optional": false}"#;
        let methods = format!("[{}, {}]", method_json(0, "WaitAsync", &format!("{ms}, {task}"), "null", true), method_json(1, "PingAsync", unit, "null", false));
        let module = class_with_roles("", &methods);
        validate(&module).expect("task methods");
        let shell = unplanned(&module, "demo");
        assert!(
            shell.contains("public unsafe global::System.Threading.Tasks.Task<long> WaitAsync(long ms, global::System.Threading.CancellationToken cancellationToken = default)"),
            "{shell}"
        );
        assert!(shell.contains("var __task = new global::Pwrs.TaskSource<long>(cancellationToken);"), "{shell}");
        assert!(shell.contains("__args.P1 = new global::Pwrs.PsTaskSlot { Source = GCHandle.ToIntPtr(__h1), Cancelled = __task.CancelFlag };"), "{shell}");
        assert!(shell.contains("__task.Discard(__failed);") && shell.contains("return __task.Task;"), "{shell}");
        assert!(shell.contains("public unsafe global::System.Threading.Tasks.Task PingAsync(global::System.Threading.CancellationToken cancellationToken = default)"), "{shell}");
        assert!(shell.contains("new global::Pwrs.TaskSource<object?>(cancellationToken)"), "{shell}");
    }

    #[test]
    fn a_method_argument_may_have_any_name_the_body_uses_for_its_own() {
        let names = ["b", "r", "s0", "h0", "c0", "t0"]
            .iter()
            .enumerate()
            .map(|(i, n)| format!(r#"{{"name": "{n}", "rust": "{n}", "index": {i}, "clr": "long", "slot": "i64", "optional": false}}"#))
            .collect::<Vec<_>>()
            .join(", ");
        let shell = unplanned(&class_with_roles("", &format!("[{}]", method_json(0, "Mix", &names, LONG, false))), "demo");
        assert!(shell.contains("public unsafe long Mix(long b, long r, long s0, long h0, long c0, long t0)"), "{shell}");
        assert!(shell.contains("MixArgs __args = default;") && shell.contains("__args.P0 = b;") && shell.contains("object? __result;"), "{shell}");
    }

    #[test]
    fn a_task_method_refuses_an_argument_named_as_its_token() {
        let token = r#"{"name": "cancellationToken", "rust": "cancellation_token", "index": 0, "clr": "long", "slot": "i64", "optional": false}"#;
        let task = r#"{"name": "task", "rust": "task", "index": 1, "clr": "long", "slot": "task", "optional": false}"#;
        let module = class_with_roles("", &format!("[{}]", method_json(0, "Wait", &format!("{token}, {task}"), "null", false)));
        let refused = validate(&module).expect_err("a second cancellationToken");
        assert!(refused.to_string().contains("its argument cancellation_token has that name too"), "{refused}");
    }

    /// An argument named after a word C# reserves is written with an `@`,
    /// in the signature and in the body, so the shell compiles; the `@` is
    /// not part of the name PowerShell reads.
    #[test]
    fn an_argument_named_after_a_csharp_keyword_is_written_with_an_at() {
        let operator = r#"{"name": "operator", "rust": "operator", "index": 0, "clr": "string", "slot": "str16", "optional": false}"#;
        let by = r#"{"name": "by", "rust": "by", "index": 1, "clr": "long", "slot": "i64", "optional": false}"#;
        let module = class_with_roles("", &format!("[{}]", method_json(0, "Apply", &format!("{operator}, {by}"), LONG, true)));
        validate(&module).expect("a method with an argument named operator");
        let shell = unplanned(&module, "demo");
        assert!(shell.contains(" @operator, long by)"), "{shell}");
        assert!(shell.contains("string __s0 = @operator ?? string.Empty;"), "{shell}");
        assert!(!shell.contains(" operator,"), "{shell}");
        assert_eq!(cs_name("type"), "type");
        assert_eq!(cs_name("checked"), "@checked");
    }

    /// An enum value named after a word C# reserves, as a variant written
    /// `r#in` is, is declared with an `@`, so the shell compiles.
    #[test]
    fn an_enum_value_named_after_a_csharp_keyword_is_written_with_an_at() {
        let json = r#"{"abi": 1, "name": "Demo", "cmdlets": [], "classes": [{
            "id": 0, "name": "Demo.Way", "rust": "Way", "mode": "enum", "description": "", "fields": [],
            "variants": [{"name": "in", "value": 0, "help": ""}, {"name": "Both", "value": 1, "help": ""}]
        }]}"#;
        let module: Module = serde_json::from_str(json).expect("a descriptor");
        validate(&module).expect("an enum with a value named in");
        let shell = unplanned(&module, "demo");
        assert!(shell.contains("        @in = 0,\n"), "{shell}");
        assert!(shell.contains("        Both = 1,\n"), "{shell}");
    }

    fn demo_module() -> Module {
        Module {
            abi: 1,
            name: "Demo".to_string(),
            cmdlets: Vec::new(),
            classes: Vec::new(),
            completers: Vec::new(),
            transforms: Vec::new(),
            providers: Vec::new(),
            on_import: false,
            on_remove: false,
            on_load: false,
            on_unload: false,
        }
    }

    /// A module declaring any hook has a lifecycle class implementing
    /// only the import interface, and its script finds that class before
    /// the shell import, writes the errors the import returned without
    /// failing, and runs the removal from its removal block; it does not
    /// call ReloadNative, since the import hook reloads. A module without
    /// hooks has neither, and calls ReloadNative after the import.
    #[test]
    fn a_module_with_hooks_removes_from_its_script_and_one_without_reloads_from_it() {
        for (on_import, on_remove, on_load, on_unload) in [(true, false, false, false), (false, true, false, false), (false, false, true, false), (false, false, false, true)] {
            let module = Module { on_import, on_remove, on_load, on_unload, ..demo_module() };
            let shell = unplanned(&module, "demo");
            assert!(shell.contains("public sealed class PwrsModuleLifecycle : IModuleAssemblyInitializer\n"), "{shell}");
            assert!(shell.contains("public void OnImport() => PwrsModule.Native.Imported();"), "{shell}");
            assert!(shell.contains("public static ErrorRecord[] TakeImportErrors()\n"), "{shell}");
            assert!(shell.contains("typeof(Pwrs.NativeModule).GetMethod(\"TakeImportErrors\", System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static);"), "{shell}");
            assert!(shell.contains("public static ErrorRecord[] Removed() => PwrsModule.Native.Removed();"), "{shell}");
            assert!(!shell.contains("IModuleAssemblyCleanup"), "{shell}");

            let psm1 = bootstrap_psm1(&module, &[], &[], &[]);
            let find = psm1.find("$lifecycle = $shell.GetType('Pwrs.Modules.Demo.PwrsModuleLifecycle', $true)").expect("the lifecycle class");
            let import = psm1.find("Import-Module -Assembly $shell -Force").expect("the import line");
            let failed = psm1
                .find("catch {\nforeach ($e in $lifecycle::TakeImportErrors()) { Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue }\n")
                .expect("the errors before a rethrow");
            let written = psm1
                .find("}\nforeach ($e in $lifecycle::TakeImportErrors()) { Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue }\n")
                .expect("the errors after the import");
            let removal = psm1
                .find("$ExecutionContext.SessionState.Module.OnRemove = { foreach ($e in $lifecycle::Removed()) { Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue } }")
                .expect("the removal block");
            let export = psm1.find("Export-ModuleMember").expect("the export");
            assert!(find < import && import < failed && failed < written && written < removal && removal < export, "{psm1}");
            assert!(!psm1.contains("ReloadNative"), "{psm1}");
            assert!(psm1.contains(&format!("catch {{\nforeach ($e in $lifecycle::TakeImportErrors()) {{ Microsoft.PowerShell.Utility\\Write-Error -ErrorRecord $e -ErrorAction Continue }}\n{RETHROW}}}\n")), "{psm1}");
            assert_eq!(empty_catches(&psm1), Vec::<&str>::new(), "{psm1}");
        }

        let none = demo_module();
        let shell = unplanned(&none, "demo");
        assert!(!shell.contains("PwrsModuleLifecycle"), "{shell}");
        let psm1 = bootstrap_psm1(&none, &[], &[], &[]);
        assert!(!psm1.contains("$lifecycle"), "{psm1}");
        assert!(!psm1.contains("OnRemove"), "{psm1}");
        let import = psm1.find("Import-Module -Assembly $shell -Force").expect("the import line");
        let reload = psm1.find("if ($null -ne $reload) { $null = $reload::ReloadNative() }").expect("the reload");
        assert!(import < reload, "{psm1}");
        assert!(psm1.contains(&format!("catch {{\n{RETHROW}}}\n")), "{psm1}");
    }

    /// Every property `New-ModuleManifest` accepts, set at once, so a
    /// key that stops being written fails here rather than in a
    /// gallery listing.
    #[test]
    fn every_manifest_property_reaches_the_psd1() {
        let one = |s: &str| vec![s.to_string()];
        let editions = vec!["Desktop".to_string()];
        let required_modules = one("Storage");
        let required_assemblies = one("System.Xml.dll");
        let scripts = one("init.ps1");
        let types = one("Demo.Types.ps1xml");
        let nested = one("Extra.psm1");
        let dsc = one("DemoResource");
        let modules = one("Demo");
        let files = one("readme.md");
        let deps = one("Pester");
        let keywords = vec!["demo".to_string()];
        let pkg = Package {
            version: "2.3.4",
            author: "An Author",
            description: Some("A described module."),
            repository: Some("https://example.invalid/demo"),
            tags: &keywords,
            license_uri: Some("https://example.invalid/demo/license"),
            release_notes: Some("Notes."),
            icon_uri: Some("https://example.invalid/demo/icon.png"),
            company: Some("A Company"),
            copyright: Some("(c) An Author"),
            prerelease: Some("beta1"),
            require_license_acceptance: true,
            external_module_dependencies: &deps,
            powershell_version: Some("7.2"),
            compatible_ps_editions: &editions,
            powershell_host_name: Some("ConsoleHost"),
            powershell_host_version: Some("5.1"),
            dotnet_framework_version: Some("4.7.2"),
            clr_version: Some("4.0"),
            processor_architecture: Some("Amd64"),
            help_info_uri: Some("https://example.invalid/demo/help"),
            default_command_prefix: Some("Dm"),
            required_modules: &required_modules,
            required_assemblies: &required_assemblies,
            scripts_to_process: &scripts,
            types_to_process: &types,
            nested_modules: &nested,
            dsc_resources_to_export: &dsc,
            module_list: &modules,
            file_list: &files,
        };
        let psd1 = manifest(&demo_module(), &pkg, Some("Demo.Format.ps1xml"), &[]);
        for expected in [
            "RootModule = 'Demo.psm1'",
            "ModuleVersion = '2.3.4'",
            "CompatiblePSEditions = @('Desktop')",
            "Author = 'An Author'",
            "CompanyName = 'A Company'",
            "Copyright = '(c) An Author'",
            "Description = 'A described module.'",
            "PowerShellVersion = '7.2'",
            "PowerShellHostName = 'ConsoleHost'",
            "PowerShellHostVersion = '5.1'",
            "DotNetFrameworkVersion = '4.7.2'",
            "ClrVersion = '4.0'",
            "ProcessorArchitecture = 'Amd64'",
            "RequiredModules = @('Storage')",
            "RequiredAssemblies = @('System.Xml.dll')",
            "ScriptsToProcess = @('init.ps1')",
            "TypesToProcess = @('Demo.Types.ps1xml')",
            "FormatsToProcess = @('Demo.Format.ps1xml')",
            "NestedModules = @('Extra.psm1')",
            "FunctionsToExport = @()",
            "CmdletsToExport = @()",
            "VariablesToExport = @()",
            "AliasesToExport = @()",
            "DscResourcesToExport = @('DemoResource')",
            "ModuleList = @('Demo')",
            "FileList = @('readme.md')",
            "HelpInfoURI = 'https://example.invalid/demo/help'",
            "DefaultCommandPrefix = 'Dm'",
            "Tags = @('pwrs', 'demo')",
            "LicenseUri = 'https://example.invalid/demo/license'",
            "ProjectUri = 'https://example.invalid/demo'",
            "IconUri = 'https://example.invalid/demo/icon.png'",
            "ReleaseNotes = 'Notes.'",
            "Prerelease = 'beta1'",
            "RequireLicenseAcceptance = $true",
            "ExternalModuleDependencies = @('Pester')",
            "GUID = ",
        ] {
            assert!(psd1.contains(expected), "missing {expected} from:\n{psd1}");
        }
    }

    /// A crate that sets nothing gets the defaults the two generated
    /// shells need and no key it has no value for, because a gallery
    /// reads an empty key as set.
    #[test]
    fn an_unset_property_writes_no_key() {
        let pkg = Package { version: "0.1.0", author: "a", ..Package::default() };
        let psd1 = manifest(&demo_module(), &pkg, None, &[]);
        assert!(psd1.contains("CompatiblePSEditions = @('Desktop', 'Core')"), "{psd1}");
        assert!(psd1.contains("PowerShellVersion = '5.1'"), "{psd1}");
        for absent in [
            "CompanyName",
            "Copyright",
            "PowerShellHostName",
            "PowerShellHostVersion",
            "DotNetFrameworkVersion",
            "ClrVersion",
            "ProcessorArchitecture",
            "RequiredModules",
            "RequiredAssemblies",
            "ScriptsToProcess",
            "TypesToProcess",
            "FormatsToProcess",
            "NestedModules",
            "DscResourcesToExport",
            "ModuleList",
            "FileList",
            "HelpInfoURI",
            "DefaultCommandPrefix",
            "LicenseUri",
            "ProjectUri",
            "IconUri",
            "ReleaseNotes",
            "Prerelease",
            "RequireLicenseAcceptance",
            "ExternalModuleDependencies",
        ] {
            assert!(!psd1.contains(absent), "{absent} written for a crate that set none:\n{psd1}");
        }
    }

    /// PowerShell itself parses a manifest with every key, and
    /// reads back the values written. String assertions alone cannot
    /// say the file is valid data; this is the only check that can.
    #[test]
    fn powershell_parses_a_manifest_with_every_key() {
        let one = |s: &str| vec![s.to_string()];
        let editions = vec!["Desktop".to_string(), "Core".to_string()];
        let required_modules = one("Storage");
        let deps = one("Pester");
        let files = one("readme.md");
        let keywords = vec!["demo".to_string()];
        let pkg = Package {
            version: "2.3.4",
            author: "O'Hara",
            description: Some("A described module."),
            repository: Some("https://example.invalid/demo"),
            tags: &keywords,
            license_uri: Some("https://example.invalid/demo/license"),
            release_notes: Some("Notes."),
            icon_uri: Some("https://example.invalid/demo/icon.png"),
            company: Some("A Company"),
            copyright: Some("(c) O'Hara"),
            prerelease: Some("beta1"),
            require_license_acceptance: true,
            external_module_dependencies: &deps,
            powershell_version: Some("7.2"),
            compatible_ps_editions: &editions,
            powershell_host_name: Some("ConsoleHost"),
            powershell_host_version: Some("5.1"),
            dotnet_framework_version: Some("4.7.2"),
            clr_version: Some("4.0"),
            processor_architecture: Some("Amd64"),
            help_info_uri: Some("https://example.invalid/demo/help"),
            default_command_prefix: Some("Dm"),
            required_modules: &required_modules,
            required_assemblies: &[],
            scripts_to_process: &[],
            types_to_process: &[],
            nested_modules: &[],
            dsc_resources_to_export: &[],
            module_list: &[],
            file_list: &files,
        };
        let psd1 = manifest(&demo_module(), &pkg, None, &[]);

        let dir = std::env::temp_dir().join(format!("pwrs_manifest_parse_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("Demo.psd1");
        std::fs::write(&path, &psd1).expect("write manifest");

        // Every value is printed on its own line so a mismatch names
        // its key.
        let script = format!(
            "$m = Import-PowerShellDataFile -Path '{}'; \
             'ModuleVersion=' + $m.ModuleVersion; 'Author=' + $m.Author; \
             'CompanyName=' + $m.CompanyName; 'Copyright=' + $m.Copyright; \
             'PowerShellVersion=' + $m.PowerShellVersion; \
             'ProcessorArchitecture=' + $m.ProcessorArchitecture; \
             'DefaultCommandPrefix=' + $m.DefaultCommandPrefix; \
             'HelpInfoURI=' + $m.HelpInfoURI; \
             'RequiredModules=' + ($m.RequiredModules -join ','); \
             'FileList=' + ($m.FileList -join ','); \
             'Editions=' + ($m.CompatiblePSEditions -join ','); \
             'Tags=' + ($m.PrivateData.PSData.Tags -join ','); \
             'Prerelease=' + $m.PrivateData.PSData.Prerelease; \
             'RequireLicenseAcceptance=' + $m.PrivateData.PSData.RequireLicenseAcceptance; \
             'ExternalModuleDependencies=' + ($m.PrivateData.PSData.ExternalModuleDependencies -join ',')",
            path.display()
        );
        let cache = pwrs_build::pwsh::OwnCache::new().expect("make pwsh's cache folder");
        let mut cmd = std::process::Command::new(pwrs_build::pwsh::pwsh_exe());
        cache.give(&mut cmd);
        let out = cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]).output().expect("run pwsh");
        let stdout = String::from_utf8_lossy(&out.stdout).replace('\r', "");
        assert!(out.status.success(), "pwsh rejected the manifest: {}\n{psd1}", String::from_utf8_lossy(&out.stderr));
        for expected in [
            "ModuleVersion=2.3.4",
            "Author=O'Hara",
            "CompanyName=A Company",
            "Copyright=(c) O'Hara",
            "PowerShellVersion=7.2",
            "ProcessorArchitecture=Amd64",
            "DefaultCommandPrefix=Dm",
            "HelpInfoURI=https://example.invalid/demo/help",
            "RequiredModules=Storage",
            "FileList=readme.md",
            "Editions=Desktop,Core",
            "Tags=pwrs,demo",
            "Prerelease=beta1",
            "RequireLicenseAcceptance=True",
            "ExternalModuleDependencies=Pester",
        ] {
            assert!(stdout.lines().any(|l| l == expected), "PowerShell read back no {expected}; it read:\n{stdout}\nfrom:\n{psd1}");
        }
        // Removed only once the assertions pass, so a failure leaves
        // the file that produced it.
        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    /// A value with a quote in it is escaped rather than closing the
    /// literal, for every shape of key the manifest writes.
    #[test]
    fn a_quote_in_a_value_is_escaped() {
        let awkward = vec!["it's".to_string()];
        let pkg = Package {
            version: "0.1.0",
            author: "O'Hara",
            copyright: Some("(c) O'Hara"),
            required_modules: &awkward,
            ..Package::default()
        };
        let psd1 = manifest(&demo_module(), &pkg, None, &[]);
        assert!(psd1.contains("Author = 'O''Hara'"), "{psd1}");
        assert!(psd1.contains("Copyright = '(c) O''Hara'"), "{psd1}");
        assert!(psd1.contains("RequiredModules = @('it''s')"), "{psd1}");
    }
}

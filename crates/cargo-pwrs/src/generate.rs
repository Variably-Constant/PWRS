//! C# shell, manifest, and bootstrap script generation from the
//! descriptor.

use crate::descriptor::{Class, Cmdlet, Completer, Field, Method, MethodParam, MethodRet, Module, Param, Transform};
use crate::Error;

/// Rejects a descriptor the shell cannot express.
pub fn validate(m: &Module) -> Result<(), Error> {
    for (i, c) in m.classes.iter().enumerate() {
        if let Some(first) = m.classes[..i].iter().find(|other| other.name.eq_ignore_ascii_case(&c.name)) {
            return Err(Error::msg(format!(
                "{} and {} are both declared as {}; PowerShell knows one type by one name, compared without regard to case, so give one of them another name",
                rust_type(first),
                rust_type(c),
                c.name
            )));
        }
    }
    for c in &m.classes {
        match c.mode.as_str() {
            "proxy" => {}
            // A copied object carries its fields and no Rust value, so
            // only what runs against no value can be declared on it: its
            // constructors and its other statics.
            "copied" => {
                if let Some(method) = c.methods.iter().find(|method| !method.is_static) {
                    return Err(Error::msg(format!(
                        "class {} is copied mode, so {} cannot take &self: no Rust value sits behind a copied object to run it against. Declare it without a receiver, or make the class mode = proxy",
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
    Ok(())
}

/// The class's Rust type by its path, or by its name where the
/// descriptor carries no path.
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
/// of the same PowerShell name: a C# class holds one member of a name.
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

/// Property types that may be null when unbound: everything but the
/// CLR primitives and the descriptor entries marked `value_type`.
fn is_reference_type(clr: &str, value_type: bool) -> bool {
    const PRIMITIVES: &[&str] = &["bool", "sbyte", "short", "int", "long", "byte", "ushort", "uint", "ulong", "float", "double", "char", "SwitchParameter"];
    !value_type && !PRIMITIVES.contains(&clr)
}

/// A descriptor CLR name as a C# type expression. A dotted name is
/// rooted with `global::` so it resolves the same way inside every
/// generated namespace.
fn cs_type(clr: &str) -> String {
    if clr.contains('.') {
        format!("global::{clr}")
    } else {
        clr.to_string()
    }
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
        // The two names a literal-path parameter answers to. A name the
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
/// module shares the Rust name, since a namespace holds one type of a
/// name.
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
    // supplied, so the setter is where boundness is recorded: no
    // dictionary is consulted on the call path. A cmdlet with no
    // parameters has nothing to record and declares neither word.
    let has_params = !c.params.is_empty();
    if has_params {
        s.push_str("        private ulong _bound;\n        private ulong _dirty;\n\n");
    }
    for p in &c.params {
        let clr = if is_reference_type(&p.clr, p.value_type) { format!("{}?", cs_type(&p.clr)) } else { cs_type(&p.clr) };
        s.push_str(&format!("        private {clr} _p{};\n", p.index));
        s.push_str(&param_attributes(p));
        if let Some(cmp) = completers.iter().find(|cm| cm.cmdlet == c.name && cm.parameter == p.name) {
            s.push_str(&format!("        [ArgumentCompleter(typeof(global::{module_ns}.{}))]\n", completer_class(cmp.id)));
        }
        // The transformation runs before the binder coerces the
        // argument to this property's type, so it is declared on the
        // property beside the parameter attributes.
        if let Some(t) = transforms.iter().find(|tr| tr.cmdlet == c.name && tr.parameter == p.name) {
            s.push_str(&format!("        [global::{module_ns}.{}]\n", transform_class(t.id)));
        }
        s.push_str(&format!(
            "        public {clr} {name} {{ get => _p{i}; set {{ _p{i} = value; _bound |= 1UL << {i}; _dirty |= 1UL << {i}; }} }}\n\n",
            name = p.name,
            i = p.index
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
            s.push_str(&format!(
                "            if ((_bound & (1UL << {i})) != 0 && !bound.ContainsKey(\"{name}\")) bound[\"{name}\"] = Pwrs.DynamicParametersBuilder.Bare(_p{i});\n",
                i = p.index,
                name = p.name
            ));
        }
        s.push_str("            return Pwrs.DynamicParametersBuilder.Build(Module, CmdletId, bound);\n        }\n\n");
    }

    s.push_str("        [StructLayout(LayoutKind.Sequential)]\n        private struct Block\n        {\n");
    s.push_str("            public ulong Dirty;\n            public ulong Bound;\n");
    for p in &c.params {
        s.push_str(&format!("            public {} P{};\n", slot_cs(&p.slot), p.index));
    }
    s.push_str("        }\n\n");

    s.push_str("        private unsafe void Run(uint phase)\n        {\n");
    s.push_str("            long t0 = Pwrs.Trace.Enabled ? Pwrs.Trace.Now() : 0;\n");
    s.push_str("            Block b = default;\n");
    if has_params {
        s.push_str("            b.Dirty = _dirty;\n            _dirty = 0;\n            b.Bound = _bound;\n");
    }
    let mut strings = Vec::new();
    let mut handles = Vec::new();
    for p in &c.params {
        match p.slot.as_str() {
            "bool" => s.push_str(&format!("            b.P{} = {}.IsPresent ? (byte)1 : (byte)0;\n", p.index, p.name)),
            "str16" => {
                s.push_str(&format!("            string s{} = {} ?? string.Empty;\n", p.index, p.name));
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
        s.push_str(&format!("{indent}b.P{i} = new Pwrs.PsStr16 {{ Ptr = (ushort*)c{i}, Len = (nuint)s{i}.Length }};\n"));
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
    s.push_str("        protected override void BeginProcessing() { if (NeedsPhase(Pwrs.Native.PhaseMaskBegin)) Run(Pwrs.Native.PhaseBegin); }\n");
    s.push_str("        protected override void ProcessRecord() => Run(Pwrs.Native.PhaseProcess);\n");
    s.push_str("        protected override void EndProcessing() { if (NeedsPhase(Pwrs.Native.PhaseMaskEnd)) Run(Pwrs.Native.PhaseEnd); }\n");
    s.push_str("    }\n\n");
    s
}

/// `Hello.Person` splits into namespace `Hello` and type `Person`; a
/// bare name lands in the module namespace.
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
    let present = format!("(b->Mask & (1UL << {i})) != 0");
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
                // The block carries the array IntoPs built, whose
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
fn arg_type(p: &MethodParam) -> String {
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
/// cast where the box already holds that type, a conversion where it
/// holds the wider one.
fn read_boxed(expr: &str, clr: &str, nullable: bool) -> String {
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
fn return_cs_type(r: &MethodRet) -> String {
    if return_nullable(r) { format!("{}?", cs_type(&r.clr)) } else { cs_type(&r.clr) }
}

/// The .NET interfaces a proxy class implements through the methods its
/// roles name, and the members that implement them. Each member calls
/// the generated method of the same role, so it crosses into Rust as
/// that method does. Members PowerShell reaches by name, the list's
/// `Count` and indexer and the `Equals` and `GetHashCode` overrides,
/// are public; the rest are explicit implementations, so a method of
/// the class may carry the interface member's name.
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
            "\n        /// <summary>The stream's elements, from {0} until it answers null. The\n        /// stream is the object's own, so a second enumeration continues it.</summary>\n        {GENERIC}.IEnumerator<{t}> {GENERIC}.IEnumerable<{t}>.GetEnumerator()\n        {{\n            while (true)\n            {{\n                var next = {0}();\n                if (next is null) yield break;\n                yield return {value};\n            }}\n        }}\n\n",
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
            "        /// <summary>The hash {0} answers, folded to 32 bits.</summary>\n        public override int GetHashCode()\n        {{\n            long hash = {0}();\n            return unchecked((int)hash ^ (int)(hash >> 32));\n        }}\n",
            hash.name
        ));
    }
    (interfaces, s)
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

/// One `#[psmethods]` method on a proxy class: a private argument
/// block, then the method packing its arguments the way a cmdlet's
/// `Run` packs parameters and making one `Call`.
///
/// A method with no receiver is `static` and calls through the
/// module's native table rather than through the object. The static
/// named `new` is the class's constructor: a public constructor that
/// chains to the internal one with the pointer a private static gets
/// from Rust, so a script reaches it as `[Type]::new(...)`.
fn proxy_method(module_ns: &str, module_class: &str, c: &Class, m: &Method) -> String {
    let (_, ty) = split_type_name(&c.name, module_ns);
    let native = format!("global::{module_ns}.{module_class}.Native");
    let block = format!("{}Args", cs_ident(&m.name));
    // A method taking a PsTask returns the task it hands Rust, of the
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
        .map(|p| if p.optional { format!("{} {} = null", arg_type(p), p.name) } else { format!("{} {}", arg_type(p), p.name) })
        .collect();
    if task.is_some() {
        params.push("global::System.Threading.CancellationToken cancellationToken = default".to_string());
    }
    let mut s = String::new();
    s.push_str(&format!("        [StructLayout(LayoutKind.Sequential)]\n        private struct {block}\n        {{\n            public ulong Bound;\n"));
    for p in &m.params {
        s.push_str(&format!("            public {} P{};\n", slot_cs(&p.slot), p.index));
    }
    s.push_str("        }\n\n");
    if !m.help.is_empty() {
        s.push_str(&format!("        /// <summary>{}</summary>\n", m.help));
    }
    // A proxy's constructor adopts the pointer of the value Rust made;
    // a copied class's copies the fields of the object Rust built.
    let copied = c.mode == "copied";
    if m.constructor {
        let names: Vec<&str> = m.params.iter().map(|p| p.name.as_str()).collect();
        let (chain, made) = if copied {
            (format!("this(default(global::Pwrs.FromFields), Construct{}({}))", m.index, names.join(", ")), ty.to_string())
        } else {
            (format!("this(Construct{}({}))", m.index, names.join(", ")), "IntPtr".to_string())
        };
        s.push_str(&format!("        public {ty}({}) : {chain} {{ }}\n\n", params.join(", ")));
        s.push_str(&format!("        private static unsafe {made} Construct{}({})\n        {{\n", m.index, params.join(", ")));
    } else if m.is_static {
        s.push_str(&format!("        public static unsafe {ret_ty} {}({})\n        {{\n", m.name, params.join(", ")));
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
        let n = &p.name;
        let bit = format!("__args.Bound |= 1UL << {i};");
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
            // The task source, held by a handle for the call and handed
            // over with its cancellation byte.
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
        format!("base.PwrsCallWith({}, &__args, {}, {}, &__args.P{})", m.index, m.mutable, peer.name, peer.index)
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
        // Rust answers the new value's pointer as a long.
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
/// the module declares no `new`, the class also keeps the public
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
        // A declared constructor answers the object the factory built;
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
        s.push_str(&proxy_method(module_ns, module_class, c, m));
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

/// The public type for one class, in its own namespace.
fn class_type(module_class: &str, module_ns: &str, c: &Class) -> String {
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
            let (interfaces, members) = interface_members(c, ty);
            let bases: Vec<String> = std::iter::once("Pwrs.ProxyBase".to_string()).chain(interfaces).collect();
            s.push_str(&format!("    public sealed class {ty} : {}\n    {{\n", bases.join(", ")));
            let reports_bytes = if c.native_bytes { "true" } else { "false" };
            s.push_str(&format!(
                "        internal {ty}(IntPtr instance) : base(global::{module_ns}.{module_class}.Native, {}, instance, {reports_bytes}) {{ }}\n",
                c.id
            ));
            for f in &c.fields {
                if !f.help.is_empty() {
                    s.push_str(&format!("        /// <summary>{}</summary>\n", f.help));
                }
                // Through base so a module method of the same name
                // cannot take the call: C# stops looking at the first
                // type with an applicable method.
                let prop_ty = field_property_type(f);
                let read = format!("base.PwrsGet({})", f.index);
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
            for m in &c.methods {
                s.push_str(&proxy_method(module_ns, module_class, c, m));
            }
            s.push_str(&members);
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
                s.push_str(&format!("        {} = {},\n", v.name, v.value));
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
            s.push_str("        public ulong Mask;\n    }\n\n");
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

/// The whole shell source for a module.
pub fn shell_source(m: &Module, native_base_name: &str) -> String {
    let ns = format!("Pwrs.Modules.{}", cs_ident(&m.name));
    let module_class = "PwrsModule";
    let mut s = String::new();
    s.push_str("using System;\nusing System.IO;\nusing System.Management.Automation;\nusing System.Management.Automation.Provider;\nusing System.Runtime.CompilerServices;\nusing System.Runtime.InteropServices;\n\n");
    // Stack frames here are not zeroed on entry. No generated method
    // reads a local before assigning it and none uses stackalloc. The
    // attribute is .NET only; netstandard2.0 has no such type.
    s.push_str("#if NET\n[module: SkipLocalsInit]\n#endif\n\n");
    for c in &m.classes {
        s.push_str(&class_type(module_class, &ns, c));
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
    // staging folder. The folder its script handed over comes first:
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
    // body is picked up by `Import-Module -Force` and nothing else is
    // asked of the author.
    s.push_str("        /// <summary>Points the module at its native library again when\n");
    s.push_str("        /// the file has changed since this session loaded it, and answers\n");
    s.push_str("        /// whether it did. A value made before the change is refused by the\n");
    s.push_str("        /// proxy that holds it rather than read against the new body.\n");
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

/// The class the engine calls at import and at removal, present only
/// when the module declares a hook for one or both. Each interface is
/// implemented only for the hook declared, so a module without hooks
/// has no such class and is not called.
fn lifecycle_class_source(module_class: &str, m: &Module) -> String {
    if !m.on_import && !m.on_remove {
        return String::new();
    }
    let mut interfaces = Vec::new();
    if m.on_import {
        interfaces.push("IModuleAssemblyInitializer");
    }
    if m.on_remove {
        interfaces.push("IModuleAssemblyCleanup");
    }
    let mut s = String::new();
    s.push_str(&format!("    public sealed class PwrsModuleLifecycle : {}\n    {{\n", interfaces.join(", ")));
    if m.on_import {
        // The library is pointed at a rebuilt body before the hook
        // runs, so the hook sees what this import is importing.
        s.push_str("        public void OnImport()\n        {\n");
        s.push_str(&format!("            {module_class}.ReloadNative();\n"));
        s.push_str(&format!("            {module_class}.Native.Lifecycle(0);\n"));
        s.push_str("        }\n");
    }
    if m.on_remove {
        s.push_str(&format!("        public void OnRemove(PSModuleInfo module) => {module_class}.Native.Lifecycle(1);\n"));
    }
    s.push_str("    }\n\n");
    s
}

/// The NavigationCmdletProvider subclass for one provider.
fn provider_class_source(class: &str, module_class: &str, p: &crate::descriptor::Provider) -> String {
    let caps = if p.capabilities.is_empty() {
        String::new()
    } else {
        // Capabilities map to ProviderCapabilities flags by name.
        let flags: Vec<String> = p.capabilities.iter().map(|c| format!("ProviderCapabilities.{c}")).collect();
        format!(", ProviderCapabilities = {}", flags.join(" | "))
    };
    let mut s = String::new();
    s.push_str(&format!("    [CmdletProvider({}, ProviderCapabilities.ShouldProcess{})]\n", cs_str(&p.name), caps));
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

/// Deterministic GUID from the module name, so a rebuilt manifest keeps
/// its identity.
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
/// match on, and carries no assembly name.
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
/// handed out again once its owner is gone, so a copy the script finds
/// staged under its process can be an earlier owner's; under the stamp
/// it was built from the same source.
pub fn bootstrap_stamp() -> String {
    shell_stamp(&crate::build::BOOTSTRAP_SOURCES.iter().map(|(_, text)| *text).collect::<Vec<_>>())
}

fn ps_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A string literal's text, or the last segment of a dotted constant
/// such as `VerbsCommon.Get`.
fn attribute_word(arg: &str) -> Option<String> {
    let arg = arg.trim();
    if let Some(inner) = arg.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(inner.to_string());
    }
    if arg.is_empty() || arg.contains(['=', ' ', '(']) {
        return None;
    }
    match arg.rsplit('.').next() {
        Some(last) => Some(last.to_string()),
        None => Some(arg.to_string()),
    }
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
    /// in `cmdlets`, because they sit on no class declaration or their
    /// verb and noun do not read as literals or `Verbs*.Name`
    /// constants. A cmdlet behind one of these is not exported.
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
/// may belong to holds nothing but whitespace and type modifiers.
fn only_modifiers(gap: &str) -> bool {
    const MODIFIERS: &[&str] =
        &["public", "internal", "private", "protected", "sealed", "static", "abstract", "partial", "unsafe", "new", "file"];
    gap.split_whitespace().all(|w| MODIFIERS.contains(&w))
}

/// The attribute groups belonging to the declaration at `decl`: the
/// run immediately before it, in source order. A group further back
/// than the first break belongs to something else, which is what
/// keeps a parameter's own attributes out.
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
/// keeps its last segment, and the `Attribute` suffix C# allows is
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
/// attribute list carries `[Cmdlet(verb, noun, ...)]`, with the
/// aliases that list declares. The verb and noun are read as string
/// literals or `Verbs*.Name` constants. Attributes inside the class
/// body, a parameter's `[Alias]` among them, belong to the members
/// they sit on and are not collected here.
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
    /// The package description, which becomes the module's; without
    /// one the module falls back to its name.
    pub description: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub keywords: &'a [String],
    /// A link to the module's license, which a gallery shows beside the
    /// listing. Cargo's own `license` is an SPDX name rather than a
    /// link, so this comes from `[package.metadata.pwrs] license-uri`.
    pub license_uri: Option<&'a str>,
    /// What changed in this version. A cargo manifest has no field for
    /// it; `[package.metadata.pwrs] release-notes` is where it lives.
    pub release_notes: Option<&'a str>,
    /// A link to the image a gallery shows beside the listing, from
    /// `[package.metadata.pwrs] icon-uri`. It is fetched over the
    /// network by whoever renders the listing, so it points at a served
    /// file rather than a path inside the module.
    pub icon_uri: Option<&'a str>,
    /// The rest of the module manifest, each from the
    /// `[package.metadata.pwrs]` key of the same name in kebab case. A
    /// cargo manifest has no field for any of them, and a field left
    /// unset is written nowhere, so a manifest carries no key it has no
    /// value for.
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
    // gallery tag may hold no whitespace.
    let mut tags: Vec<String> = vec!["pwrs".to_string()];
    for k in pkg.keywords {
        let tag = k.replace(char::is_whitespace, "-");
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    let tag_list: Vec<String> = tags.iter().map(|t| ps_str(t)).collect();
    // A key is written only where there is a value for it, so the
    // manifest carries no empty one for a gallery to read as set.
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

    // PSData holds what a gallery reads. Tags is always written because
    // the tag list always carries `pwrs`.
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

/// A module laid inside this one, and whether a failed import of it
/// writes a warning and lets this module's import go on, instead of
/// failing it.
pub struct Bundled<'a> {
    pub name: &'a str,
    pub warn_on_failure: bool,
}

/// The step of `module`'s script that imports the modules laid inside it,
/// in order, before its own shell: each from `<Name>/<Name>.psd1` beside
/// the script, leaving one the session already holds as it is. A failed
/// import fails `module`'s import with that error; for a module that
/// warns on failure, a warning names it and the error instead, and the
/// import goes on without it. Empty when nothing is bundled.
///
/// A module the session holds under the name is the one kept, whichever
/// folder it came from: PowerShell 7 refuses a second folder of a loaded
/// module, since its cmdlet names are taken. -Global puts the import
/// where a user's own Import-Module would, so the bundled cmdlets are
/// callable from the session and not only from this script, and
/// -DisableNameChecking keeps the bundled module's unapproved-verb
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
            Write-Warning ({module} + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)\n\
        }}\n\
    }}\n\
}}\n",
        names = names.join(", "),
        warned = warned.join(", "),
        module = ps_str(module)
    )
}

/// The module's script. When the bootstrap's loader cannot be reached on
/// a PowerShell 7 whose .NET is older than the .NET 8 the `net10.0`
/// assemblies reference, it names the PowerShell the module needs in
/// place of the missing-type error, which names neither; a load that
/// succeeds never evaluates the check. `desktop_dependencies` names
/// files the build placed in `netstandard2.0` for Windows PowerShell to
/// load beside the shell; the script copies each next to the staged
/// shell, where .NET Framework looks for a shell's dependencies, and
/// PowerShell 7 skips the step. `bundled` names the modules laid inside
/// this one, which [`bundled_step`] imports before the shell.
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
    // The native library loads in the shell's type initializer, which a
    // module's import hook reaches while the engine imports the shell and
    // ReloadNative reaches otherwise, so a refusal there, such as the CPU
    // check's, arrives wrapped in a TypeInitializationException; the
    // script rethrows the innermost, whose message is the reason.
    //
    // Runspaces importing at the same instant run the script side by
    // side. The bootstrap is staged in a folder named for the process,
    // the stamp of its source and the edition, under a temporary name
    // renamed into place, so a copy another import finds there is whole
    // and built from the same source; LoadFrom answers the assembly
    // already loaded for a second copy of it.
    format!(
        "{bundled_step}\
$root = $PSScriptRoot\n\
$tfm = if ($PSVersionTable.PSEdition -eq 'Core') {{ 'net10.0' }} else {{ 'netstandard2.0' }}\n\
if (-not ('Pwrs.Bootstrap.Loader' -as [type])) {{\n\
    $stage = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'pwrs-load', [string]$PID, 'boot', {boot_stamp}, $tfm)\n\
    $null = [System.IO.Directory]::CreateDirectory($stage)\n\
    $boot = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.dll')\n\
    if (-not [System.IO.File]::Exists($boot)) {{\n\
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
try {{\n\
Import-Module -Assembly $shell -Force\n\
$reload = $shell.GetTypes() | Where-Object {{ $_.Name -eq 'PwrsModule' }} | Select-Object -First 1\n\
if ($null -ne $reload) {{ $null = $reload::ReloadNative() }}\n\
}}\n\
catch {{\n\
$inner = $_.Exception\n\
while ($null -ne $inner.InnerException) {{ $inner = $inner.InnerException }}\n\
throw $inner\n\
}}\n\
Export-ModuleMember -Cmdlet {cmdlets} -Alias {aliases}\n",
        bundled_step = bundled_step,
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

    fn found(source: &str) -> Vec<(String, Vec<String>)> {
        hybrid_cmdlets(source).cmdlets.into_iter().map(|c| (c.name, c.aliases)).collect()
    }

    #[test]
    fn a_cmdlet_attribute_that_yields_no_cmdlet_is_counted_rather_than_dropped() {
        let orphan = "[Cmdlet(VerbsCommon.Get, \"Thing\")]\npublic enum NotAClass { A }";
        let scan = hybrid_cmdlets(orphan);
        assert!(scan.cmdlets.is_empty(), "nothing to export");
        assert_eq!(scan.unclaimed, 1, "the attribute sits on no class");

        // Named arguments the verb and noun reader does not accept.
        let unreadable = "[Cmdlet(verb: VerbsCommon.Get, noun: \"Thing\")]\npublic sealed class C : PSCmdlet { }";
        let scan = hybrid_cmdlets(unreadable);
        assert!(scan.cmdlets.is_empty(), "no name could be read");
        assert_eq!(scan.unclaimed, 1, "the attribute is on a class but yielded nothing");

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
        "#;
        assert_eq!(found(source), vec![("Get-Thing".to_string(), vec![]), ("Set-Thing".to_string(), vec![])]);
    }

    #[test]
    fn collects_class_aliases_whichever_side_of_the_cmdlet_attribute_they_sit() {
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

    /// The script hands its shell the module folder after the loader
    /// returns the shell and before the shell is imported, and the shell
    /// reads that folder before it asks the loader's map.
    #[test]
    fn the_script_hands_its_shell_the_module_root_before_import() {
        let module = demo_module();
        let psm1 = bootstrap_psm1(&module, &[], &[], &[]);
        let load = psm1.find("$shell = [Pwrs.Bootstrap.Loader]::Load(").expect("the load line");
        let hand = psm1
            .find("$shell.GetType('Pwrs.Modules.Demo.PwrsModuleRoot', $true).GetField('Value').SetValue($null, $root)")
            .expect("the handoff line");
        let import = psm1.find("Import-Module -Assembly $shell").expect("the import line");
        assert!(load < hand && hand < import, "{psm1}");

        let shell = shell_source(&module, "demo");
        assert!(shell.contains("public static class PwrsModuleRoot"), "{shell}");
        let handed = shell.find("string? handed = PwrsModuleRoot.Value;").expect("the handed root");
        let mapped = shell.find("Pwrs.Bootstrap.Loader.ModuleRootOf(here)").expect("the loader's map");
        assert!(handed < mapped, "{shell}");
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

    /// Every catch clause of a script whose block holds only whitespace,
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

    /// The script takes no lock and holds no empty catch, so it needs no
    /// param block and no suppression of PSScriptAnalyzer's empty-catch
    /// rule, with the desktop step and a bundled module that warns in place.
    #[test]
    fn the_script_takes_no_lock_and_holds_no_empty_catch() {
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

    /// A bundled module is imported into the session ahead of the
    /// shell, from its folder beside the script, and one the session
    /// already holds is left as it is; a module bundling none has no
    /// such step.
    #[test]
    fn the_script_imports_bundled_modules_before_its_shell() {
        let module = demo_module();
        let none = bootstrap_psm1(&module, &[], &[], &[]);
        assert!(!none.contains("$bundled"), "{none}");

        let psm1 = bootstrap_psm1(&module, &[], &[], &[Bundled { name: "Calc", warn_on_failure: false }, Bundled { name: "Tls", warn_on_failure: false }]);
        let list = psm1.find("foreach ($bundled in @('Calc', 'Tls')) {").expect("the bundled list");
        let held = psm1.find("if (-not (Get-Module -Name $bundled)) {").expect("the held check");
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
            .find("Write-Warning ('Demo' + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)")
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

    /// A module whose one proxy class carries `roles`, JSON members naming
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

    /// A `#[psmethods]` method as the descriptor carries it.
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
        let shell = shell_source(&module, "demo");
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
        let shell = shell_source(&module, "trex");
        assert_eq!(shell.matches("internal struct SegmentFields0").count(), 1, "{shell}");
        assert_eq!(shell.matches("internal struct SegmentFields1").count(), 1, "{shell}");
        assert!(shell.contains("var b = (SegmentFields1*)fields;"), "{shell}");
    }

    /// Cmdlets and providers whose Rust types share a name, from
    /// different modules, each get a class of their own, the id after
    /// the name; a Rust name no other shares keeps the plain class name.
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
        let shell = shell_source(&module, "trex");
        assert!(shell.contains("public sealed class GetCommand0 : Pwrs.RustCmdlet"), "{shell}");
        assert!(shell.contains("public sealed class GetCommand1 : Pwrs.RustCmdlet"), "{shell}");
        assert!(shell.contains("public sealed class GetOrbitCommand : Pwrs.RustCmdlet"), "{shell}");
        assert!(!shell.contains("public sealed class GetCommand :"), "{shell}");
        assert!(shell.contains("public sealed class StoreProvider0 : Pwrs.ProviderBase"), "{shell}");
        assert!(shell.contains("public sealed class StoreProvider1 : Pwrs.ProviderBase"), "{shell}");
    }

    #[test]
    fn a_stream_class_enumerates_until_its_next_method_answers_null() {
        let methods = format!("[{}]", method_json(0, "Take", "", r#"{"clr": "long", "optional": true}"#, true));
        let shell = shell_source(&class_with_roles(r#""next": "Take","#, &methods), "demo");
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
        let shell = shell_source(&class_with_roles(r#""compare": "Order", "equals": "Same", "hash": "Hash","#, &methods), "demo");
        assert!(shell.contains("public sealed class Row : Pwrs.ProxyBase, global::System.IComparable, global::System.IComparable<Row>, global::System.IEquatable<Row>"), "{shell}");
        assert!(shell.contains("if (other is null) throw new ArgumentNullException(nameof(other));"), "{shell}");
        assert!(shell.contains("base.PwrsCallWith(0, &__args, false, other, &__args.P0)"), "{shell}");
        assert!(shell.contains("public override int GetHashCode()") && shell.contains("long hash = Hash();"), "{shell}");
        assert!(shell.contains("public override bool Equals(object? obj)"), "{shell}");
    }

    #[test]
    fn a_method_taking_a_task_returns_it_and_takes_the_callers_cancellation_token() {
        let ms = r#"{"name": "ms", "rust": "ms", "index": 0, "clr": "long", "slot": "i64", "optional": false}"#;
        let task = r#"{"name": "task", "rust": "task", "index": 1, "clr": "long", "slot": "task", "optional": false}"#;
        let unit = r#"{"name": "task", "rust": "task", "index": 0, "clr": "void", "slot": "task", "optional": false}"#;
        let methods = format!("[{}, {}]", method_json(0, "WaitAsync", &format!("{ms}, {task}"), "null", true), method_json(1, "PingAsync", unit, "null", false));
        let module = class_with_roles("", &methods);
        validate(&module).expect("task methods");
        let shell = shell_source(&module, "demo");
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
    fn a_method_argument_may_carry_any_name_the_body_uses_for_its_own() {
        let names = ["b", "r", "s0", "h0", "c0", "t0"]
            .iter()
            .enumerate()
            .map(|(i, n)| format!(r#"{{"name": "{n}", "rust": "{n}", "index": {i}, "clr": "long", "slot": "i64", "optional": false}}"#))
            .collect::<Vec<_>>()
            .join(", ");
        let shell = shell_source(&class_with_roles("", &format!("[{}]", method_json(0, "Mix", &names, LONG, false))), "demo");
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
        }
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
            keywords: &keywords,
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

    /// PowerShell itself parses a manifest carrying every key, and
    /// reads back the values written. String assertions alone cannot
    /// say the file is valid data; this is the only check that can.
    #[test]
    fn powershell_parses_a_manifest_carrying_every_key() {
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
            keywords: &keywords,
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
        // the key that carries it.
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
        // Removed only once the assertions hold, so a failure leaves
        // the file that produced it.
        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    /// A value carrying a quote is escaped rather than closing the
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

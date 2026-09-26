//! C# shell, manifest, and bootstrap script generation from the
//! descriptor.

use crate::descriptor::{Class, Cmdlet, Completer, Field, Method, MethodParam, MethodRet, Module, Param, Transform};
use crate::Error;

/// Rejects a descriptor the shell cannot express.
pub fn validate(m: &Module) -> Result<(), Error> {
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

fn cmdlet_class(module_class: &str, module_ns: &str, c: &Cmdlet, completers: &[Completer], transforms: &[Transform]) -> String {
    let class = format!("{}Command", cs_ident(&c.rust));
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
        s.push_str("        public object GetDynamicParameters()\n        {\n");
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

/// Expression converting the object `r` a proxy call returned to the
/// method's declared return type.
fn return_expr(r: &MethodRet) -> String {
    let t = cs_type(&r.clr);
    if r.clr == "object" {
        "r".to_string()
    } else if r.clr.ends_with("[]") {
        format!("({t}?)LanguagePrimitives.ConvertTo(r, typeof({t}))")
    } else {
        read_boxed("r", &r.clr, return_nullable(r))
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
    let ret_ty = match &m.ret {
        None => "void".to_string(),
        Some(r) if return_nullable(r) => format!("{}?", cs_type(&r.clr)),
        Some(r) => cs_type(&r.clr),
    };
    let params: Vec<String> = m
        .params
        .iter()
        .map(|p| if p.optional { format!("{} {} = null", arg_type(p), p.name) } else { format!("{} {}", arg_type(p), p.name) })
        .collect();
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
    s.push_str(&format!("            {block} b = default;\n"));
    let mut strings = Vec::new();
    let mut handles = Vec::new();
    for p in &m.params {
        let i = p.index;
        let n = &p.name;
        let bit = format!("b.Bound |= 1UL << {i};");
        match p.slot.as_str() {
            "bool" if p.optional => s.push_str(&format!("            if ({n}.HasValue) {{ b.P{i} = {n}.Value ? (byte)1 : (byte)0; {bit} }}\n")),
            "bool" => s.push_str(&format!("            b.P{i} = {n} ? (byte)1 : (byte)0;\n            {bit}\n")),
            "str16" if p.optional => {
                s.push_str(&format!(
                    "            string? t{i} = {n} == null ? null : LanguagePrimitives.ConvertTo<string>({n});\n            string s{i} = t{i} ?? string.Empty;\n            if (t{i} != null) {{ {bit} }}\n"
                ));
                strings.push(i);
            }
            "str16" => {
                s.push_str(&format!("            string s{i} = {n} ?? string.Empty;\n            if ({n} != null) {{ {bit} }}\n"));
                strings.push(i);
            }
            "handle" if p.value_type && p.optional => {
                s.push_str(&format!("            GCHandle h{i} = {n}.HasValue ? GCHandle.Alloc((object){n}.Value) : default;\n            if ({n}.HasValue) {{ {bit} }}\n"));
                handles.push(i);
            }
            "handle" if p.value_type => {
                s.push_str(&format!("            GCHandle h{i} = GCHandle.Alloc((object){n});\n            {bit}\n"));
                handles.push(i);
            }
            "handle" => {
                s.push_str(&format!("            GCHandle h{i} = {n} == null ? default : GCHandle.Alloc({n});\n            if ({n} != null) {{ {bit} }}\n"));
                handles.push(i);
            }
            _number if p.optional => s.push_str(&format!("            if ({n}.HasValue) {{ b.P{i} = {n}.Value; {bit} }}\n")),
            _number => s.push_str(&format!("            b.P{i} = {n};\n            {bit}\n")),
        }
    }
    // A static has no object to call through, so it names the module's
    // native table and the class id itself. A method with a receiver
    // enters the object's gate exclusively when it takes `&mut self`
    // and as shared when it takes `&self`.
    let invoke = if m.is_static {
        format!("global::Pwrs.StaticCall.Invoke({native}, {}, {}, &b)", c.id, m.index)
    } else {
        format!("base.PwrsCall({}, &b, {})", m.index, m.mutable)
    };
    let call = match &m.ret {
        Some(_returns) => {
            s.push_str("            object? r;\n");
            format!("r = {invoke};")
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
        s.push_str(&format!("{indent}b.P{i} = h{i}.IsAllocated ? GCHandle.ToIntPtr(h{i}) : IntPtr.Zero;\n"));
    }
    for i in &strings {
        s.push_str(&format!("{indent}fixed (char* c{i} = s{i})\n{indent}{{\n"));
        indent.push_str("    ");
        s.push_str(&format!("{indent}b.P{i} = new Pwrs.PsStr16 {{ Ptr = (ushort*)c{i}, Len = (nuint)s{i}.Length }};\n"));
    }
    s.push_str(&format!("{indent}{call}\n"));
    for _ in &strings {
        indent.truncate(indent.len() - 4);
        s.push_str(&format!("{indent}}}\n"));
    }
    if guarded {
        s.push_str("            }\n            finally\n            {\n");
        for i in &handles {
            s.push_str(&format!("                if (h{i}.IsAllocated) h{i}.Free();\n"));
        }
        s.push_str("            }\n");
    }
    if m.constructor && copied {
        s.push_str(&format!("            return ({ty})r!;\n"));
    } else if m.constructor {
        // Rust answers the new value's pointer as a long.
        s.push_str("            return (IntPtr)(long)r!;\n");
    } else if let Some(r) = &m.ret {
        s.push_str(&format!("            return {};\n", return_expr(r)));
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
            s.push_str("    }\n}\n\n");
        }
        "proxy" => {
            s.push_str(&format!("namespace {ns}\n{{\n"));
            if !c.description.is_empty() {
                s.push_str(&format!("    /// <summary>{}</summary>\n", c.description.replace('\n', " ")));
            }
            s.push_str(&format!("    public sealed class {ty} : Pwrs.ProxyBase\n    {{\n"));
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
            let block = format!("{}Fields", cs_ident(&c.rust));
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
        s.push_str(&cmdlet_class(module_class, &ns, c, &m.completers, &m.transforms));
    }
    for cmp in &m.completers {
        s.push_str(&completer_class_source(module_class, cmp));
    }
    for t in &m.transforms {
        s.push_str(&transform_class_source(module_class, t));
    }
    for p in &m.providers {
        s.push_str(&provider_class_source(module_class, p));
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
fn provider_class_source(module_class: &str, p: &crate::descriptor::Provider) -> String {
    let class = format!("{}Provider", cs_ident(&p.rust));
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
                        if positional.len() >= 2 {
                            if let (Some(verb), Some(noun)) = (attribute_word(positional[0]), attribute_word(positional[1])) {
                                name = Some(format!("{verb}-{noun}"));
                            }
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
            if let Some((attr_name, _args)) = attribute_parts(attr) {
                if attr_name == "Cmdlet" {
                    declared += 1;
                }
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
    // A concurrent import may copy the same file first, so a failed copy
    // counts only when the file is still not there.
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
try {{ [System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, $dep), $to) }}\n\
catch [System.IO.IOException] {{ if (-not [System.IO.File]::Exists($to)) {{ throw }} }}\n\
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
    // The whole script runs under a mutex named for the process, so
    // runspaces importing at the same instant take turns: the bootstrap
    // copy, the staging and the engine's own module table are shared by
    // all of them. A mutex a dead thread left behind is still acquired,
    // which is what the caught exception reports.
    format!(
        "$pwrsImportLock = New-Object System.Threading.Mutex($false, ('Local\\pwrs-import-' + $PID))\n\
try {{ $null = $pwrsImportLock.WaitOne() }} catch [System.Threading.AbandonedMutexException] {{ }}\n\
try {{\n\
{bundled_step}\
$root = $PSScriptRoot\n\
$tfm = if ($PSVersionTable.PSEdition -eq 'Core') {{ 'net10.0' }} else {{ 'netstandard2.0' }}\n\
if (-not ('Pwrs.Bootstrap.Loader' -as [type])) {{\n\
    $stage = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'pwrs-load', $PID)\n\
    $null = [System.IO.Directory]::CreateDirectory($stage)\n\
    $boot = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.dll')\n\
    [System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.dll'), $boot, $true)\n\
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
Export-ModuleMember -Cmdlet {cmdlets} -Alias {aliases}\n\
}} finally {{\n\
$pwrsImportLock.ReleaseMutex()\n\
$pwrsImportLock.Dispose()\n\
}}\n",
        bundled_step = bundled_step,
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
        let mut hosts: Vec<(&str, RunScript)> = vec![("pwsh", pwrs_build::pwsh::run_pwsh_script)];
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
        let out = std::process::Command::new(pwrs_build::pwsh::pwsh_exe())
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .expect("run pwsh");
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

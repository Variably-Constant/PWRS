#Requires -Version 7
# Reads the .NET types a module's proxy classes derive from or implement,
# from the reference assemblies each of its shell's compiles takes, and
# writes what the shell needs of each as JSON: its kind, whether it is
# sealed or abstract, its constructors, its members with their C#
# signatures and where each is declared, and the interfaces it implements.
#
# The request names the types and, for each target framework, its
# reference assemblies:
#   {"targets": [{"name": "net10.0", "references": ["...dll", ...]}, ...],
#    "types": ["System.IO.Stream", "System.Collections.Generic.IComparer<long>"]}
param(
    [Parameter(Mandatory)] [string] $Request,
    [Parameter(Mandatory)] [string] $Output
)

$ErrorActionPreference = 'Stop'

if (-not ('Pwrs.Build.Members' -as [type])) {
    Add-Type -TypeDefinition @'
#nullable enable
using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text.Json;

namespace Pwrs.Build
{
    /// <summary>
    /// A type as a signature names it, with its text as C# writes it and
    /// what it is built from, so a base type can be followed to its
    /// definition with its type arguments.
    /// </summary>
    public sealed class TypeName
    {
        public string Text = "";
        public bool ValueType;
        public bool ByRef;
        /// <summary>modreq(InAttribute): an in parameter.</summary>
        public bool InMod;
        /// <summary>modreq(IsExternalInit): an init-only setter.</summary>
        public bool InitMod;
        /// <summary>A pointer, a function pointer or a pinned type.</summary>
        public bool Unsupported;
        /// <summary>Names a type parameter of the method itself.</summary>
        public bool MethodGeneric;
        public MetadataReader? Reader;
        public EntityHandle Handle;
        public TypeName? Generic;
        public ImmutableArray<TypeName> Args = ImmutableArray<TypeName>.Empty;

        public TypeName With(string text)
        {
            return new TypeName { Text = text, ValueType = ValueType, Unsupported = Unsupported, MethodGeneric = MethodGeneric };
        }
    }

    /// <summary>The type arguments a type's own type parameters stand for.</summary>
    public sealed class Context
    {
        public ImmutableArray<TypeName> Args = ImmutableArray<TypeName>.Empty;
    }

    internal sealed class Provider : ISignatureTypeProvider<TypeName, Context>
    {
        private readonly Members _all;

        public Provider(Members all) { _all = all; }

        public TypeName GetPrimitiveType(PrimitiveTypeCode code)
        {
            switch (code)
            {
                case PrimitiveTypeCode.Boolean: return new TypeName { Text = "bool", ValueType = true };
                case PrimitiveTypeCode.Byte: return new TypeName { Text = "byte", ValueType = true };
                case PrimitiveTypeCode.Char: return new TypeName { Text = "char", ValueType = true };
                case PrimitiveTypeCode.Double: return new TypeName { Text = "double", ValueType = true };
                case PrimitiveTypeCode.Int16: return new TypeName { Text = "short", ValueType = true };
                case PrimitiveTypeCode.Int32: return new TypeName { Text = "int", ValueType = true };
                case PrimitiveTypeCode.Int64: return new TypeName { Text = "long", ValueType = true };
                case PrimitiveTypeCode.IntPtr: return new TypeName { Text = "nint", ValueType = true };
                case PrimitiveTypeCode.Object: return new TypeName { Text = "object" };
                case PrimitiveTypeCode.SByte: return new TypeName { Text = "sbyte", ValueType = true };
                case PrimitiveTypeCode.Single: return new TypeName { Text = "float", ValueType = true };
                case PrimitiveTypeCode.String: return new TypeName { Text = "string" };
                case PrimitiveTypeCode.UInt16: return new TypeName { Text = "ushort", ValueType = true };
                case PrimitiveTypeCode.UInt32: return new TypeName { Text = "uint", ValueType = true };
                case PrimitiveTypeCode.UInt64: return new TypeName { Text = "ulong", ValueType = true };
                case PrimitiveTypeCode.UIntPtr: return new TypeName { Text = "nuint", ValueType = true };
                case PrimitiveTypeCode.Void: return new TypeName { Text = "void" };
                default: return new TypeName { Text = "System.TypedReference", ValueType = true, Unsupported = true };
            }
        }

        public TypeName GetTypeFromDefinition(MetadataReader reader, TypeDefinitionHandle handle, byte rawTypeKind)
        {
            return new TypeName { Text = Members.Keyword(Members.DefinitionName(reader, handle)), ValueType = rawTypeKind == (byte)SignatureTypeKind.ValueType, Reader = reader, Handle = handle };
        }

        public TypeName GetTypeFromReference(MetadataReader reader, TypeReferenceHandle handle, byte rawTypeKind)
        {
            return new TypeName { Text = Members.Keyword(Members.ReferenceName(reader, handle)), ValueType = rawTypeKind == (byte)SignatureTypeKind.ValueType, Reader = reader, Handle = handle };
        }

        public TypeName GetTypeFromSpecification(MetadataReader reader, Context context, TypeSpecificationHandle handle, byte rawTypeKind)
        {
            return reader.GetTypeSpecification(handle).DecodeSignature(this, context);
        }

        public TypeName GetSZArrayType(TypeName element)
        {
            return new TypeName { Text = element.Text + "[]", Unsupported = element.Unsupported, MethodGeneric = element.MethodGeneric };
        }

        public TypeName GetArrayType(TypeName element, ArrayShape shape)
        {
            return new TypeName { Text = element.Text + "[" + new string(',', shape.Rank - 1) + "]", Unsupported = element.Unsupported, MethodGeneric = element.MethodGeneric };
        }

        public TypeName GetByReferenceType(TypeName element)
        {
            var t = element.With(element.Text);
            t.ByRef = true;
            t.InMod = element.InMod;
            return t;
        }

        public TypeName GetPointerType(TypeName element)
        {
            var t = element.With(element.Text + "*");
            t.Unsupported = true;
            return t;
        }

        public TypeName GetPinnedType(TypeName element)
        {
            var t = element.With(element.Text);
            t.Unsupported = true;
            return t;
        }

        public TypeName GetFunctionPointerType(MethodSignature<TypeName> signature)
        {
            return new TypeName { Text = "delegate*", ValueType = true, Unsupported = true };
        }

        public TypeName GetGenericInstantiation(TypeName generic, ImmutableArray<TypeName> arguments)
        {
            bool unsupported = generic.Unsupported || arguments.Any(a => a.Unsupported);
            bool methodGeneric = arguments.Any(a => a.MethodGeneric);
            string text = generic.Text == "System.Nullable" && arguments.Length == 1
                ? arguments[0].Text + "?"
                : generic.Text + "<" + string.Join(", ", arguments.Select(a => a.Text)) + ">";
            return new TypeName { Text = text, ValueType = generic.ValueType, Unsupported = unsupported, MethodGeneric = methodGeneric, Generic = generic, Args = arguments };
        }

        public TypeName GetGenericTypeParameter(Context context, int index)
        {
            if (index < context.Args.Length) return context.Args[index];
            return new TypeName { Text = "!" + index, Unsupported = true };
        }

        public TypeName GetGenericMethodParameter(Context context, int index)
        {
            return new TypeName { Text = "!!" + index, MethodGeneric = true };
        }

        public TypeName GetModifiedType(TypeName modifier, TypeName unmodified, bool isRequired)
        {
            var t = unmodified.With(unmodified.Text);
            t.ByRef = unmodified.ByRef;
            t.Reader = unmodified.Reader;
            t.Handle = unmodified.Handle;
            t.Generic = unmodified.Generic;
            t.Args = unmodified.Args;
            t.InMod = unmodified.InMod || (isRequired && modifier.Text == "System.Runtime.InteropServices.InAttribute");
            t.InitMod = unmodified.InitMod || (isRequired && modifier.Text == "System.Runtime.CompilerServices.IsExternalInit");
            return t;
        }
    }

    /// <summary>
    /// The types of one target framework's reference assemblies, by the
    /// names C# gives them, and the description of each requested one.
    /// </summary>
    public sealed class Members
    {
        private readonly Dictionary<string, (MetadataReader Reader, TypeDefinitionHandle Handle, string Assembly)> _types = new Dictionary<string, (MetadataReader, TypeDefinitionHandle, string)>(StringComparer.Ordinal);
        private readonly Provider _provider;

        private Members()
        {
            _provider = new Provider(this);
        }

        /// <summary>Reads the request and writes the description of every type it names for every target.</summary>
        public static void Run(string requestPath, string outputPath)
        {
            using JsonDocument request = JsonDocument.Parse(File.ReadAllText(requestPath));
            string[] names = request.RootElement.GetProperty("types").EnumerateArray().Select(e => e.GetString() ?? "").ToArray();
            using var buffer = new MemoryStream();
            using (var w = new Utf8JsonWriter(buffer, new JsonWriterOptions { Indented = true }))
            {
                w.WriteStartObject();
                w.WriteStartObject("targets");
                foreach (JsonElement target in request.RootElement.GetProperty("targets").EnumerateArray())
                {
                    string name = target.GetProperty("name").GetString() ?? "";
                    var readers = new List<PEReader>();
                    try
                    {
                        var all = new Members();
                        foreach (JsonElement reference in target.GetProperty("references").EnumerateArray())
                        {
                            string path = reference.GetString() ?? "";
                            var pe = new PEReader(File.OpenRead(path));
                            readers.Add(pe);
                            if (pe.HasMetadata) all.Index(pe.GetMetadataReader(), Path.GetFileNameWithoutExtension(path));
                        }
                        w.WriteStartObject(name);
                        foreach (string type in names)
                        {
                            w.WritePropertyName(type);
                            all.Describe(w, type);
                        }
                        w.WriteEndObject();
                    }
                    finally
                    {
                        foreach (var pe in readers) pe.Dispose();
                    }
                }
                w.WriteEndObject();
                w.WriteEndObject();
            }
            File.WriteAllBytes(outputPath, buffer.ToArray());
        }

        /// <summary>Adds every public type an assembly defines, nested ones under their outer type's name.</summary>
        private void Index(MetadataReader reader, string assembly)
        {
            foreach (TypeDefinitionHandle h in reader.TypeDefinitions)
            {
                TypeDefinition d = reader.GetTypeDefinition(h);
                if (!IsVisible(reader, d)) continue;
                string key = Key(DefinitionName(reader, h), d.GetGenericParameters().Count);
                if (!_types.ContainsKey(key)) _types[key] = (reader, h, assembly);
            }
        }

        private static bool IsVisible(MetadataReader reader, TypeDefinition d)
        {
            TypeAttributes v = d.Attributes & TypeAttributes.VisibilityMask;
            if (v == TypeAttributes.Public) return true;
            if (v != TypeAttributes.NestedPublic) return false;
            return IsVisible(reader, reader.GetTypeDefinition(d.GetDeclaringType()));
        }

        private static string Key(string name, int arity)
        {
            return arity == 0 ? name : name + "`" + arity;
        }

        public static string StripArity(string name)
        {
            int at = name.IndexOf('`');
            return at < 0 ? name : name.Substring(0, at);
        }

        public static string DefinitionName(MetadataReader reader, TypeDefinitionHandle handle)
        {
            TypeDefinition d = reader.GetTypeDefinition(handle);
            string name = StripArity(reader.GetString(d.Name));
            TypeDefinitionHandle outer = d.GetDeclaringType();
            if (!outer.IsNil) return DefinitionName(reader, outer) + "." + name;
            string ns = reader.GetString(d.Namespace);
            return ns.Length == 0 ? name : ns + "." + name;
        }

        public static string ReferenceName(MetadataReader reader, TypeReferenceHandle handle)
        {
            TypeReference t = reader.GetTypeReference(handle);
            string name = StripArity(reader.GetString(t.Name));
            if (t.ResolutionScope.Kind == HandleKind.TypeReference) return ReferenceName(reader, (TypeReferenceHandle)t.ResolutionScope) + "." + name;
            string ns = reader.GetString(t.Namespace);
            return ns.Length == 0 ? name : ns + "." + name;
        }

        private static readonly Dictionary<string, string> Keywords = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["System.Boolean"] = "bool", ["System.Char"] = "char", ["System.SByte"] = "sbyte", ["System.Byte"] = "byte",
            ["System.Int16"] = "short", ["System.UInt16"] = "ushort", ["System.Int32"] = "int", ["System.UInt32"] = "uint",
            ["System.Int64"] = "long", ["System.UInt64"] = "ulong", ["System.Single"] = "float", ["System.Double"] = "double",
            ["System.Decimal"] = "decimal", ["System.String"] = "string", ["System.Object"] = "object",
            ["System.IntPtr"] = "nint", ["System.UIntPtr"] = "nuint", ["System.Void"] = "void",
        };

        /// <summary>The C# keyword for a type that has one, else the name as it is.</summary>
        public static string Keyword(string full)
        {
            return Keywords.TryGetValue(full, out string? word) ? word : full;
        }

        /// <summary>The full name of each type a C# keyword names, which the index has it under.</summary>
        private static readonly Dictionary<string, string> FullNames = Keywords.ToDictionary(pair => pair.Value, pair => pair.Key, StringComparer.Ordinal);

        private static readonly HashSet<string> ValueKeywords = new HashSet<string>(StringComparer.Ordinal)
        {
            "bool", "char", "sbyte", "byte", "short", "ushort", "int", "uint", "long", "ulong", "float", "double", "decimal", "nint", "nuint",
        };

        /// <summary>The definition a type name refers to, followed through a reference by its name.</summary>
        private (MetadataReader Reader, TypeDefinitionHandle Handle, string Assembly)? Resolve(TypeName t)
        {
            TypeName named = t.Generic ?? t;
            if (named.Reader is null) return null;
            if (named.Handle.Kind == HandleKind.TypeDefinition)
            {
                var h = (TypeDefinitionHandle)named.Handle;
                string key = Key(DefinitionName(named.Reader, h), named.Reader.GetTypeDefinition(h).GetGenericParameters().Count);
                return _types.TryGetValue(key, out var found) ? found : (named.Reader, h, "");
            }
            if (named.Handle.Kind == HandleKind.TypeReference)
            {
                string key = Key(ReferenceName(named.Reader, (TypeReferenceHandle)named.Handle), t.Args.Length);
                return _types.TryGetValue(key, out var found) ? found : ((MetadataReader, TypeDefinitionHandle, string)?)null;
            }
            return null;
        }

        /// <summary>The type an EntityHandle in a type's own metadata names: its base type or an interface.</summary>
        private TypeName Named(MetadataReader reader, EntityHandle handle, Context context)
        {
            switch (handle.Kind)
            {
                case HandleKind.TypeDefinition: return _provider.GetTypeFromDefinition(reader, (TypeDefinitionHandle)handle, 0);
                case HandleKind.TypeReference: return _provider.GetTypeFromReference(reader, (TypeReferenceHandle)handle, 0);
                default: return reader.GetTypeSpecification((TypeSpecificationHandle)handle).DecodeSignature(_provider, context);
            }
        }

        /// <summary>
        /// Splits a C# type name into its generic type's name and its type
        /// arguments, each one parsed the same way.
        /// </summary>
        private TypeName Parse(string text)
        {
            text = text.Trim();
            if (text.EndsWith("[]", StringComparison.Ordinal)) return _provider.GetSZArrayType(Parse(text.Substring(0, text.Length - 2)));
            if (text.EndsWith("?", StringComparison.Ordinal))
            {
                TypeName inner = Parse(text.Substring(0, text.Length - 1));
                return new TypeName { Text = inner.Text + "?", ValueType = true, Generic = new TypeName { Text = "System.Nullable", ValueType = true }, Args = ImmutableArray.Create(inner) };
            }
            int open = text.IndexOf('<');
            string written = open < 0 ? text : text.Substring(0, open).Trim();
            string name = Keyword(written);
            string full = FullNames.TryGetValue(written, out string? spelled) ? spelled : written;
            var args = ImmutableArray.CreateBuilder<TypeName>();
            if (open >= 0 && text.EndsWith(">", StringComparison.Ordinal))
            {
                string inside = text.Substring(open + 1, text.Length - open - 2);
                int depth = 0, start = 0;
                for (int i = 0; i < inside.Length; i++)
                {
                    char c = inside[i];
                    if (c == '<' || c == '[') depth++;
                    else if (c == '>' || c == ']') depth--;
                    else if (c == ',' && depth == 0)
                    {
                        args.Add(Parse(inside.Substring(start, i - start)));
                        start = i + 1;
                    }
                }
                args.Add(Parse(inside.Substring(start)));
            }
            bool value = ValueKeywords.Contains(name);
            MetadataReader? reader = null;
            EntityHandle handle = default;
            if (_types.TryGetValue(Key(full, args.Count), out var found))
            {
                reader = found.Reader;
                handle = found.Handle;
                value = IsValueType(found.Reader, found.Reader.GetTypeDefinition(found.Handle));
            }
            string rendered = args.Count == 0 ? name : name + "<" + string.Join(", ", args.Select(a => a.Text)) + ">";
            var plain = new TypeName { Text = name, ValueType = value, Reader = reader, Handle = handle };
            return args.Count == 0 ? plain : new TypeName { Text = rendered, ValueType = value, Generic = plain, Args = args.ToImmutable() };
        }

        private static string BaseName(MetadataReader reader, TypeDefinition d)
        {
            if (d.BaseType.IsNil) return "";
            switch (d.BaseType.Kind)
            {
                case HandleKind.TypeReference: return ReferenceName(reader, (TypeReferenceHandle)d.BaseType);
                case HandleKind.TypeDefinition: return DefinitionName(reader, (TypeDefinitionHandle)d.BaseType);
                default: return "";
            }
        }

        private static bool IsValueType(MetadataReader reader, TypeDefinition d)
        {
            string b = BaseName(reader, d);
            return b == "System.ValueType" || b == "System.Enum";
        }

        private static string Kind(MetadataReader reader, TypeDefinition d)
        {
            if ((d.Attributes & TypeAttributes.Interface) != 0) return "interface";
            switch (BaseName(reader, d))
            {
                case "System.Enum": return "enum";
                case "System.ValueType": return "struct";
                case "System.MulticastDelegate": return "delegate";
                default: return "class";
            }
        }

        private static string? Access(MethodAttributes a)
        {
            switch (a & MethodAttributes.MemberAccessMask)
            {
                case MethodAttributes.Public: return "public";
                case MethodAttributes.Family: return "protected";
                case MethodAttributes.FamORAssem: return "protected";
                default: return null;
            }
        }

        private static string Slot(MethodAttributes a, bool inInterface)
        {
            bool isStatic = (a & MethodAttributes.Static) != 0;
            bool isAbstract = (a & MethodAttributes.Abstract) != 0;
            bool isVirtual = (a & MethodAttributes.Virtual) != 0;
            if (inInterface)
            {
                if (isStatic) return isAbstract || isVirtual ? "static-abstract" : "static";
                return isAbstract ? "abstract" : isVirtual ? "default" : "none";
            }
            if (isStatic) return "static";
            if (isAbstract) return "abstract";
            if (isVirtual) return (a & MethodAttributes.Final) != 0 ? "sealed" : "virtual";
            return "none";
        }

        /// <summary>Writes one requested type's description.</summary>
        private void Describe(Utf8JsonWriter w, string requested)
        {
            w.WriteStartObject();
            TypeName named = Parse(requested);
            var found = Resolve(named);
            if (found is null)
            {
                w.WriteBoolean("found", false);
                string bare = (named.Generic ?? named).Text;
                int[] arities = _types.Keys.Where(k => k == bare || k.StartsWith(bare + "`", StringComparison.Ordinal))
                    .Select(k => k == bare ? 0 : int.Parse(k.Substring(bare.Length + 1))).OrderBy(n => n).ToArray();
                string missing = arities.Length == 0
                    ? $"{requested} is in none of the reference assemblies the shell compiles against, as a public type"
                    : $"{bare} takes " + string.Join(" or ", arities.Select(n => n == 0 ? "no type arguments" : n == 1 ? "1 type argument" : $"{n} type arguments")) + $", and {requested} gives {named.Args.Length}";
                w.WriteString("problem", missing);
                w.WriteEndObject();
                return;
            }
            var (reader, handle, assembly) = found.Value;
            TypeDefinition d = reader.GetTypeDefinition(handle);
            var context = new Context { Args = named.Args };
            string kind = Kind(reader, d);
            bool isInterface = kind == "interface";
            w.WriteBoolean("found", true);
            w.WriteString("name", named.Text);
            w.WriteString("assembly", assembly);
            w.WriteString("kind", kind);
            w.WriteBoolean("sealed", (d.Attributes & TypeAttributes.Sealed) != 0);
            w.WriteBoolean("abstract", (d.Attributes & TypeAttributes.Abstract) != 0);

            w.WriteStartArray("constructors");
            if (!isInterface)
            {
                foreach (MethodDefinitionHandle mh in d.GetMethods())
                {
                    MethodDefinition m = reader.GetMethodDefinition(mh);
                    if (reader.GetString(m.Name) != ".ctor") continue;
                    string? access = Access(m.Attributes);
                    if (access is null) continue;
                    MethodSignature<TypeName> sig = m.DecodeSignature(_provider, context);
                    w.WriteStartObject();
                    w.WriteString("access", access);
                    WriteParams(w, reader, m, sig);
                    w.WriteEndObject();
                }
            }
            w.WriteEndArray();

            var interfaces = new List<string>();
            var visited = new HashSet<string>(StringComparer.Ordinal);
            w.WriteStartArray("members");
            string? problem = null;
            if (isInterface)
            {
                problem = WalkInterface(w, reader, handle, context, named.Text, interfaces, visited);
            }
            else
            {
                // A class and its bases share one set: the most derived
                // declaration of a signature is the one a derived class
                // overrides or hides.
                var seenMethods = new HashSet<string>(StringComparer.Ordinal);
                var seenProperties = new Dictionary<string, PropertySeen>(StringComparer.Ordinal);
                var seenEvents = new HashSet<string>(StringComparer.Ordinal);
                MetadataReader r = reader;
                TypeDefinitionHandle h = handle;
                Context c = context;
                string from = named.Text;
                while (true)
                {
                    TypeDefinition current = r.GetTypeDefinition(h);
                    WriteMembers(w, r, current, c, from, false, seenMethods, seenProperties, seenEvents);
                    foreach (InterfaceImplementationHandle ih in current.GetInterfaceImplementations())
                    {
                        TypeName i = Named(r, r.GetInterfaceImplementation(ih).Interface, c);
                        problem ??= CollectInterfaces(i, interfaces, visited);
                    }
                    if (current.BaseType.IsNil) break;
                    TypeName b = Named(r, current.BaseType, c);
                    var next = Resolve(b);
                    if (next is null)
                    {
                        problem ??= $"its base type {b.Text} is in none of the reference assemblies, so the members it declares are unknown";
                        break;
                    }
                    (r, h, _) = next.Value;
                    c = new Context { Args = b.Args };
                    from = b.Text;
                }
            }
            w.WriteEndArray();
            w.WriteStartArray("interfaces");
            foreach (string i in interfaces) w.WriteStringValue(i);
            w.WriteEndArray();
            if (problem is null) w.WriteNull("problem"); else w.WriteString("problem", problem);
            w.WriteEndObject();
        }

        /// <summary>
        /// Adds an interface and every interface it extends to the list,
        /// once each, and returns what could not be read, if anything.
        /// </summary>
        private string? CollectInterfaces(TypeName i, List<string> interfaces, HashSet<string> visited)
        {
            if (!visited.Add(i.Text)) return null;
            interfaces.Add(i.Text);
            var found = Resolve(i);
            if (found is null) return $"the interface {i.Text} it implements is in none of the reference assemblies";
            var (r, h, _) = found.Value;
            var c = new Context { Args = i.Args };
            string? problem = null;
            foreach (InterfaceImplementationHandle ih in r.GetTypeDefinition(h).GetInterfaceImplementations())
            {
                problem ??= CollectInterfaces(Named(r, r.GetInterfaceImplementation(ih).Interface, c), interfaces, visited);
            }
            return problem;
        }

        /// <summary>
        /// Writes an interface's members and those of every interface it
        /// extends, each under the interface that declares it. Two
        /// interfaces may each declare a member of one signature, and a
        /// class implements each one, so every interface is read alone.
        /// </summary>
        private string? WalkInterface(Utf8JsonWriter w, MetadataReader r, TypeDefinitionHandle h, Context c, string from, List<string> interfaces, HashSet<string> visited)
        {
            if (!visited.Add(from)) return null;
            TypeDefinition d = r.GetTypeDefinition(h);
            WriteMembers(w, r, d, c, from, true, new HashSet<string>(StringComparer.Ordinal), new Dictionary<string, PropertySeen>(StringComparer.Ordinal), new HashSet<string>(StringComparer.Ordinal));
            string? problem = null;
            foreach (InterfaceImplementationHandle ih in d.GetInterfaceImplementations())
            {
                TypeName i = Named(r, r.GetInterfaceImplementation(ih).Interface, c);
                if (!interfaces.Contains(i.Text)) interfaces.Add(i.Text);
                var found = Resolve(i);
                if (found is null)
                {
                    problem ??= $"the interface {i.Text} it extends is in none of the reference assemblies, so the members it declares are unknown";
                    continue;
                }
                var (ri, hi, _) = found.Value;
                problem ??= WalkInterface(w, ri, hi, new Context { Args = i.Args }, i.Text, interfaces, visited);
            }
            return problem;
        }

        private sealed class PropertySeen
        {
            public bool Get;
            public bool Set;
        }

        private void WriteParams(Utf8JsonWriter w, MetadataReader r, MethodDefinition m, MethodSignature<TypeName> sig)
        {
            var names = new string[sig.ParameterTypes.Length];
            var attrs = new ParameterAttributes[sig.ParameterTypes.Length];
            foreach (ParameterHandle ph in m.GetParameters())
            {
                Parameter p = r.GetParameter(ph);
                int at = p.SequenceNumber - 1;
                if (at < 0 || at >= names.Length) continue;
                names[at] = r.GetString(p.Name);
                attrs[at] = p.Attributes;
            }
            w.WriteStartArray("params");
            for (int i = 0; i < sig.ParameterTypes.Length; i++)
            {
                TypeName t = sig.ParameterTypes[i];
                w.WriteStartObject();
                w.WriteString("name", names[i] ?? $"arg{i}");
                WriteType(w, "type", t);
                string mode = !t.ByRef ? "" : (attrs[i] & ParameterAttributes.Out) != 0 && (attrs[i] & ParameterAttributes.In) == 0 ? "out" : t.InMod ? "in" : "ref";
                w.WriteString("mode", mode);
                w.WriteBoolean("optional", (attrs[i] & ParameterAttributes.Optional) != 0 || (attrs[i] & ParameterAttributes.HasDefault) != 0);
                w.WriteEndObject();
            }
            w.WriteEndArray();
        }

        private static void WriteType(Utf8JsonWriter w, string key, TypeName t)
        {
            w.WriteString(key, t.Text);
            w.WriteBoolean(key + "_value", t.ValueType);
            if (t.Unsupported || t.MethodGeneric) w.WriteBoolean(key + "_unsupported", true);
        }

        private static string SignatureKey(string name, MethodSignature<TypeName> sig)
        {
            return name + "(" + string.Join(",", sig.ParameterTypes.Select(p => (p.ByRef ? "&" : "") + p.Text)) + ")`" + sig.GenericParameterCount;
        }

        /// <summary>
        /// Writes the members one type declares that its derived types reach:
        /// public and protected ones, each signature once, the most derived
        /// declaration of it first.
        /// </summary>
        private void WriteMembers(Utf8JsonWriter w, MetadataReader r, TypeDefinition d, Context c, string from, bool inInterface,
            HashSet<string> seenMethods, Dictionary<string, PropertySeen> seenProperties, HashSet<string> seenEvents)
        {
            var accessors = new HashSet<MethodDefinitionHandle>();
            foreach (PropertyDefinitionHandle ph in d.GetProperties())
            {
                PropertyDefinition p = r.GetPropertyDefinition(ph);
                PropertyAccessors a = p.GetAccessors();
                if (!a.Getter.IsNil) accessors.Add(a.Getter);
                if (!a.Setter.IsNil) accessors.Add(a.Setter);
                MethodDefinition? getter = a.Getter.IsNil ? null : r.GetMethodDefinition(a.Getter);
                MethodDefinition? setter = a.Setter.IsNil ? null : r.GetMethodDefinition(a.Setter);
                string? getAccess = getter is null ? null : Access(getter.Value.Attributes);
                string? setAccess = setter is null ? null : Access(setter.Value.Attributes);
                if (getAccess is null && setAccess is null) continue;
                MethodSignature<TypeName> sig = p.DecodeSignature(_provider, c);
                string name = r.GetString(p.Name);
                string key = name + "[" + string.Join(",", sig.ParameterTypes.Select(t => t.Text)) + "]";
                bool writeGet = getAccess != null;
                bool writeSet = setAccess != null;
                if (seenProperties.TryGetValue(key, out PropertySeen? seen))
                {
                    writeGet = writeGet && !seen.Get;
                    writeSet = writeSet && !seen.Set;
                    if (!writeGet && !writeSet) continue;
                }
                else
                {
                    seen = new PropertySeen();
                    seenProperties[key] = seen;
                }
                seen.Get |= writeGet;
                seen.Set |= writeSet;
                w.WriteStartObject();
                w.WriteString("kind", "property");
                w.WriteString("name", name);
                w.WriteString("from", from);
                WriteType(w, "type", sig.ReturnType);
                MethodDefinition source = getter ?? setter!.Value;
                MethodSignature<TypeName> sourceSig = source.DecodeSignature(_provider, c);
                WriteParams(w, r, source, new MethodSignature<TypeName>(sourceSig.Header, sourceSig.ReturnType, sourceSig.RequiredParameterCount, sourceSig.GenericParameterCount,
                    getter is null ? sourceSig.ParameterTypes.RemoveAt(sourceSig.ParameterTypes.Length - 1) : sourceSig.ParameterTypes));
                WriteAccessor(w, "get", writeGet ? getter : null, getAccess, inInterface, c);
                WriteAccessor(w, "set", writeSet ? setter : null, setAccess, inInterface, c);
                w.WriteEndObject();
            }
            foreach (EventDefinitionHandle eh in d.GetEvents())
            {
                EventDefinition e = r.GetEventDefinition(eh);
                EventAccessors a = e.GetAccessors();
                if (!a.Adder.IsNil) accessors.Add(a.Adder);
                if (!a.Remover.IsNil) accessors.Add(a.Remover);
                if (!a.Raiser.IsNil) accessors.Add(a.Raiser);
                if (a.Adder.IsNil) continue;
                MethodDefinition adder = r.GetMethodDefinition(a.Adder);
                string? access = Access(adder.Attributes);
                string name = r.GetString(e.Name);
                if (access is null || !seenEvents.Add(name)) continue;
                w.WriteStartObject();
                w.WriteString("kind", "event");
                w.WriteString("name", name);
                w.WriteString("from", from);
                w.WriteString("access", access);
                w.WriteString("slot", Slot(adder.Attributes, inInterface));
                w.WriteEndObject();
            }
            foreach (MethodDefinitionHandle mh in d.GetMethods())
            {
                if (accessors.Contains(mh)) continue;
                MethodDefinition m = r.GetMethodDefinition(mh);
                string name = r.GetString(m.Name);
                if (name == ".ctor" || name == ".cctor") continue;
                string? access = Access(m.Attributes);
                if (access is null) continue;
                MethodSignature<TypeName> sig = m.DecodeSignature(_provider, c);
                if (!seenMethods.Add(SignatureKey(name, sig))) continue;
                w.WriteStartObject();
                w.WriteString("kind", (m.Attributes & MethodAttributes.SpecialName) != 0 && name.StartsWith("op_", StringComparison.Ordinal) ? "operator" : "method");
                w.WriteString("name", name);
                w.WriteString("from", from);
                w.WriteString("access", access);
                w.WriteString("slot", Slot(m.Attributes, inInterface));
                w.WriteNumber("generic", sig.GenericParameterCount);
                WriteParams(w, r, m, sig);
                WriteType(w, "returns", sig.ReturnType);
                if (sig.ReturnType.ByRef) w.WriteBoolean("returns_ref", true);
                w.WriteEndObject();
            }
            foreach (FieldDefinitionHandle fh in d.GetFields())
            {
                FieldDefinition f = r.GetFieldDefinition(fh);
                FieldAttributes fa = f.Attributes & FieldAttributes.FieldAccessMask;
                if (fa != FieldAttributes.Public && fa != FieldAttributes.Family && fa != FieldAttributes.FamORAssem) continue;
                w.WriteStartObject();
                w.WriteString("kind", "field");
                w.WriteString("name", r.GetString(f.Name));
                w.WriteString("from", from);
                w.WriteEndObject();
            }
            foreach (TypeDefinitionHandle nh in d.GetNestedTypes())
            {
                TypeDefinition n = r.GetTypeDefinition(nh);
                TypeAttributes v = n.Attributes & TypeAttributes.VisibilityMask;
                if (v != TypeAttributes.NestedPublic && v != TypeAttributes.NestedFamily && v != TypeAttributes.NestedFamORAssem) continue;
                w.WriteStartObject();
                w.WriteString("kind", "type");
                w.WriteString("name", StripArity(r.GetString(n.Name)));
                w.WriteString("from", from);
                w.WriteEndObject();
            }
        }

        private void WriteAccessor(Utf8JsonWriter w, string key, MethodDefinition? m, string? access, bool inInterface, Context c)
        {
            if (m is null || access is null)
            {
                w.WriteNull(key);
                return;
            }
            w.WriteStartObject(key);
            w.WriteString("access", access);
            w.WriteString("slot", Slot(m.Value.Attributes, inInterface));
            if (key == "set" && m.Value.DecodeSignature(_provider, c).ReturnType.InitMod) w.WriteBoolean("init", true);
            w.WriteEndObject();
        }
    }
}
'@
}

[Pwrs.Build.Members]::Run($Request, $Output)

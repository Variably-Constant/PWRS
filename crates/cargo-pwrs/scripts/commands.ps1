#Requires -Version 7
# Reads the cmdlets a module wraps, from the assemblies of each source the
# request names, and writes what a wrapper's shell needs of each as JSON:
# its class, whether it implements IDynamicParameters, its [Cmdlet] and
# [OutputType] attributes, and every parameter of its class and its
# bases with the parameter's type and every attribute, each argument
# typed. A type is written with its C# text, the name the runtime looks
# it up by, whether it is public, and whether the compile set the source
# names declares it public, so the build can tell what a shell can name.
#
#   {"compile": [{"name": "net10.0", "references": ["...dll", ...]}],
#    "sources": [{"name": "7.4.22/unix", "compile": "net10.0",
#                 "assemblies": ["...dll", ...], "context": ["...dll", ...]}],
#    "raw": [{"name": "kept/core", "compile": "net10.0", "context": ["...dll", ...],
#             "commands": {"Some.Module\\Get-Thing": {...}}}],
#    "commands": ["Get-Process", ...]}
#
# A source's assemblies are searched for the commands; its context only
# resolves the types they name. A raw source has declarations
# installed.ps1 read from functions, by any name the request chooses,
# whose types are completed against its context the same way.
param(
    [Parameter(Mandatory)] [string] $Request,
    [Parameter(Mandatory)] [string] $Output
)

$ErrorActionPreference = 'Stop'

if (-not ('Pwrs.Build.Commands.Reader' -as [type])) {
    Add-Type -TypeDefinition @'
#nullable enable
using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text.Json;

namespace Pwrs.Build.Commands
{
    /// <summary>A type as a declaration names it.</summary>
    public sealed class TypeRef
    {
        /// <summary>
        /// The CLR name without the assembly: namespace, outer types joined
        /// by '+', and an instantiation's arguments in brackets.
        /// </summary>
        public string Name = "";
        /// <summary>The C# text, every name rooted at global::.</summary>
        public string CSharp = "";
        /// <summary>The defining assembly's simple name; empty for a primitive.</summary>
        public string Assembly = "";
        /// <summary>A definition was found for it.</summary>
        public bool Known;
        /// <summary>Public, and nested only in public types.</summary>
        public bool Visible;
        /// <summary>The source's compile set declares it public, every part of it.</summary>
        public bool Compile;
        /// <summary>A pointer, a by-reference or a type parameter, which no declaration copies.</summary>
        public bool Unsupported;
        /// <summary>A value type: a primitive but string and object, an enum, a struct or a nullable one.</summary>
        public bool ValueType;
        public bool Enum;
        public PrimitiveTypeCode? Underlying;
        /// <summary>An array's element type.</summary>
        public TypeRef? Element;
        /// <summary>The name Type.GetType resolves, with its assembly where it has one.</summary>
        public string? Runtime;
    }

    /// <summary>Names of types as metadata spells them.</summary>
    public static class Names
    {
        public static string Full(MetadataReader r, TypeDefinitionHandle h)
        {
            TypeDefinition d = r.GetTypeDefinition(h);
            string name = r.GetString(d.Name);
            TypeDefinitionHandle outer = d.GetDeclaringType();
            if (!outer.IsNil) return Full(r, outer) + "+" + name;
            string ns = r.GetString(d.Namespace);
            return ns.Length == 0 ? name : ns + "." + name;
        }

        public static string Full(MetadataReader r, TypeReferenceHandle h)
        {
            TypeReference t = r.GetTypeReference(h);
            string name = r.GetString(t.Name);
            if (t.ResolutionScope.Kind == HandleKind.TypeReference) return Full(r, (TypeReferenceHandle)t.ResolutionScope) + "+" + name;
            string ns = r.GetString(t.Namespace);
            return ns.Length == 0 ? name : ns + "." + name;
        }

        /// <summary>The assembly a type reference resolves in.</summary>
        public static string Scope(MetadataReader r, TypeReferenceHandle h)
        {
            TypeReference t = r.GetTypeReference(h);
            switch (t.ResolutionScope.Kind)
            {
                case HandleKind.AssemblyReference:
                    return r.GetString(r.GetAssemblyReference((AssemblyReferenceHandle)t.ResolutionScope).Name);
                case HandleKind.TypeReference:
                    return Scope(r, (TypeReferenceHandle)t.ResolutionScope);
                default:
                    return r.GetString(r.GetAssemblyDefinition().Name);
            }
        }

        /// <summary>The C# text of a CLR name: rooted, '.' for '+', without generic arity.</summary>
        public static string CSharp(string clr)
        {
            var parts = clr.Split('.', '+').Select(p => { int tick = p.IndexOf('`'); return tick < 0 ? p : p.Substring(0, tick); });
            return "global::" + string.Join(".", parts);
        }

        public static bool IsVisible(MetadataReader r, TypeDefinition d)
        {
            TypeAttributes v = d.Attributes & TypeAttributes.VisibilityMask;
            if (v == TypeAttributes.Public) return true;
            if (v != TypeAttributes.NestedPublic) return false;
            return IsVisible(r, r.GetTypeDefinition(d.GetDeclaringType()));
        }

        /// <summary>The name of a definition's base type, or null.</summary>
        public static string? BaseName(MetadataReader r, TypeDefinition d)
        {
            EntityHandle b = d.BaseType;
            if (b.IsNil) return null;
            if (b.Kind == HandleKind.TypeDefinition) return Full(r, (TypeDefinitionHandle)b);
            if (b.Kind == HandleKind.TypeReference) return Full(r, (TypeReferenceHandle)b);
            return null;
        }

        /// <summary>The name of the type an attribute's constructor belongs to.</summary>
        public static string AttributeType(MetadataReader r, CustomAttribute a, out string assembly)
        {
            assembly = r.GetString(r.GetAssemblyDefinition().Name);
            if (a.Constructor.Kind == HandleKind.MemberReference)
            {
                EntityHandle parent = r.GetMemberReference((MemberReferenceHandle)a.Constructor).Parent;
                if (parent.Kind == HandleKind.TypeReference)
                {
                    assembly = Scope(r, (TypeReferenceHandle)parent);
                    return Full(r, (TypeReferenceHandle)parent);
                }
                if (parent.Kind == HandleKind.TypeDefinition) return Full(r, (TypeDefinitionHandle)parent);
                return "";
            }
            return Full(r, r.GetMethodDefinition((MethodDefinitionHandle)a.Constructor).GetDeclaringType());
        }
    }

    /// <summary>Every type a set of assemblies defines, by CLR name, the first definition of a name winning.</summary>
    public sealed class TypeIndex
    {
        public readonly Dictionary<string, (MetadataReader Reader, TypeDefinitionHandle Handle, string Assembly)> Types =
            new Dictionary<string, (MetadataReader, TypeDefinitionHandle, string)>(StringComparer.Ordinal);

        public void Add(MetadataReader r)
        {
            string assembly = r.GetString(r.GetAssemblyDefinition().Name);
            foreach (TypeDefinitionHandle h in r.TypeDefinitions)
            {
                string name = Names.Full(r, h);
                if (!Types.ContainsKey(name)) Types[name] = (r, h, assembly);
            }
        }
    }

    /// <summary>Builds the TypeRef of every type one source's signatures and attributes name.</summary>
    public sealed class Provider : ISignatureTypeProvider<TypeRef, object?>, ICustomAttributeTypeProvider<TypeRef>
    {
        private readonly TypeIndex _index;
        private readonly HashSet<string> _compile;

        public Provider(TypeIndex index, HashSet<string> compile)
        {
            _index = index;
            _compile = compile;
        }

        public TypeRef Named(string name, string assemblyHint)
        {
            var t = new TypeRef { Name = name, CSharp = Names.CSharp(name), Assembly = assemblyHint };
            if (_index.Types.TryGetValue(name, out var def))
            {
                TypeDefinition d = def.Reader.GetTypeDefinition(def.Handle);
                t.Known = true;
                t.Assembly = def.Assembly;
                t.Visible = Names.IsVisible(def.Reader, d);
                string? baseName = Names.BaseName(def.Reader, d);
                t.ValueType = (baseName == "System.ValueType" || baseName == "System.Enum") && name != "System.Enum";
                if (baseName == "System.Enum")
                {
                    t.Enum = true;
                    t.Underlying = Underlying(def.Reader, d);
                }
            }
            t.Compile = t.Visible && _compile.Contains(name);
            t.Runtime = t.Assembly.Length == 0 ? name : name + ", " + t.Assembly;
            return t;
        }

        /// <summary>An enum's underlying type, from the signature of its value__ field.</summary>
        private static PrimitiveTypeCode? Underlying(MetadataReader r, TypeDefinition d)
        {
            foreach (FieldDefinitionHandle fh in d.GetFields())
            {
                FieldDefinition f = r.GetFieldDefinition(fh);
                if ((f.Attributes & FieldAttributes.Static) != 0) continue;
                BlobReader blob = r.GetBlobReader(f.Signature);
                blob.ReadSignatureHeader();
                return (PrimitiveTypeCode)blob.ReadSignatureTypeCode();
            }
            return null;
        }

        private static TypeRef Unsupported(string name) =>
            new TypeRef { Name = name, CSharp = name, Unsupported = true };

        public TypeRef GetPrimitiveType(PrimitiveTypeCode code)
        {
            string keyword;
            switch (code)
            {
                case PrimitiveTypeCode.Boolean: keyword = "bool"; break;
                case PrimitiveTypeCode.Char: keyword = "char"; break;
                case PrimitiveTypeCode.SByte: keyword = "sbyte"; break;
                case PrimitiveTypeCode.Byte: keyword = "byte"; break;
                case PrimitiveTypeCode.Int16: keyword = "short"; break;
                case PrimitiveTypeCode.UInt16: keyword = "ushort"; break;
                case PrimitiveTypeCode.Int32: keyword = "int"; break;
                case PrimitiveTypeCode.UInt32: keyword = "uint"; break;
                case PrimitiveTypeCode.Int64: keyword = "long"; break;
                case PrimitiveTypeCode.UInt64: keyword = "ulong"; break;
                case PrimitiveTypeCode.Single: keyword = "float"; break;
                case PrimitiveTypeCode.Double: keyword = "double"; break;
                case PrimitiveTypeCode.String: keyword = "string"; break;
                case PrimitiveTypeCode.Object: keyword = "object"; break;
                case PrimitiveTypeCode.IntPtr: keyword = "global::System.IntPtr"; break;
                case PrimitiveTypeCode.UIntPtr: keyword = "global::System.UIntPtr"; break;
                default: return Unsupported("System." + code);
            }
            string name = "System." + code;
            bool value = code != PrimitiveTypeCode.String && code != PrimitiveTypeCode.Object;
            return new TypeRef { Name = name, CSharp = keyword, Known = true, Visible = true, Compile = true, ValueType = value, Runtime = name };
        }

        public TypeRef GetTypeFromDefinition(MetadataReader r, TypeDefinitionHandle h, byte rawTypeKind) =>
            Named(Names.Full(r, h), r.GetString(r.GetAssemblyDefinition().Name));

        public TypeRef GetTypeFromReference(MetadataReader r, TypeReferenceHandle h, byte rawTypeKind) =>
            Named(Names.Full(r, h), Names.Scope(r, h));

        public TypeRef GetTypeFromSpecification(MetadataReader r, object? context, TypeSpecificationHandle h, byte rawTypeKind) =>
            r.GetTypeSpecification(h).DecodeSignature(this, context);

        public TypeRef GetSZArrayType(TypeRef element) => new TypeRef
        {
            Name = element.Name + "[]",
            CSharp = element.CSharp + "[]",
            Assembly = element.Assembly,
            Known = element.Known,
            Visible = element.Visible,
            Compile = element.Compile,
            Unsupported = element.Unsupported,
            Element = element,
            Runtime = element.Runtime == null ? null : InsertBeforeAssembly(element, "[]"),
        };

        /// <summary>The runtime name of a type built on <paramref name="t"/>, the suffix before its assembly.</summary>
        private static string InsertBeforeAssembly(TypeRef t, string suffix) =>
            t.Assembly.Length == 0 ? t.Name + suffix : t.Name + suffix + ", " + t.Assembly;

        public TypeRef GetGenericInstantiation(TypeRef generic, ImmutableArray<TypeRef> arguments)
        {
            string csharp = generic.CSharp + "<" + string.Join(", ", arguments.Select(a => a.CSharp)) + ">";
            string name = generic.Name + "[" + string.Join(",", arguments.Select(a => "[" + a.Name + (a.Assembly.Length == 0 ? "" : ", " + a.Assembly) + "]")) + "]";
            bool runtime = generic.Runtime != null && arguments.All(a => a.Runtime != null);
            return new TypeRef
            {
                Name = name,
                CSharp = csharp,
                Assembly = generic.Assembly,
                Known = generic.Known && arguments.All(a => a.Known),
                Visible = generic.Visible && arguments.All(a => a.Visible),
                Compile = generic.Compile && arguments.All(a => a.Compile),
                Unsupported = generic.Unsupported || arguments.Any(a => a.Unsupported),
                ValueType = generic.ValueType,
                Runtime = runtime ? (generic.Assembly.Length == 0 ? name : name + ", " + generic.Assembly) : null,
            };
        }

        public TypeRef GetArrayType(TypeRef element, ArrayShape shape) => Unsupported(element.Name + "[" + new string(',', shape.Rank - 1) + "]");
        public TypeRef GetByReferenceType(TypeRef element) => Unsupported(element.Name + "&");
        public TypeRef GetPointerType(TypeRef element) => Unsupported(element.Name + "*");
        public TypeRef GetPinnedType(TypeRef element) => element;
        public TypeRef GetFunctionPointerType(MethodSignature<TypeRef> signature) => Unsupported("delegate*");
        public TypeRef GetGenericMethodParameter(object? context, int index) => Unsupported("!!" + index);
        public TypeRef GetGenericTypeParameter(object? context, int index) => Unsupported("!" + index);
        public TypeRef GetModifiedType(TypeRef modifier, TypeRef unmodified, bool isRequired) => unmodified;

        public TypeRef GetSystemType() => Named("System.Type", "");
        public bool IsSystemType(TypeRef type) => type.Name == "System.Type";

        public TypeRef GetTypeFromSerializedName(string name) => Serialized(name);

        public PrimitiveTypeCode GetUnderlyingEnumType(TypeRef type) =>
            type.Underlying ?? throw new BadImageFormatException("the underlying type of " + type.Name + " is not in the assemblies read");

        /// <summary>
        /// The type a serialized name names, as a System.Type argument or a
        /// boxed enum is written: "Namespace.Name[+Nested][[]], Assembly,
        /// Version=..., ...". A generic instantiation is not resolved.
        /// </summary>
        public TypeRef Serialized(string serialized)
        {
            int depth = 0;
            int comma = -1;
            for (int i = 0; i < serialized.Length && comma < 0; i++)
            {
                char c = serialized[i];
                if (c == '[') depth++;
                else if (c == ']') depth--;
                else if (c == ',' && depth == 0) comma = i;
            }
            string name = (comma < 0 ? serialized : serialized.Substring(0, comma)).Trim();
            string assembly = comma < 0 ? "" : serialized.Substring(comma + 1).Split(',')[0].Trim();
            if (name.EndsWith("[]", StringComparison.Ordinal)) return GetSZArrayType(Serialized(name.Substring(0, name.Length - 2) + (assembly.Length == 0 ? "" : ", " + assembly)));
            if (name.Contains('[')) return Unsupported(name);
            return Named(name, assembly);
        }
    }

    /// <summary>One source: its assemblies searched for cmdlets, and what resolves the types they name.</summary>
    public sealed class Source
    {
        private const string Sma = "System.Management.Automation.";
        private readonly TypeIndex _index;
        private readonly Provider _provider;
        private readonly Dictionary<string, (MetadataReader Reader, TypeDefinitionHandle Handle)> _cmdlets =
            new Dictionary<string, (MetadataReader, TypeDefinitionHandle)>(StringComparer.OrdinalIgnoreCase);

        public Source(TypeIndex index, HashSet<string> compile, IEnumerable<MetadataReader> searched)
        {
            _index = index;
            _provider = new Provider(index, compile);
            foreach (MetadataReader r in searched)
            {
                foreach (TypeDefinitionHandle h in r.TypeDefinitions)
                {
                    foreach (CustomAttributeHandle ah in r.GetTypeDefinition(h).GetCustomAttributes())
                    {
                        CustomAttribute a = r.GetCustomAttribute(ah);
                        if (Names.AttributeType(r, a, out _) != Sma + "CmdletAttribute") continue;
                        CustomAttributeValue<TypeRef> v = a.DecodeValue(_provider);
                        string key = v.FixedArguments[0].Value + "-" + v.FixedArguments[1].Value;
                        if (!_cmdlets.ContainsKey(key)) _cmdlets[key] = (r, h);
                    }
                }
            }
        }

        /// <summary>Writes the command's declaration, or null when no class here declares it.</summary>
        public void Describe(Utf8JsonWriter w, string command)
        {
            if (!_cmdlets.TryGetValue(command, out var at))
            {
                w.WriteNullValue();
                return;
            }
            MetadataReader r = at.Reader;
            TypeDefinition cls = r.GetTypeDefinition(at.Handle);
            w.WriteStartObject();
            w.WriteString("class", Names.Full(r, at.Handle));
            w.WriteString("assembly", r.GetString(r.GetAssemblyDefinition().Name));
            w.WriteStartArray("cmdlet");
            WriteAttributes(w, r, cls.GetCustomAttributes(), Sma + "CmdletAttribute");
            w.WriteEndArray();
            w.WriteStartArray("output");
            WriteAttributes(w, r, cls.GetCustomAttributes(), Sma + "OutputTypeAttribute");
            w.WriteEndArray();
            bool dynamic = false;
            var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            w.WriteStartArray("parameters");
            MetadataReader cr = r;
            TypeDefinitionHandle ch = at.Handle;
            while (true)
            {
                TypeDefinition d = cr.GetTypeDefinition(ch);
                foreach (InterfaceImplementationHandle ih in d.GetInterfaceImplementations())
                {
                    EntityHandle i = cr.GetInterfaceImplementation(ih).Interface;
                    string? iname = i.Kind == HandleKind.TypeReference ? Names.Full(cr, (TypeReferenceHandle)i)
                        : i.Kind == HandleKind.TypeDefinition ? Names.Full(cr, (TypeDefinitionHandle)i) : null;
                    if (iname == Sma + "IDynamicParameters") dynamic = true;
                }
                foreach (PropertyDefinitionHandle ph in d.GetProperties())
                {
                    PropertyDefinition p = cr.GetPropertyDefinition(ph);
                    CustomAttributeHandleCollection attrs = p.GetCustomAttributes();
                    if (!attrs.Any(ah => Names.AttributeType(cr, cr.GetCustomAttribute(ah), out _) == Sma + "ParameterAttribute")) continue;
                    string name = cr.GetString(p.Name);
                    if (!seen.Add(name)) continue;
                    w.WriteStartObject();
                    w.WriteString("name", name);
                    WriteType(w, "type", p.DecodeSignature(_provider, null).ReturnType);
                    w.WriteStartArray("attributes");
                    WriteAttributes(w, cr, attrs, null);
                    w.WriteEndArray();
                    w.WriteEndObject();
                }
                string? next = Names.BaseName(cr, d);
                if (next == null || !_index.Types.TryGetValue(next, out var b)) break;
                cr = b.Reader;
                ch = b.Handle;
            }
            w.WriteEndArray();
            w.WriteBoolean("dynamic", dynamic);
            w.WriteEndObject();
        }

        /// <summary>Writes each attribute, or only those of the type <paramref name="only"/>.</summary>
        private void WriteAttributes(Utf8JsonWriter w, MetadataReader r, CustomAttributeHandleCollection handles, string? only)
        {
            foreach (CustomAttributeHandle ah in handles)
            {
                CustomAttribute a = r.GetCustomAttribute(ah);
                string type = Names.AttributeType(r, a, out string assembly);
                if (only != null && type != only) continue;
                w.WriteStartObject();
                WriteType(w, "type", _provider.Named(type, assembly));
                try
                {
                    CustomAttributeValue<TypeRef> v = a.DecodeValue(_provider);
                    w.WriteStartArray("args");
                    foreach (CustomAttributeTypedArgument<TypeRef> arg in v.FixedArguments) WriteValue(w, arg.Type, arg.Value);
                    w.WriteEndArray();
                    w.WriteStartArray("named");
                    foreach (CustomAttributeNamedArgument<TypeRef> arg in v.NamedArguments)
                    {
                        w.WriteStartObject();
                        w.WriteString("name", arg.Name);
                        w.WritePropertyName("value");
                        WriteValue(w, arg.Type, arg.Value);
                        w.WriteEndObject();
                    }
                    w.WriteEndArray();
                }
                catch (Exception e) when (e is BadImageFormatException || e is InvalidOperationException || e is ArgumentException)
                {
                    w.WriteString("error", e.Message);
                }
                w.WriteEndObject();
            }
        }

        private void WriteValue(Utf8JsonWriter w, TypeRef type, object? value)
        {
            w.WriteStartObject();
            if (value is ImmutableArray<CustomAttributeTypedArgument<TypeRef>> items)
            {
                w.WriteString("kind", "array");
                WriteType(w, "element", type.Element ?? _provider.GetPrimitiveType(PrimitiveTypeCode.Object));
                w.WriteStartArray("items");
                foreach (CustomAttributeTypedArgument<TypeRef> item in items) WriteValue(w, item.Type, item.Value);
                w.WriteEndArray();
            }
            else if (value is null)
            {
                w.WriteString("kind", "null");
                WriteType(w, "type", type);
            }
            else if (value is TypeRef named)
            {
                w.WriteString("kind", "type");
                WriteType(w, "type", named);
            }
            else if (type.Enum)
            {
                w.WriteString("kind", "enum");
                WriteType(w, "type", type);
                w.WriteString("text", Convert.ToString(value, CultureInfo.InvariantCulture));
            }
            else
            {
                switch (value)
                {
                    case bool b:
                        w.WriteString("kind", "bool");
                        w.WriteBoolean("value", b);
                        break;
                    case string s:
                        w.WriteString("kind", "string");
                        w.WriteString("value", s);
                        break;
                    case char c:
                        w.WriteString("kind", "char");
                        w.WriteString("value", c.ToString());
                        break;
                    case float f:
                        w.WriteString("kind", "number");
                        w.WriteString("type", type.Name);
                        w.WriteString("text", f.ToString("R", CultureInfo.InvariantCulture));
                        break;
                    case double d:
                        w.WriteString("kind", "number");
                        w.WriteString("type", type.Name);
                        w.WriteString("text", d.ToString("R", CultureInfo.InvariantCulture));
                        break;
                    default:
                        w.WriteString("kind", "number");
                        w.WriteString("type", type.Name);
                        w.WriteString("text", Convert.ToString(value, CultureInfo.InvariantCulture));
                        break;
                }
            }
            w.WriteEndObject();
        }

        internal static void WriteType(Utf8JsonWriter w, string key, TypeRef t)
        {
            w.WriteStartObject(key);
            w.WriteString("name", t.Name);
            w.WriteString("csharp", t.CSharp);
            w.WriteString("assembly", t.Assembly);
            if (t.Runtime != null) w.WriteString("runtime", t.Runtime);
            w.WriteBoolean("known", t.Known);
            w.WriteBoolean("visible", t.Visible);
            w.WriteBoolean("compile", t.Compile);
            if (t.Unsupported) w.WriteBoolean("unsupported", true);
            if (t.ValueType) w.WriteBoolean("valuetype", true);
            if (t.Enum) w.WriteBoolean("enum", true);
            w.WriteEndObject();
        }
    }

    /// <summary>
    /// Completes a declaration installed.ps1 read from a function, whose
    /// types are named by full name and assembly alone, into what Source
    /// writes for a cmdlet: each type as the source's index and compile set
    /// describe it.
    /// </summary>
    public static class Completer
    {
        private static TypeRef Type(Provider p, string name, string assembly) =>
            name.EndsWith("[]", StringComparison.Ordinal) ? p.GetSZArrayType(Type(p, name.Substring(0, name.Length - 2), assembly)) : p.Named(name, assembly);

        private static TypeRef Type(Provider p, JsonElement t) =>
            Type(p, t.GetProperty("name").GetString() ?? "", t.TryGetProperty("assembly", out JsonElement a) ? a.GetString() ?? "" : "");

        public static void Describe(Utf8JsonWriter w, Provider p, JsonElement raw)
        {
            w.WriteStartObject();
            w.WriteString("class", "");
            w.WriteString("assembly", "");
            foreach (string list in new[] { "cmdlet", "output" })
            {
                w.WriteStartArray(list);
                if (raw.TryGetProperty(list, out JsonElement attributes))
                {
                    foreach (JsonElement a in attributes.EnumerateArray()) WriteAttribute(w, p, a);
                }
                w.WriteEndArray();
            }
            w.WriteStartArray("parameters");
            foreach (JsonElement parameter in raw.GetProperty("parameters").EnumerateArray())
            {
                w.WriteStartObject();
                w.WriteString("name", parameter.GetProperty("name").GetString());
                Source.WriteType(w, "type", Type(p, parameter.GetProperty("type")));
                w.WriteStartArray("attributes");
                foreach (JsonElement a in parameter.GetProperty("attributes").EnumerateArray()) WriteAttribute(w, p, a);
                w.WriteEndArray();
                w.WriteEndObject();
            }
            w.WriteEndArray();
            w.WriteBoolean("dynamic", raw.TryGetProperty("dynamic", out JsonElement d) && d.ValueKind == JsonValueKind.True);
            w.WriteEndObject();
        }

        private static void WriteAttribute(Utf8JsonWriter w, Provider p, JsonElement a)
        {
            w.WriteStartObject();
            Source.WriteType(w, "type", Type(p, a.GetProperty("type")));
            if (a.TryGetProperty("error", out JsonElement error))
            {
                w.WriteString("error", error.GetString());
            }
            else
            {
                w.WriteStartArray("args");
                if (a.TryGetProperty("args", out JsonElement args)) foreach (JsonElement v in args.EnumerateArray()) WriteValue(w, p, v);
                w.WriteEndArray();
                w.WriteStartArray("named");
                if (a.TryGetProperty("named", out JsonElement named))
                {
                    foreach (JsonElement n in named.EnumerateArray())
                    {
                        w.WriteStartObject();
                        w.WriteString("name", n.GetProperty("name").GetString());
                        w.WritePropertyName("value");
                        WriteValue(w, p, n.GetProperty("value"));
                        w.WriteEndObject();
                    }
                }
                w.WriteEndArray();
            }
            w.WriteEndObject();
        }

        private static void WriteValue(Utf8JsonWriter w, Provider p, JsonElement v)
        {
            string kind = v.GetProperty("kind").GetString() ?? "";
            w.WriteStartObject();
            w.WriteString("kind", kind);
            switch (kind)
            {
                case "null":
                case "type":
                    Source.WriteType(w, "type", Type(p, v.GetProperty("type")));
                    break;
                case "enum":
                    Source.WriteType(w, "type", Type(p, v.GetProperty("type")));
                    w.WriteString("text", v.GetProperty("text").GetString());
                    break;
                case "array":
                    Source.WriteType(w, "element", Type(p, v.GetProperty("element")));
                    w.WriteStartArray("items");
                    foreach (JsonElement item in v.GetProperty("items").EnumerateArray()) WriteValue(w, p, item);
                    w.WriteEndArray();
                    break;
                case "bool":
                    w.WriteBoolean("value", v.GetProperty("value").GetBoolean());
                    break;
                case "number":
                    w.WriteString("type", v.GetProperty("type").GetString());
                    w.WriteString("text", v.GetProperty("text").GetString());
                    break;
                default:
                    w.WriteString("value", v.GetProperty("value").GetString());
                    break;
            }
            w.WriteEndObject();
        }
    }

    public static class Reader
    {
        /// <summary>Reads the request and writes every command's declaration from every source.</summary>
        public static void Run(string requestPath, string outputPath)
        {
            using JsonDocument request = JsonDocument.Parse(File.ReadAllText(requestPath));
            JsonElement root = request.RootElement;
            string[] commands = root.GetProperty("commands").EnumerateArray().Select(e => e.GetString() ?? "").ToArray();
            var open = new List<PEReader>();
            try
            {
                var compileSets = new Dictionary<string, HashSet<string>>(StringComparer.Ordinal);
                foreach (JsonElement set in root.GetProperty("compile").EnumerateArray())
                {
                    var names = new HashSet<string>(StringComparer.Ordinal);
                    foreach (MetadataReader r in Open(set.GetProperty("references"), open))
                    {
                        foreach (TypeDefinitionHandle h in r.TypeDefinitions)
                        {
                            if (Names.IsVisible(r, r.GetTypeDefinition(h))) names.Add(Names.Full(r, h));
                        }
                    }
                    compileSets[set.GetProperty("name").GetString() ?? ""] = names;
                }
                using var buffer = new MemoryStream();
                using (var w = new Utf8JsonWriter(buffer, new JsonWriterOptions { Indented = true }))
                {
                    w.WriteStartObject();
                    w.WriteStartObject("sources");
                    foreach (JsonElement source in root.GetProperty("sources").EnumerateArray())
                    {
                        string name = source.GetProperty("name").GetString() ?? "";
                        string compile = source.GetProperty("compile").GetString() ?? "";
                        if (!compileSets.TryGetValue(compile, out HashSet<string>? compileSet))
                        {
                            throw new InvalidOperationException("source " + name + " names the compile set " + compile + ", which the request does not have");
                        }
                        var index = new TypeIndex();
                        List<MetadataReader> searched = Open(source.GetProperty("assemblies"), open);
                        foreach (MetadataReader r in searched) index.Add(r);
                        foreach (MetadataReader r in Open(source.GetProperty("context"), open)) index.Add(r);
                        var reader = new Source(index, compileSet, searched);
                        w.WriteStartObject(name);
                        foreach (string command in commands)
                        {
                            w.WritePropertyName(command);
                            reader.Describe(w, command);
                        }
                        w.WriteEndObject();
                    }
                    // A source read in the host from a function, whose types
                    // only need completing against its context.
                    if (root.TryGetProperty("raw", out JsonElement raws))
                    {
                        foreach (JsonElement raw in raws.EnumerateArray())
                        {
                            string name = raw.GetProperty("name").GetString() ?? "";
                            string compile = raw.GetProperty("compile").GetString() ?? "";
                            if (!compileSets.TryGetValue(compile, out HashSet<string>? compileSet))
                            {
                                throw new InvalidOperationException("source " + name + " names the compile set " + compile + ", which the request does not have");
                            }
                            var index = new TypeIndex();
                            foreach (MetadataReader r in Open(raw.GetProperty("context"), open)) index.Add(r);
                            var provider = new Provider(index, compileSet);
                            w.WriteStartObject(name);
                            foreach (JsonProperty command in raw.GetProperty("commands").EnumerateObject())
                            {
                                w.WritePropertyName(command.Name);
                                Completer.Describe(w, provider, command.Value);
                            }
                            w.WriteEndObject();
                        }
                    }
                    w.WriteEndObject();
                    w.WriteEndObject();
                }
                File.WriteAllBytes(outputPath, buffer.ToArray());
            }
            finally
            {
                foreach (PEReader pe in open) pe.Dispose();
            }
        }

        /// <summary>
        /// A reader of each listed file that is an assembly; a native file
        /// or a module beside them is passed over.
        /// </summary>
        private static List<MetadataReader> Open(JsonElement files, List<PEReader> open)
        {
            var readers = new List<MetadataReader>();
            foreach (JsonElement f in files.EnumerateArray())
            {
                var pe = new PEReader(File.OpenRead(f.GetString() ?? ""));
                open.Add(pe);
                if (!pe.HasMetadata) continue;
                MetadataReader r = pe.GetMetadataReader();
                if (r.IsAssembly) readers.Add(r);
            }
            return readers;
        }
    }
}
'@
}

[Pwrs.Build.Commands.Reader]::Run($Request, $Output)

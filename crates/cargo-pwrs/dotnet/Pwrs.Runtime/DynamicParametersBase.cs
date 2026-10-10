using System;
using System.Collections;
using System.Runtime.InteropServices;
using System.Management.Automation;

namespace Pwrs
{
    /// <summary>
    /// Helper the generated cmdlet uses from GetDynamicParameters. It
    /// hands the native module the table of bound parameters the
    /// generated cmdlet built, which the Rust hook reads as its
    /// PsHashtable and branches on, and turns the packed answer into a
    /// RuntimeDefinedParameterDictionary. The answer is one string: a
    /// line per parameter joined by U+0003, each of seven cells joined
    /// by U+0001 (name, CLR type, mandatory, position, set, help,
    /// validate-set values joined by U+0002).
    /// </summary>
    public static class DynamicParametersBuilder
    {
        private const char LineSeparator = '\u0003';
        private const char CellSeparator = '\u0001';
        private const char ValueSeparator = '\u0002';

        /// <summary>
        /// The value a bound parameter holds, out of the PSObject the
        /// binder may have wrapped it in. The generated cmdlet puts every
        /// value through this as it builds the table.
        /// </summary>
        public static object? Bare(object? value) => value is PSObject wrapped ? wrapped.BaseObject : value;

        /// <summary>
        /// Null when the hook adds nothing: the engine reads that as no
        /// dynamic parameters and prices it below an empty dictionary.
        /// A line without its seven cells is an error naming the count.
        /// </summary>
        public static RuntimeDefinedParameterDictionary? Build(NativeModule module, uint cmdletId, IDictionary bound)
        {
            GCHandle handle = GCHandle.Alloc(bound);
            string? packed;
            try
            {
                packed = module.DynamicParameters(cmdletId, GCHandle.ToIntPtr(handle));
            }
            finally
            {
                handle.Free();
            }
            if (string.IsNullOrEmpty(packed)) return null;
            var dict = new RuntimeDefinedParameterDictionary();
            foreach (string line in packed!.Split(LineSeparator))
            {
                string[] c = line.Split(CellSeparator);
                if (c.Length != 7)
                {
                    throw new PwrsException($"pwrs: a dynamic parameter line holds {c.Length} cells, not 7");
                }
                Type type = Type.GetType(c[1], throwOnError: false) ?? typeof(object);
                var attrs = new System.Collections.ObjectModel.Collection<Attribute>();
                var p = new ParameterAttribute { Mandatory = c[2] == "1" };
                if (int.TryParse(c[3], out int pos) && pos >= 0) p.Position = pos;
                if (!string.IsNullOrEmpty(c[4])) p.ParameterSetName = c[4];
                if (!string.IsNullOrEmpty(c[5])) p.HelpMessage = c[5];
                attrs.Add(p);
                if (!string.IsNullOrEmpty(c[6]))
                {
                    attrs.Add(new ValidateSetAttribute(c[6].Split(ValueSeparator)));
                }
                dict.Add(c[0], new RuntimeDefinedParameter(c[0], type, attrs));
            }
            return dict;
        }
    }
}

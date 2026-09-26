using System;
using System.Collections.Generic;

namespace Pwrs
{
    /// <summary>
    /// One module's generated-class factories, keyed by class id. A
    /// class id is the class's position in its own module's
    /// export_module! list, so every module numbers its classes from 0
    /// and a table serves one module alone.
    ///
    /// The module's generated shell is the only writer. It registers
    /// one delegate per copied class, proxy class and enum from its
    /// type initializer, which the runtime finishes before any other
    /// thread can use the shell, and so before factory_new can reach the
    /// table. After that the table is only read, and a Dictionary is
    /// safe for concurrent readers once nothing modifies it. Register
    /// is public for the generated shell alone; a call from anywhere
    /// else would break that.
    ///
    /// On .NET the loader gives each shell a load context of its own,
    /// holding its own copy of this assembly, and that copy's
    /// factory_new entry reads <see cref="OfThisCopy"/>. On .NET
    /// Framework one copy serves every module in the process, so each
    /// <see cref="NativeModule"/> owns a table and hands its native
    /// library a host table whose factory_new reads that one.
    /// </summary>
    public sealed class Factories
    {
#if NET
        /// <summary>The table of the one shell this copy of the assembly serves.</summary>
        internal static readonly Factories OfThisCopy = new Factories();
#endif

        private readonly Dictionary<uint, Func<IntPtr, object>> _byId = new Dictionary<uint, Func<IntPtr, object>>();

        /// <summary>Records the factory for one class id of this module.</summary>
        public void Register(uint classId, Func<IntPtr, object> factory) => _byId[classId] = factory;

        internal object Create(uint classId, IntPtr fields)
        {
            _byId.TryGetValue(classId, out Func<IntPtr, object>? f);
            if (f == null) throw new KeyNotFoundException($"pwrs: no factory registered for class id {classId}");
            return f(fields);
        }
    }
}

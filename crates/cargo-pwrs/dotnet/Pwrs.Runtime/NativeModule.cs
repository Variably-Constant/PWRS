using System;
using System.Collections;
using System.IO;
using System.Runtime.InteropServices;

namespace Pwrs
{
    /// <summary>
    /// One loaded native module: resolves the library for the current
    /// runtime identifier, binds the exports, and hands the module's
    /// host table across at init. On .NET the exports are called through
    /// unmanaged function pointers (a direct calli with the GC transition
    /// and no marshalling stub); on .NET Framework through delegates.
    /// </summary>
    public sealed unsafe class NativeModule
    {
#if !NET
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int ModuleInitFn(IntPtr vtable);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int CmdletCreateFn(uint cmdletId, IntPtr* instance, uint* phases, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int CmdletInvokeFn(IntPtr instance, uint phase, IntPtr cmdlet, IntPtr parameters, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void CmdletStopFn(IntPtr instance);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int ProxyGetFn(uint classId, uint fieldId, IntPtr instance, IntPtr* result, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int ProxyCallFn(uint classId, uint methodId, IntPtr instance, IntPtr args, IntPtr* result, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void ProxyDropFn(uint classId, IntPtr instance);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate ulong ProxyBytesFn(uint classId, IntPtr instance);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int CompleterInvokeFn(uint completerId, PsStr16 word, PsStr16 command, IntPtr fakeBound, IntPtr* result, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int TransformInvokeFn(uint transformId, IntPtr value, IntPtr* result, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int DynParamsInvokeFn(uint cmdletId, IntPtr cmdlet, IntPtr* result, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int ProviderInvokeFn(uint providerId, uint op, IntPtr instance, IntPtr args, IntPtr* result, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int LifecycleFn(uint op, IntPtr* err);
#endif

        /// <summary>
        /// One library and every export bound from it. Immutable, so a
        /// reload publishes a whole set with one reference store and a
        /// call already running keeps the set it began with.
        /// </summary>
        private sealed unsafe class Bindings
        {
            internal readonly IntPtr Lib;
            internal readonly int Generation;
            /// <summary>Write time of the file this load was taken
            /// from, which is what a later reload compares against.
            /// </summary>
            internal readonly DateTime SourceStamp;
#if NET
            internal readonly delegate* unmanaged[Cdecl]<IntPtr, int> Init;
            internal readonly delegate* unmanaged[Cdecl]<uint, IntPtr*, uint*, IntPtr*, int> Create;
            internal readonly delegate* unmanaged[Cdecl]<IntPtr, uint, IntPtr, IntPtr, IntPtr*, int> Invoke;
            internal readonly delegate* unmanaged[Cdecl]<IntPtr, void> Stop;
            internal readonly delegate* unmanaged[Cdecl]<IntPtr, void> Release;
            internal readonly delegate* unmanaged[Cdecl]<uint, uint, IntPtr, IntPtr*, IntPtr*, int> ProxyGet;
            internal readonly delegate* unmanaged[Cdecl]<uint, uint, IntPtr, IntPtr, IntPtr*, IntPtr*, int> ProxyCall;
            internal readonly delegate* unmanaged[Cdecl]<uint, IntPtr, void> ProxyDrop;
            /// <summary>Null for a library built before the export existed.</summary>
            internal readonly delegate* unmanaged[Cdecl]<uint, IntPtr, ulong> ProxyBytes;
            internal readonly delegate* unmanaged[Cdecl]<uint, PsStr16, PsStr16, IntPtr, IntPtr*, IntPtr*, int> Completer;
            /// <summary>Null for a library built before the export existed.</summary>
            internal readonly delegate* unmanaged[Cdecl]<uint, IntPtr, IntPtr*, IntPtr*, int> Transform;
            internal readonly delegate* unmanaged[Cdecl]<uint, IntPtr, IntPtr*, IntPtr*, int> DynParams;
            internal readonly delegate* unmanaged[Cdecl]<uint, uint, IntPtr, IntPtr, IntPtr*, IntPtr*, int> Provider;
            /// <summary>Null for a library built before the export existed.</summary>
            internal readonly delegate* unmanaged[Cdecl]<uint, IntPtr*, int> Lifecycle;
#else
            internal readonly ModuleInitFn Init;
            internal readonly CmdletCreateFn Create;
            internal readonly CmdletInvokeFn Invoke;
            internal readonly CmdletStopFn Stop;
            internal readonly CmdletStopFn Release;
            internal readonly ProxyGetFn ProxyGet;
            internal readonly ProxyCallFn ProxyCall;
            internal readonly ProxyDropFn ProxyDrop;
            /// <summary>Null for a library built before the export existed.</summary>
            internal readonly ProxyBytesFn? ProxyBytes;
            internal readonly CompleterInvokeFn Completer;
            /// <summary>Null for a library built before the export existed.</summary>
            internal readonly TransformInvokeFn? Transform;
            internal readonly DynParamsInvokeFn DynParams;
            internal readonly ProviderInvokeFn Provider;
            /// <summary>Null for a library built before the export existed.</summary>
            internal readonly LifecycleFn? Lifecycle;
#endif

            internal Bindings(string path, int generation, DateTime sourceStamp, IntPtr hostTable)
            {
                Generation = generation;
                SourceStamp = sourceStamp;
                Lib = Loader.Load(path);
                CpuCheck.Require(Lib, path);
#if NET
                Init =(delegate* unmanaged[Cdecl]<IntPtr, int>)Loader.Export(Lib, "pwrs_module_init");
                Create = (delegate* unmanaged[Cdecl]<uint, IntPtr*, uint*, IntPtr*, int>)Loader.Export(Lib, "pwrs_cmdlet_create");
                Invoke = (delegate* unmanaged[Cdecl]<IntPtr, uint, IntPtr, IntPtr, IntPtr*, int>)Loader.Export(Lib, "pwrs_cmdlet_invoke");
                Stop = (delegate* unmanaged[Cdecl]<IntPtr, void>)Loader.Export(Lib, "pwrs_cmdlet_stop");
                Release = (delegate* unmanaged[Cdecl]<IntPtr, void>)Loader.Export(Lib, "pwrs_cmdlet_release");
                ProxyGet = (delegate* unmanaged[Cdecl]<uint, uint, IntPtr, IntPtr*, IntPtr*, int>)Loader.Export(Lib, "pwrs_proxy_get");
                ProxyCall = (delegate* unmanaged[Cdecl]<uint, uint, IntPtr, IntPtr, IntPtr*, IntPtr*, int>)Loader.Export(Lib, "pwrs_proxy_call");
                ProxyDrop = (delegate* unmanaged[Cdecl]<uint, IntPtr, void>)Loader.Export(Lib, "pwrs_proxy_drop");
                ProxyBytes = (delegate* unmanaged[Cdecl]<uint, IntPtr, ulong>)Loader.TryExport(Lib, "pwrs_proxy_bytes");
                Completer = (delegate* unmanaged[Cdecl]<uint, PsStr16, PsStr16, IntPtr, IntPtr*, IntPtr*, int>)Loader.Export(Lib, "pwrs_completer_invoke");
                Transform = (delegate* unmanaged[Cdecl]<uint, IntPtr, IntPtr*, IntPtr*, int>)Loader.TryExport(Lib, "pwrs_transform_invoke");
                DynParams = (delegate* unmanaged[Cdecl]<uint, IntPtr, IntPtr*, IntPtr*, int>)Loader.Export(Lib, "pwrs_dynparams_invoke");
                Provider = (delegate* unmanaged[Cdecl]<uint, uint, IntPtr, IntPtr, IntPtr*, IntPtr*, int>)Loader.Export(Lib, "pwrs_provider_invoke");
                Lifecycle = (delegate* unmanaged[Cdecl]<uint, IntPtr*, int>)Loader.TryExport(Lib, "pwrs_module_lifecycle");
#else
                Init = Loader.GetExport<ModuleInitFn>(Lib, "pwrs_module_init");
                Create = Loader.GetExport<CmdletCreateFn>(Lib, "pwrs_cmdlet_create");
                Invoke = Loader.GetExport<CmdletInvokeFn>(Lib, "pwrs_cmdlet_invoke");
                Stop = Loader.GetExport<CmdletStopFn>(Lib, "pwrs_cmdlet_stop");
                Release = Loader.GetExport<CmdletStopFn>(Lib, "pwrs_cmdlet_release");
                ProxyGet = Loader.GetExport<ProxyGetFn>(Lib, "pwrs_proxy_get");
                ProxyCall = Loader.GetExport<ProxyCallFn>(Lib, "pwrs_proxy_call");
                ProxyDrop = Loader.GetExport<ProxyDropFn>(Lib, "pwrs_proxy_drop");
                ProxyBytes = Loader.TryGetExport<ProxyBytesFn>(Lib, "pwrs_proxy_bytes");
                Completer = Loader.GetExport<CompleterInvokeFn>(Lib, "pwrs_completer_invoke");
                Transform = Loader.TryGetExport<TransformInvokeFn>(Lib, "pwrs_transform_invoke");
                DynParams = Loader.GetExport<DynParamsInvokeFn>(Lib, "pwrs_dynparams_invoke");
                Provider = Loader.GetExport<ProviderInvokeFn>(Lib, "pwrs_provider_invoke");
                Lifecycle = Loader.TryGetExport<LifecycleFn>(Lib, "pwrs_module_lifecycle");
#endif
                int status = Init(hostTable);
                if (status != Native.Ok)
                {
                    throw new InvalidOperationException($"pwrs_module_init failed with status {status} (runtime ABI {Native.AbiVersion})");
                }
            }
        }

        private volatile Bindings _b;
        private readonly string _moduleRoot;
        private readonly string _libraryBaseName;

        /// <summary>
        /// Whether this runtime keeps a class-factory table per module,
        /// which it does: two modules it serves together each build their
        /// own objects. A module that hands objects to another PWRS module
        /// reads this by reflection before relying on it, since a runtime
        /// without the property keeps one table for every module it
        /// serves, and on Windows PowerShell one runtime serves every
        /// module in the process.
        /// </summary>
        public static bool FactoriesPerModule => true;

        /// <summary>
        /// This module's generated-class factories, which the generated
        /// shell fills from its type initializer.
        /// </summary>
        public Factories Factories { get; }

        /// <summary>
        /// The host table every load of this module's library is handed
        /// at init: the shared one on .NET, where the assembly holding it
        /// serves this module alone, and this module's own copy on .NET
        /// Framework.
        /// </summary>
        private readonly IntPtr _hostTable;
#if !NET
        /// <summary>
        /// Keeps the delegate behind the copy's factory_new alive for as
        /// long as this module, and so its library, is loaded.
        /// </summary>
        private readonly HostVTable.ModuleTable _table;
#endif

#if NET
        /// <summary>
        /// The folder of the module this copy of the assembly serves,
        /// which helper_path in the shared table stages from: on .NET
        /// every module has a copy of its own, as it has of its factories.
        /// </summary>
        internal static volatile string? RootOfThisCopy;
#endif

        public NativeModule(string moduleRoot, string libraryBaseName)
        {
            _moduleRoot = moduleRoot;
            _libraryBaseName = libraryBaseName;
#if NET
            Factories = Pwrs.Factories.OfThisCopy;
            _hostTable = HostVTable.Pointer;
            RootOfThisCopy = moduleRoot;
#else
            Factories = new Factories();
            _table = new HostVTable.ModuleTable(Factories, moduleRoot);
            _hostTable = _table.Pointer;
#endif
            string source = ResolvePath(moduleRoot, libraryBaseName);
            _b = new Bindings(Stage(moduleRoot, source), 0, File.GetLastWriteTimeUtc(source), _hostTable);
        }

        /// <summary>
        /// Copies the library to a per-load path under the process's
        /// own temporary folder and answers where.
        ///
        /// A mapped file is locked on Windows, so loading the built
        /// library in place would make the next `cargo pwrs build`
        /// fail to clear the module folder while a session has the
        /// module imported. It goes outside the module rather than
        /// beside it for the same reason a copy is taken at all, and
        /// because an installed module sits where the session may have
        /// no right to write. The folder is the one Pwrs.Bootstrap
        /// stages the managed assemblies into, so a session leaves one
        /// tree behind and the next session to start sweeps it.
        ///
        /// The copy's name is the write time and length of the library
        /// it was taken from, which identifies the bytes rather than
        /// the load. A new surface brings a load context of its own
        /// and with it a second copy of this class, whose per-instance
        /// state starts again; only a name the file itself decides is
        /// the same across both.
        /// </summary>
        private static string Stage(string moduleRoot, string source)
        {
            var info = new FileInfo(source);
            string into = Path.Combine(StageRoot(moduleRoot), "native");
            Directory.CreateDirectory(into);
            string staged = Path.Combine(into, Path.GetFileNameWithoutExtension(source) + "." + Mark(info) + Path.GetExtension(source));
            // A path already there holds the bytes its name names, and
            // may be mapped. Windows refuses a write over a mapped
            // file; Linux permits one, and it would rewrite the image
            // a thread is running out of.
            if (!File.Exists(staged))
            {
                try
                {
                    File.Copy(source, staged);
                }
                catch (IOException)
                {
                    // Another thread staged it between the two calls.
                }
            }
            return staged;
        }

        /// <summary>
        /// The bytes of a file, named: its write time and length in hex.
        /// A staged copy carries it, so a rebuilt file stages under a new
        /// name and the same bytes always under the same one.
        /// </summary>
        private static string Mark(FileInfo info)
            => info.LastWriteTimeUtc.Ticks.ToString("x", System.Globalization.CultureInfo.InvariantCulture)
                + "-" + info.Length.ToString("x", System.Globalization.CultureInfo.InvariantCulture);

        /// <summary>
        /// The path of a copy of the helper executable <paramref name="name"/>
        /// the module ships in runtimes/&lt;rid&gt;/native/, staged at
        /// native/&lt;file&gt;.&lt;mark&gt;/&lt;file&gt; under the process's
        /// staging folder so a running helper holds no file in the module
        /// folder. The first request for those bytes copies them under a
        /// temporary name and moves the copy into place; later requests
        /// answer the same path. Outside Windows the copy is readable and
        /// executable by its owner whatever mode the shipped file has.
        /// </summary>
        internal static string StageHelper(string moduleRoot, string name)
        {
            if (name.Length == 0 || name == "." || name == ".."
                || name.IndexOf('/') >= 0 || name.IndexOf('\\') >= 0 || name.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0)
            {
                throw new ArgumentException($"a helper is named by its file name without a folder or an extension, and '{name}' is not one", nameof(name));
            }
            string rid = RuntimeId();
            string file = ExecutableName(name);
            string shipped = Path.Combine(moduleRoot, "runtimes", rid, "native");
            var info = new FileInfo(Path.Combine(shipped, file));
            if (!info.Exists)
            {
                throw new FileNotFoundException($"pwrs helper {file} for {rid} not found in {shipped}; a helper ships when [package.metadata.pwrs] helpers names it", info.FullName);
            }
            string into = Path.Combine(StageRoot(moduleRoot), "native", file + "." + Mark(info));
            string staged = Path.Combine(into, file);
            if (File.Exists(staged)) return staged;
            Directory.CreateDirectory(into);
            string pending = Path.Combine(into, Guid.NewGuid().ToString("N") + ".partial");
            File.Copy(info.FullName, pending);
#if NET
            if (!OperatingSystem.IsWindows())
            {
                File.SetUnixFileMode(pending, File.GetUnixFileMode(pending) | UnixFileMode.UserRead | UnixFileMode.UserExecute);
            }
#endif
            try
            {
                File.Move(pending, staged);
            }
            catch (IOException) when (File.Exists(staged))
            {
                // Another thread moved the same bytes into place first.
                File.Delete(pending);
            }
            return staged;
        }

        /// <summary>
        /// The file name of the executable <paramref name="name"/> on this
        /// operating system: with .exe on Windows, bare elsewhere.
        /// </summary>
        private static string ExecutableName(string name)
        {
#if NET
            return OperatingSystem.IsWindows() ? name + ".exe" : name;
#else
            return name + ".exe";
#endif
        }

        /// <summary>
        /// Where this process stages a module's loads. Pwrs.Bootstrap
        /// computes the same path for the managed assemblies; it is
        /// not shared, because the bootstrap lives in the default
        /// context and a reference to it from here would load a
        /// second copy of it into the module's.
        /// </summary>
        private static string StageRoot(string moduleRoot)
        {
            // Masked rather than Abs: GetHashCode may return
            // int.MinValue, which has no positive counterpart.
            string key = (moduleRoot.ToUpperInvariant().GetHashCode() & 0x7fffffff).ToString(System.Globalization.CultureInfo.InvariantCulture);
#if NET
            string pid = Environment.ProcessId.ToString(System.Globalization.CultureInfo.InvariantCulture);
#else
            string pid = System.Diagnostics.Process.GetCurrentProcess().Id.ToString(System.Globalization.CultureInfo.InvariantCulture);
#endif
            return Path.Combine(Path.GetTempPath(), "pwrs-load", pid, key);
        }

        /// <summary>
        /// Which load a handle belongs to. A proxy records this when it
        /// is made and compares before every call, so a value left over
        /// from an earlier load is refused on the managed side rather
        /// than dereferenced against a body that has moved on.
        /// </summary>
        public int Generation => _b.Generation;

        /// <summary>
        /// Loads the rebuilt library beside the running one and points
        /// every export at it, when the file has changed since the
        /// running load was taken. Answers whether it did.
        ///
        /// The old image is never freed. Freeing is what makes a swap
        /// unsafe: a thread still inside the image, a callback that
        /// fires after it, a proxy dereferenced from script, and a
        /// FreeLibrary that decrements a reference count without
        /// unmapping are all faults no handler sees. Keeping the
        /// mapping leaves every one of those a live address, and costs
        /// one image per reload.
        ///
        /// The copy is what lets the build overwrite the original while
        /// this session holds the previous one: Windows keeps a mapped
        /// file locked, so the running load is always a staged copy and
        /// the path the build writes stays free.
        /// </summary>
        public bool ReloadIfChanged()
        {
            Bindings current = _b;
            string source = ResolvePath(_moduleRoot, _libraryBaseName);
            DateTime stamp = File.GetLastWriteTimeUtc(source);
            if (stamp == current.SourceStamp) return false;

            int next = current.Generation + 1;
            _b = new Bindings(Stage(_moduleRoot, source), next, stamp, _hostTable);
            return true;
        }

        /// <summary>
        /// Makes the Rust instance this cmdlet owns for its lifetime and
        /// reports which phases its type still needs a native call for.
        /// </summary>
        internal IntPtr CmdletCreate(uint cmdletId, out uint phases)
        {
            IntPtr instance = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            uint mask = Native.PhaseMaskAll;
            int status = _b.Create(cmdletId, &instance, &mask, &err);
            phases = mask;
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok || instance == IntPtr.Zero)
            {
                throw new PwrsException(errObj as string ?? $"pwrs_cmdlet_create failed with status {status}");
            }
            return instance;
        }

        internal int CmdletInvoke(IntPtr instance, uint phase, IntPtr cmdlet, IntPtr parameters, IntPtr* err)
            => _b.Invoke(instance, phase, cmdlet, parameters, err);
        internal void CmdletStop(IntPtr instance) => _b.Stop(instance);
        internal void CmdletRelease(IntPtr instance) => _b.Release(instance);
        internal int ProxyGet(uint classId, uint fieldId, IntPtr instance, IntPtr* result, IntPtr* err)
            => _b.ProxyGet(classId, fieldId, instance, result, err);
        internal int ProxyCall(uint classId, uint methodId, IntPtr instance, IntPtr args, IntPtr* result, IntPtr* err)
            => _b.ProxyCall(classId, methodId, instance, args, result, err);
        internal void ProxyDrop(uint classId, IntPtr instance) => _b.ProxyDrop(classId, instance);

        /// <summary>
        /// The native bytes a proxy value reports holding, capped at what
        /// GC.AddMemoryPressure takes; 0 for a library without the export.
        /// </summary>
        internal long ProxyBytes(uint classId, IntPtr instance)
        {
            Bindings b = _b;
            if (b.ProxyBytes == null) return 0;
            ulong bytes = b.ProxyBytes(classId, instance);
            return bytes > long.MaxValue ? long.MaxValue : (long)bytes;
        }

        /// <summary>Runs a completer; returns the object[] of string[4] rows.</summary>
        internal object?[] Complete(uint completerId, string word, string command, IDictionary fakeBound)
        {
            GCHandle bound = GCHandle.Alloc(fakeBound);
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            fixed (char* w = word)
            fixed (char* c = command)
            {
                var pw = new PsStr16 { Ptr = (ushort*)w, Len = (nuint)word.Length };
                var pc = new PsStr16 { Ptr = (ushort*)c, Len = (nuint)command.Length };
                int status = _b.Completer(completerId, pw, pc, GCHandle.ToIntPtr(bound), &result, &err);
                bound.Free();
                return Rows(status, result, err);
            }
        }

        /// <summary>
        /// Runs a transform over the value the binder is about to
        /// assign and returns what to assign instead. A library built
        /// before the export existed has no transforms, so the value
        /// passes through untouched.
        /// </summary>
        public object? Transform(uint transformId, object? value)
        {
            Bindings b = _b;
            if (b.Transform == null) return value;
            GCHandle given = GCHandle.Alloc(value);
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status = b.Transform(transformId, GCHandle.ToIntPtr(given), &result, &err);
            given.Free();
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"pwrs_transform_invoke failed with status {status}");
            }
            return Native.TakeTarget(result);
        }

        /// <summary>
        /// Runs dynamic-parameter discovery; returns the packed answer,
        /// one line per parameter joined by U+0003, or null for none.
        /// </summary>
        internal string? DynamicParameters(uint cmdletId, IntPtr cmdlet)
        {
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status = _b.DynParams(cmdletId, cmdlet, &result, &err);
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"pwrs_dynparams_invoke failed with status {status}");
            }
            object? packed = Native.TakeTarget(result);
            return (packed is System.Management.Automation.PSObject wrapped ? wrapped.BaseObject : packed) as string;
        }

        internal int ProviderInvoke(uint providerId, uint op, IntPtr instance, IntPtr args, IntPtr* result, IntPtr* err)
            => _b.Provider(providerId, op, instance, args, result, err);

        /// <summary>
        /// Runs the module's import hook (op 0) or removal hook (op 1).
        /// A library built before the export existed has no hooks and
        /// is treated as declaring none. Public because the caller is
        /// the generated shell, a separate assembly.
        /// </summary>
        public void Lifecycle(uint op)
        {
            Bindings b = _b;
            if (b.Lifecycle == null) return;
            IntPtr err = IntPtr.Zero;
            int status = b.Lifecycle(op, &err);
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"pwrs_module_lifecycle({op}) failed with status {status}");
            }
        }

        private static object?[] Rows(int status, IntPtr result, IntPtr err)
        {
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"pwrs call failed with status {status}");
            }
            object? rows = Native.TakeTarget(result);
            if (rows is object?[] arr) return arr;
            return Array.Empty<object?>();
        }

        private static string ResolvePath(string root, string baseName)
        {
            string rid = RuntimeId();
            string file = FileName(baseName);
            string candidate = Path.Combine(root, "runtimes", rid, "native", file);
            if (File.Exists(candidate)) return candidate;
            candidate = Path.Combine(root, file);
            if (File.Exists(candidate)) return candidate;
            throw new FileNotFoundException($"pwrs native library {file} for {rid} not found under {root}");
        }

        private static string FileName(string baseName)
        {
#if NET
            if (OperatingSystem.IsWindows()) return baseName + ".dll";
            if (OperatingSystem.IsMacOS()) return "lib" + baseName + ".dylib";
            return "lib" + baseName + ".so";
#else
            return baseName + ".dll";
#endif
        }

        private static string RuntimeId()
        {
            string arch = RuntimeInformation.ProcessArchitecture.ToString().ToLowerInvariant();
#if NET
            string os = OperatingSystem.IsWindows() ? "win"
                : OperatingSystem.IsMacOS() ? "osx"
                : OperatingSystem.IsFreeBSD() ? "freebsd"
                : "linux";
#else
            string os = "win";
#endif
            return os + "-" + arch;
        }
    }

    internal static class Loader
    {
#if NET
        public static IntPtr Load(string path) => NativeLibrary.Load(path);
        public static IntPtr Export(IntPtr lib, string name) => NativeLibrary.GetExport(lib, name);
        /// <summary>Zero when the library has no such export.</summary>
        public static IntPtr TryExport(IntPtr lib, string name) => NativeLibrary.TryGetExport(lib, name, out IntPtr p) ? p : IntPtr.Zero;
        /// <summary>The address of an export, code or data; zero when the library has none.</summary>
        public static IntPtr TryExportAddress(IntPtr lib, string name) => TryExport(lib, name);
#else
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryW(string path);
        [DllImport("kernel32", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern IntPtr GetProcAddress(IntPtr module, string name);

        public static IntPtr Load(string path)
        {
            IntPtr h = LoadLibraryW(path);
            if (h == IntPtr.Zero) throw new DllNotFoundException(path);
            return h;
        }
        public static T GetExport<T>(IntPtr lib, string name) where T : Delegate
        {
            IntPtr p = GetProcAddress(lib, name);
            if (p == IntPtr.Zero) throw new EntryPointNotFoundException(name);
            return Marshal.GetDelegateForFunctionPointer<T>(p);
        }
        /// <summary>Null when the library has no such export.</summary>
        public static T? TryGetExport<T>(IntPtr lib, string name) where T : Delegate
        {
            IntPtr p = GetProcAddress(lib, name);
            return p == IntPtr.Zero ? null : Marshal.GetDelegateForFunctionPointer<T>(p);
        }
        /// <summary>The address of an export, code or data; zero when the library has none.</summary>
        public static IntPtr TryExportAddress(IntPtr lib, string name) => GetProcAddress(lib, name);
#endif
    }
}

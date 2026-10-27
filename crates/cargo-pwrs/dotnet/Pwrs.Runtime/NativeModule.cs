using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Management.Automation;
using System.Management.Automation.Runspaces;
using System.Runtime.InteropServices;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// One loaded native module: resolves the library for the current
    /// runtime identifier, binds the exports, and passes the module's
    /// host table to it at init. On .NET the exports are called through
    /// unmanaged function pointers (a direct calli with the GC transition
    /// and no marshaling stub); on .NET Framework through delegates.
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
        /// call already running goes on with the set it began with.
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

            internal Bindings(string path, string source, int generation, DateTime sourceStamp, IntPtr hostTable)
            {
                Generation = generation;
                SourceStamp = sourceStamp;
                Lib = Loader.Load(path, source);
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
        /// Whether this runtime has a class-factory table per module,
        /// which it does: two modules using it together each build their
        /// own objects. A module that passes objects to another PWRS module
        /// reads this by reflection before relying on it, since a runtime
        /// without the property has one table for every module that uses
        /// it, and on Windows PowerShell every module in the process uses
        /// one runtime.
        /// </summary>
        public static bool FactoriesPerModule => true;

        /// <summary>
        /// This module's generated-class factories, which the generated
        /// shell fills from its type initializer.
        /// </summary>
        public Factories Factories { get; }

        /// <summary>
        /// The host table every load of this module's library receives
        /// at init: the shared one on .NET, where the assembly that defines
        /// it is this module's alone, and this module's own copy on .NET
        /// Framework.
        /// </summary>
        private readonly IntPtr _hostTable;
#if !NET
        /// <summary>
        /// The table behind the copy's factory_new, stored here so its
        /// delegate lives as long as this module, and so its library, is
        /// loaded.
        /// </summary>
        private readonly HostVTable.ModuleTable _table;
#endif

#if NET
        /// <summary>
        /// The folder of the module this copy of the assembly belongs to,
        /// which helper_path in the shared table stages from: on .NET
        /// every module has a copy of its own, as it has of its factories.
        /// </summary>
        internal static volatile string? RootOfThisCopy;
#endif

        public NativeModule(string moduleRoot, string libraryBaseName)
        {
            _moduleRoot = moduleRoot;
            _libraryBaseName = libraryBaseName;
            _stateChanged = OnStateChanged;
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
            _b = new Bindings(Stage(moduleRoot, source), source, 0, File.GetLastWriteTimeUtc(source), _hostTable);
        }

        /// <summary>
        /// Copies the library to a per-load path under the process's
        /// own temporary folder and returns the path.
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
        /// state starts again; only a name taken from the file itself is
        /// the same across both.
        ///
        /// Each copy is in a folder of its own with that name, with the
        /// .pdb the library records beside it when the module folder has
        /// that file beside the library. The library records the .pdb by
        /// name, so two builds' copies need two folders, and a debugger
        /// that searches a library's own folder, as Visual Studio's does,
        /// finds the symbols there. The .pdb goes in first, so a copy that
        /// exists has it.
        /// </summary>
        private static string Stage(string moduleRoot, string source)
        {
            var info = new FileInfo(source);
            string name = Path.GetFileNameWithoutExtension(source) + "." + Mark(info);
            string into = Path.Combine(StageRoot(moduleRoot), "native", name);
            Directory.CreateDirectory(into);
            string staged = Path.Combine(into, name + Path.GetExtension(source));
            // A file already at that path has the bytes its name stands
            // for, and may be mapped. Windows refuses a write over a mapped
            // file; Linux permits one, and it would rewrite the image
            // a thread is running out of.
            if (!File.Exists(staged))
            {
                StagePdb(source, into);
                Place(source, staged);
            }
            return staged;
        }

        /// <summary>
        /// Copies <paramref name="source"/> to <paramref name="staged"/>
        /// under a temporary name and moves the copy into place, so a path
        /// that exists is always whole. A file another thread moved there
        /// first has the same bytes, and this copy is deleted.
        /// </summary>
        private static void Place(string source, string staged)
        {
            if (File.Exists(staged)) return;
            string pending = staged + "." + Guid.NewGuid().ToString("N") + ".partial";
            File.Copy(source, pending);
            try
            {
                File.Move(pending, staged);
            }
            catch (IOException) when (File.Exists(staged))
            {
                File.Delete(pending);
            }
        }

        /// <summary>
        /// Copies into <paramref name="into"/> the .pdb the Windows image
        /// <paramref name="source"/> records, when the module folder has
        /// that file beside the image. A file that is not such an image
        /// records none. When the image's headers cannot be read, the
        /// reason goes to standard error and the copy goes on without
        /// symbols.
        /// </summary>
        private static void StagePdb(string source, string into)
        {
            string? pdb;
            try
            {
                pdb = Pe.RecordedPdb(source);
            }
            catch (Exception e) when (e is IOException || e is UnauthorizedAccessException || e is InvalidDataException)
            {
                Console.Error.WriteLine($"pwrs: the symbol file of {source} could not be named, so its copy has none: {e.Message}");
                return;
            }
            if (pdb == null) return;
            string shipped = Path.Combine(Path.GetDirectoryName(source) ?? string.Empty, pdb);
            if (File.Exists(shipped)) Place(shipped, Path.Combine(into, pdb));
        }

        /// <summary>
        /// A name for a file's bytes: its write time and length in hex.
        /// A staged copy's name includes it, so a rebuilt file stages under
        /// a new name and the same bytes always under the same one.
        /// </summary>
        private static string Mark(FileInfo info)
            => info.LastWriteTimeUtc.Ticks.ToString("x", System.Globalization.CultureInfo.InvariantCulture)
                + "-" + info.Length.ToString("x", System.Globalization.CultureInfo.InvariantCulture);

        /// <summary>
        /// The path of a copy of the helper executable <paramref name="name"/>
        /// the module ships in runtimes/&lt;rid&gt;/native/, staged at
        /// native/&lt;file&gt;.&lt;mark&gt;/&lt;file&gt; under the process's
        /// staging folder so a running helper uses no file in the module
        /// folder. The first call for those bytes copies them under a
        /// temporary name and moves the copy into place; later calls
        /// return the same path. Outside Windows the copy is readable and
        /// executable by its owner whatever mode the shipped file has. A
        /// helper shipped inside an app bundle, as a macOS build lays one
        /// out, is staged with its whole bundle; see StageBundle.
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
            string? bundle = BundleHolding(shipped, file);
            if (bundle != null)
            {
                return StageBundle(moduleRoot, bundle, file);
            }
            var info = new FileInfo(Path.Combine(shipped, file));
            if (!info.Exists)
            {
                throw new FileNotFoundException($"pwrs helper {file} for {rid} not found in {shipped}; a helper ships when [package.metadata.pwrs] helpers names it", info.FullName);
            }
            return StageCopy(moduleRoot, info, file, executable: true);
        }

        /// <summary>
        /// The path of a copy of the file <paramref name="name"/>, extension
        /// included, that the module ships in runtimes/&lt;rid&gt;/native/, as
        /// [package.metadata.pwrs] native-files puts one there: staged by
        /// the same rules as StageHelper, so a library loaded from it uses
        /// no file in the module folder.
        /// </summary>
        internal static string StageFile(string moduleRoot, string name)
        {
            if (name.Length == 0 || name == "." || name == ".."
                || name.IndexOf('/') >= 0 || name.IndexOf('\\') >= 0 || name.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0)
            {
                throw new ArgumentException($"a native file is named by its file name without a folder, and '{name}' is not one", nameof(name));
            }
            string rid = RuntimeId();
            string shipped = Path.Combine(moduleRoot, "runtimes", rid, "native");
            var info = new FileInfo(Path.Combine(shipped, name));
            if (!info.Exists)
            {
                throw new FileNotFoundException($"pwrs native file {name} for {rid} not found in {shipped}; a file ships there when [package.metadata.pwrs] native-files names it", info.FullName);
            }
            return StageCopy(moduleRoot, info, name, executable: false);
        }

        /// <summary>
        /// Stages <paramref name="info"/> at native/&lt;file&gt;.&lt;mark&gt;/&lt;file&gt;
        /// under the process's staging folder: the first call for those
        /// bytes copies them under a temporary name and moves the copy into
        /// place, and later calls return the same path. Outside Windows an
        /// <paramref name="executable"/> copy is readable and executable by
        /// its owner whatever mode the shipped file has. The .pdb the file
        /// records goes in first, as <see cref="Stage"/> stages the
        /// library's.
        /// </summary>
        private static string StageCopy(string moduleRoot, FileInfo info, string file, bool executable)
        {
            string into = Path.Combine(StageRoot(moduleRoot), "native", file + "." + Mark(info));
            string staged = Path.Combine(into, file);
            if (File.Exists(staged)) return staged;
            Directory.CreateDirectory(into);
            StagePdb(info.FullName, into);
            string pending = Path.Combine(into, Guid.NewGuid().ToString("N") + ".partial");
            File.Copy(info.FullName, pending);
#if NET
            if (executable && !OperatingSystem.IsWindows())
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
        /// The app bundle in <paramref name="shipped"/> whose Contents/MacOS
        /// has the executable <paramref name="file"/>, or null when none
        /// does. Two that have it are an error naming both, since which one
        /// started would otherwise depend on the order the folder lists them.
        /// </summary>
        private static string? BundleHolding(string shipped, string file)
        {
            if (!Directory.Exists(shipped)) return null;
            string? found = null;
            foreach (string bundle in Directory.GetDirectories(shipped, "*.app"))
            {
                if (!File.Exists(Path.Combine(bundle, "Contents", "MacOS", file))) continue;
                if (found != null)
                {
                    throw new InvalidOperationException($"pwrs helper {file} ships in two app bundles in {shipped}, {Path.GetFileName(found)} and {Path.GetFileName(bundle)}");
                }
                found = bundle;
            }
            return found;
        }

        /// <summary>
        /// The path of the executable <paramref name="file"/> in a copy of the
        /// app bundle <paramref name="bundle"/>, staged whole at
        /// native/&lt;bundle&gt;.&lt;mark&gt;/&lt;bundle&gt; under the process's
        /// staging folder by the rules StageHelper uses for one file: the
        /// first call copies the bundle under a temporary name and moves
        /// the copy into place, and later calls return the same path. The
        /// mark is the latest write time and the total length of the bundle's
        /// files, so a rebuilt bundle stages beside a running one. The copy
        /// has the bundle's folder name, and macOS reads the bundle the
        /// executable starts in from the folders around it. Outside Windows
        /// the copy's executable is readable and executable by its owner.
        /// </summary>
        private static string StageBundle(string moduleRoot, string bundle, string file)
        {
            string name = Path.GetFileName(bundle);
            string into = Path.Combine(StageRoot(moduleRoot), "native", name + "." + TreeMark(bundle));
            string staged = Path.Combine(into, name);
            string executable = Path.Combine(staged, "Contents", "MacOS", file);
            if (File.Exists(executable)) return executable;
            Directory.CreateDirectory(into);
            string pending = Path.Combine(into, Guid.NewGuid().ToString("N") + ".partial");
            CopyTree(bundle, pending);
#if NET
            if (!OperatingSystem.IsWindows())
            {
                string copied = Path.Combine(pending, "Contents", "MacOS", file);
                File.SetUnixFileMode(copied, File.GetUnixFileMode(copied) | UnixFileMode.UserRead | UnixFileMode.UserExecute);
            }
#endif
            try
            {
                Directory.Move(pending, staged);
            }
            catch (IOException) when (Directory.Exists(staged))
            {
                // Another thread moved the same bundle into place first.
                Directory.Delete(pending, true);
            }
            return executable;
        }

        /// <summary>
        /// The files under a folder, named as Mark names one file: the latest
        /// write time among them and their total length, in hex.
        /// </summary>
        private static string TreeMark(string folder)
        {
            long ticks = 0;
            long length = 0;
            foreach (string path in Directory.GetFiles(folder, "*", SearchOption.AllDirectories))
            {
                var info = new FileInfo(path);
                ticks = Math.Max(ticks, info.LastWriteTimeUtc.Ticks);
                length += info.Length;
            }
            return ticks.ToString("x", System.Globalization.CultureInfo.InvariantCulture)
                + "-" + length.ToString("x", System.Globalization.CultureInfo.InvariantCulture);
        }

        /// <summary>
        /// Copies the folder <paramref name="from"/> and everything under it
        /// to <paramref name="to"/>, which does not exist yet.
        /// </summary>
        private static void CopyTree(string from, string to)
        {
            Directory.CreateDirectory(to);
            foreach (string path in Directory.GetFiles(from))
            {
                File.Copy(path, Path.Combine(to, Path.GetFileName(path)));
            }
            foreach (string folder in Directory.GetDirectories(from))
            {
                CopyTree(folder, Path.Combine(to, Path.GetFileName(folder)));
            }
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
        /// running load was taken. Returns whether it did.
        ///
        /// The old image is never freed. Freeing is what makes a swap
        /// unsafe: a thread still inside the image, a callback that
        /// fires after it, a proxy dereferenced from script, and a
        /// FreeLibrary that decrements a reference count without
        /// unmapping are all faults no handler sees. With the old image
        /// still mapped, every one of those is a live address, at a cost
        /// of one image per reload.
        ///
        /// The copy is what lets the build overwrite the original while
        /// this session has the previous one loaded: Windows locks a
        /// file while it is mapped, so the running load is always a staged copy
        /// and the path the build writes stays free.
        ///
        /// For a module with lifecycle hooks, the old copy runs its unload
        /// hook when it had run its load hook, and the new copy runs its
        /// load hook when the module is imported in some runspace. An error
        /// from either is thrown once the reload is done, and both together
        /// in an AggregateException.
        ///
        /// Runspaces importing at once all call this. The one in
        /// <see cref="_gate"/> loads the library while the others wait
        /// for it, then find the file loaded and return false, so each
        /// rebuilt file is loaded and initialized once, under one
        /// generation.
        /// </summary>
        public bool ReloadIfChanged()
        {
            string source = ResolvePath(_moduleRoot, _libraryBaseName);
            if (File.GetLastWriteTimeUtc(source) == _b.SourceStamp) return false;
            if (!_gate.Enter(true)) return false;
            var errors = new List<Exception>();
            bool reloaded;
            try
            {
                reloaded = Reload(errors);
                if (reloaded && _total > 0) Load(errors);
            }
            finally
            {
                _gate.Leave();
            }
            if (errors.Count == 1) throw errors[0];
            if (errors.Count > 1) throw new AggregateException(errors);
            return reloaded;
        }

        /// <summary>
        /// Inside the gate: when the library's file has changed since the
        /// load in use was taken, loads the rebuilt library, runs the unload
        /// hook of the copy in use when that copy had loaded, and points
        /// every export at the new copy, which has not loaded. Returns
        /// whether it reloaded. A rebuilt library that fails to load throws,
        /// and the copy in use stays as it was.
        /// </summary>
        private bool Reload(List<Exception> errors)
        {
            string source = ResolvePath(_moduleRoot, _libraryBaseName);
            Bindings current = _b;
            DateTime stamp = File.GetLastWriteTimeUtc(source);
            if (stamp == current.SourceStamp) return false;
            var next = new Bindings(Stage(_moduleRoot, source), source, current.Generation + 1, stamp, _hostTable);
            if (_loaded) Unload(errors);
            _b = next;
            return true;
        }

        private const uint OpImport = 0;
        private const uint OpRemove = 1;
        private const uint OpLoad = 2;
        private const uint OpUnload = 3;

        /// <summary>
        /// Entered by the thread loading a rebuilt library, changing the
        /// count of imports, or running a lifecycle hook, so the module's
        /// hooks run one at a time and no two changes to the count overlap.
        /// </summary>
        private readonly Gate _gate = new Gate();

        /// <summary>
        /// The imports not yet removed, by the runspace that made each.
        /// Read and changed inside the gate.
        /// </summary>
        private readonly Dictionary<Runspace, int> _imports = new Dictionary<Runspace, int>();

        /// <summary>
        /// The imports not yet removed that were made on a thread with no
        /// default runspace. Read and changed inside the gate.
        /// </summary>
        private int _unattributed;

        /// <summary>Every import not yet removed. Read and changed inside the gate.</summary>
        private int _total;

        /// <summary>
        /// Whether the copy in use has run its load hook and not its unload
        /// hook since. Read and changed inside the gate.
        /// </summary>
        private bool _loaded;

        /// <summary>The handler registered for the state changes of each runspace that imported the module.</summary>
        private readonly EventHandler<RunspaceStateEventArgs> _stateChanged;

        /// <summary>The errors this thread's last import returned without failing.</summary>
        [ThreadStatic]
        private static List<Exception>? t_importErrors;

        /// <summary>
        /// Makes the Rust instance this cmdlet uses for its lifetime and
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
        /// The bytes of native memory a proxy value reports owning,
        /// capped at what GC.AddMemoryPressure takes; 0 for a library
        /// without the export.
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
        /// is returned untouched.
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
                throw errObj as Exception ?? new PwrsException(errObj as string ?? $"pwrs_transform_invoke failed with status {status}");
            }
            return Native.TakeTarget(result);
        }

        /// <summary>
        /// Runs dynamic-parameter discovery; returns the packed result,
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
                throw errObj as Exception ?? new PwrsException(errObj as string ?? $"pwrs_dynparams_invoke failed with status {status}");
            }
            object? packed = Native.TakeTarget(result);
            return (packed is System.Management.Automation.PSObject wrapped ? wrapped.BaseObject : packed) as string;
        }

        internal int ProviderInvoke(uint providerId, uint op, IntPtr instance, IntPtr args, IntPtr* result, IntPtr* err)
            => _b.Provider(providerId, op, instance, args, result, err);

        /// <summary>
        /// Runs the import hook (op 0), removal hook (op 1), load hook
        /// (op 2) or unload hook (op 3) of the copy in use, and throws the
        /// hook's error. A library built before the export existed has no
        /// hooks and is treated as declaring none. Public because on
        /// Windows PowerShell every module in the process uses one
        /// runtime, and the shell of a module built by an earlier
        /// cargo-pwrs calls this for its import and removal hooks.
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
                if (errObj is object?[] cells) throw PwrsException.FromCells(cells, null);
                throw errObj as Exception ?? new PwrsException(errObj as string ?? $"pwrs_module_lifecycle({op}) failed with status {status}");
            }
        }

        /// <summary>
        /// The module's part of an import, run by the generated shell's
        /// import hook on the thread running Import-Module. It reloads a
        /// rebuilt library, runs the load hook when the copy in use has not
        /// loaded, counts the import against that thread's runspace, and
        /// runs the import hook. An error from the load or import hook is
        /// thrown and fails the import, which then counts for nothing. An
        /// error the import returns without failing, from the old copy's
        /// unload hook at a reload, goes to <see cref="TakeImportErrors"/>.
        /// </summary>
        public void Imported()
        {
            Runspace? runspace = Runspace.DefaultRunspace;
            var errors = new List<Exception>();
            t_importErrors = errors;
            if (!_gate.Enter(true)) throw Reentered();
            try
            {
                Reload(errors);
                if (!_loaded)
                {
                    Lifecycle(OpLoad);
                    _loaded = true;
                }
                Count(runspace, 1);
                try
                {
                    Lifecycle(OpImport);
                }
                catch
                {
                    Count(runspace, -1);
                    if (_total == 0) Unload(errors);
                    throw;
                }
            }
            finally
            {
                _gate.Leave();
            }
        }

        /// <summary>
        /// Takes the errors this thread's last import returned without
        /// failing, as the records the module script writes.
        /// </summary>
        public static ErrorRecord[] TakeImportErrors()
        {
            List<Exception>? errors = t_importErrors;
            t_importErrors = null;
            return Records(errors);
        }

        /// <summary>
        /// The module's part of a removal, run from the module script's
        /// removal block on the thread running Remove-Module. For the import
        /// that thread's runspace made, it runs the removal hook, then the
        /// unload hook when no import is left in the process and the copy
        /// in use had loaded. Returns the hooks' errors as records, which
        /// the block writes; the removal goes ahead either way.
        /// </summary>
        public ErrorRecord[] Removed()
        {
            Runspace? runspace = Runspace.DefaultRunspace;
            var errors = new List<Exception>();
            if (!_gate.Enter(true))
            {
                errors.Add(Reentered());
                return Records(errors);
            }
            try
            {
                Runspace? counted = runspace != null && _imports.ContainsKey(runspace) ? runspace : null;
                if (counted != null || _unattributed > 0)
                {
                    Run(OpRemove, errors);
                    Count(counted, -1);
                    if (_total == 0 && _loaded) Unload(errors);
                }
            }
            finally
            {
                _gate.Leave();
            }
            return Records(errors);
        }

        /// <summary>
        /// Called when a runspace that imported the module changes state. A
        /// runspace closed or broken with imports left counts as removing
        /// them.
        /// </summary>
        private void OnStateChanged(object? sender, RunspaceStateEventArgs e)
        {
            RunspaceState state = e.RunspaceStateInfo.State;
            if (state != RunspaceState.Closed && state != RunspaceState.Broken) return;
            if (sender is Runspace runspace) Closed(runspace);
        }

        /// <summary>
        /// Runs the removal hook once for each import
        /// <paramref name="runspace"/> has left, then the unload hook when
        /// no import is left in the process and the copy in use had loaded.
        /// A closing runspace has no command to report to, so the hooks'
        /// errors go to standard error. A close raised on the thread inside
        /// the gate, which a hook closing a runspace is, runs on the thread
        /// pool once the gate is free.
        /// </summary>
        private void Closed(Runspace runspace)
        {
            if (!_gate.Enter(true))
            {
                ThreadPool.QueueUserWorkItem(_ => Closed(runspace));
                return;
            }
            var removal = new List<Exception>();
            var unload = new List<Exception>();
            try
            {
                if (!_imports.TryGetValue(runspace, out int left)) return;
                for (int i = 0; i < left; i++) Run(OpRemove, removal);
                Count(runspace, -left);
                if (_total == 0 && _loaded) Unload(unload);
            }
            finally
            {
                _gate.Leave();
            }
            Report("removal", removal, runspace);
            Report("unload", unload, runspace);
        }

        /// <summary>Writes each error of a hook that a runspace's close ran to standard error.</summary>
        private void Report(string hook, List<Exception> errors, Runspace runspace)
        {
            foreach (ErrorRecord record in Records(errors))
            {
                string module = Path.GetFileName(_moduleRoot.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
                Console.Error.WriteLine($"pwrs: the {hook} hook of {module} failed as runspace {runspace.InstanceId} closed: [{record.FullyQualifiedErrorId}] {record.Exception.Message}");
            }
        }

        /// <summary>
        /// Inside the gate: adds <paramref name="by"/> to the imports of
        /// <paramref name="runspace"/>, registering for its state changes
        /// at its first import and unregistering when it has none left.
        /// </summary>
        private void Count(Runspace? runspace, int by)
        {
            _total += by;
            if (runspace == null)
            {
                _unattributed += by;
                return;
            }
            _imports.TryGetValue(runspace, out int had);
            int now = had + by;
            if (now > 0)
            {
                _imports[runspace] = now;
                if (had == 0) runspace.StateChanged += _stateChanged;
            }
            else
            {
                _imports.Remove(runspace);
                runspace.StateChanged -= _stateChanged;
            }
        }

        /// <summary>Inside the gate: runs the load hook of the copy in use, adding its error to <paramref name="errors"/>.</summary>
        private void Load(List<Exception> errors)
        {
            if (Run(OpLoad, errors)) _loaded = true;
        }

        /// <summary>
        /// Inside the gate: runs the unload hook of the copy in use, adding
        /// its error to <paramref name="errors"/>. The copy counts as not
        /// loaded after it either way.
        /// </summary>
        private void Unload(List<Exception> errors)
        {
            Run(OpUnload, errors);
            _loaded = false;
        }

        /// <summary>Runs one hook, adding its error to <paramref name="errors"/>; returns whether it succeeded.</summary>
        private bool Run(uint op, List<Exception> errors)
        {
            try
            {
                Lifecycle(op);
                return true;
            }
            catch (Exception e)
            {
                errors.Add(e);
                return false;
            }
        }

        /// <summary>The error for an import or removal of the module that a hook of the module started.</summary>
        private static PwrsException Reentered()
            => new PwrsException(
                "a lifecycle hook of this module imported or removed the module, which cannot happen until that hook returns",
                null,
                "PwrsLifecycleReentered",
                ErrorCategory.InvalidOperation,
                null);

        /// <summary>The record of each error: its own, or the one PowerShell makes for an exception that has none.</summary>
        private static ErrorRecord[] Records(List<Exception>? errors)
        {
            if (errors == null || errors.Count == 0) return Array.Empty<ErrorRecord>();
            var records = new ErrorRecord[errors.Count];
            for (int i = 0; i < errors.Count; i++)
            {
                Exception e = errors[i];
                records[i] = e is IContainsErrorRecord own ? own.ErrorRecord : new ErrorRecord(e, e.GetType().FullName ?? e.GetType().Name, ErrorCategory.NotSpecified, null);
            }
            return records;
        }

        private static object?[] Rows(int status, IntPtr result, IntPtr err)
        {
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw errObj as Exception ?? new PwrsException(errObj as string ?? $"pwrs call failed with status {status}");
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
        /// <summary>
        /// Loads <paramref name="path"/>, the staged copy of the library
        /// <paramref name="source"/>. On Windows the load is NativeLibrary's,
        /// LoadLibraryExW searching the library's own folder first for what
        /// it imports, and a failure names what it lacks. Elsewhere a
        /// failure has the runtime's text, which ends in dlerror's and
        /// so names a missing dependency. A failure throws the type
        /// NativeLibrary.Load throws, BadImageFormatException for a bad
        /// image and DllNotFoundException otherwise, with no inner
        /// exception, as the module's script rethrows the innermost.
        /// </summary>
        public static IntPtr Load(string path, string source)
        {
            if (OperatingSystem.IsWindows())
            {
                IntPtr h = LoadFailure.LoadLibraryExW(path, IntPtr.Zero, LoadFailure.LoadWithAlteredSearchPath);
                if (h != IntPtr.Zero) return h;
                int error = Marshal.GetLastWin32Error();
                string message = LoadFailure.Describe(source, path, error, altered: true);
                if (error == LoadFailure.BadExeFormat) throw new BadImageFormatException(message);
                throw new DllNotFoundException(message);
            }
            try
            {
                return NativeLibrary.Load(path);
            }
            catch (DllNotFoundException e)
            {
                throw new DllNotFoundException(source + " could not be loaded: " + e.Message);
            }
            catch (BadImageFormatException e)
            {
                throw new BadImageFormatException(source + " could not be loaded: " + e.Message);
            }
        }
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

        /// <summary>
        /// Loads <paramref name="path"/>, the staged copy of the library
        /// <paramref name="source"/>, searching the executable's folder
        /// first for what it imports; a failure names what it lacks.
        /// </summary>
        public static IntPtr Load(string path, string source)
        {
            IntPtr h = LoadLibraryW(path);
            if (h != IntPtr.Zero) return h;
            int error = Marshal.GetLastWin32Error();
            throw new DllNotFoundException(LoadFailure.Describe(source, path, error, altered: false));
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

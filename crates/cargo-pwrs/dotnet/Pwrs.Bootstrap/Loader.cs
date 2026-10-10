using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
#if NET
using System.Runtime.Loader;
#endif
using System.Threading;

namespace Pwrs.Bootstrap
{
    /// <summary>
    /// Loads a module's Pwrs.Runtime and shell assemblies from the
    /// folder matching the running edition. The identity of this
    /// assembly is fixed for the life of the project so two modules
    /// never conflict on it. On .NET each module root gets its own
    /// AssemblyLoadContext that resolves the module's own files and
    /// falls through to the default context for everything else, so
    /// two modules built against different runtime versions coexist.
    /// On .NET Framework the load goes through Assembly.LoadFrom and
    /// one runtime version per process is the rule.
    ///
    /// Every runspace in the process loads through the same tables.
    /// Each is a map that is never written once published: a change
    /// copies it and swaps the copy in by compare-exchange, so a reader
    /// takes one volatile read and loads run side by side.
    /// </summary>
    public static class Loader
    {
#if NET
        private sealed class ModuleContext : AssemblyLoadContext
        {
            private readonly string _dir;

            public ModuleContext(string dir) : base("pwrs:" + dir, isCollectible: false)
            {
                _dir = dir;
            }

            protected override Assembly? Load(AssemblyName name)
            {
                string path = Path.Combine(_dir, name.Name + ".dll");
                return File.Exists(path) ? LoadFromAssemblyPath(path) : null;
            }
        }

        /// <summary>
        /// What a module root has loaded, and from where. Published
        /// before its context exists; the thread that published it makes
        /// the context and lands <see cref="Made"/>, and every other
        /// thread loading the same shell waits for that.
        /// </summary>
        private sealed class Loaded
        {
            internal readonly string Shell;
            internal readonly int Generation;
            internal readonly Flight Made = new Flight();
            /// <summary>Set before <see cref="Made"/> lands; null when making it failed.</summary>
            internal ModuleContext? Context;

            internal Loaded(string shell, int generation)
            {
                Shell = shell;
                Generation = generation;
            }
        }

        private static Dictionary<string, Loaded> s_contexts = new Dictionary<string, Loaded>(StringComparer.OrdinalIgnoreCase);
#endif

        /// <summary>
        /// Work one thread does while the others that need it wait: the
        /// thread that published the flight does the work and lands it,
        /// and a waiter spins briefly, then parks on an event made by the
        /// first waiter that needs one.
        /// </summary>
        private sealed class Flight
        {
            private int _landed;
            private ManualResetEvent? _landing;

            /// <summary>Marks the work done and wakes every waiter.</summary>
            internal void Land()
            {
                Interlocked.Exchange(ref _landed, 1);
                Volatile.Read(ref _landing)?.Set();
            }

            /// <summary>
            /// Returns once the flight has landed. A waiter publishes the
            /// event before its last look, and the lander marks the flight
            /// before it reads the event, so either the look sees the mark
            /// or the lander sets the event.
            /// </summary>
            internal void Wait()
            {
                if (Volatile.Read(ref _landed) != 0) return;
                var spin = new SpinWait();
                while (!spin.NextSpinWillYield)
                {
                    spin.SpinOnce();
                    if (Volatile.Read(ref _landed) != 0) return;
                }
                ManualResetEvent landing = Landing();
                if (Volatile.Read(ref _landed) == 0) landing.WaitOne();
            }

            private ManualResetEvent Landing()
            {
                ManualResetEvent? landing = Volatile.Read(ref _landing);
                if (landing != null) return landing;
                var made = new ManualResetEvent(false);
                ManualResetEvent? first = Interlocked.CompareExchange(ref _landing, made, null);
                if (first == null) return made;
                made.Dispose();
                return first;
            }
        }

        /// <summary>
        /// Swaps in a copy of <paramref name="seen"/> holding
        /// <paramref name="key"/> as <paramref name="value"/>, when
        /// <paramref name="seen"/> is still the published map. Answers
        /// false when another thread published first, for the caller to
        /// read the map again.
        /// </summary>
        private static bool TryPublish<T>(ref Dictionary<string, T> map, Dictionary<string, T> seen, string key, T value)
        {
            var next = new Dictionary<string, T>(seen, StringComparer.OrdinalIgnoreCase);
            next[key] = value;
            return Interlocked.CompareExchange(ref map, next, seen) == seen;
        }

        /// <summary>
        /// Copies an assembly to a per-generation folder and answers
        /// where it went. A loaded assembly is locked on Windows and
        /// the build clears the module folder before writing it, so
        /// loading from a copy is what keeps that folder writable.
        /// A path already there holds the same bytes and may be
        /// mapped, so it is left alone. The copy is written under a
        /// temporary name and renamed into place, so a path that exists
        /// is always whole.
        /// </summary>
        private static string Stage(string source, string into)
        {
            Directory.CreateDirectory(into);
            string staged = Path.Combine(into, Path.GetFileName(source));
            // Two levels up from the assembly is the module, the
            // same relation the staged copy loses.
            string tfmDir = Path.GetDirectoryName(source) ?? source;
            string root = Path.GetDirectoryName(tfmDir) ?? tfmDir;
            while (true)
            {
                Dictionary<string, string> seen = Volatile.Read(ref s_roots);
                if (seen.TryGetValue(into, out string? known) && string.Equals(known, root, StringComparison.Ordinal)) break;
                if (TryPublish(ref s_roots, seen, into, root)) break;
            }
            if (!File.Exists(staged))
            {
                string partial = staged + "." + Guid.NewGuid().ToString("N") + ".partial";
                File.Copy(source, partial);
                try
                {
                    File.Move(partial, staged);
                }
                catch (IOException) when (File.Exists(staged))
                {
                    // Staged by another caller between the two calls;
                    // that copy is whole, so this one goes.
                    File.Delete(partial);
                }
            }
            return staged;
        }

        /// <summary>The module folder each staging folder was copied from.</summary>
        private static Dictionary<string, string> s_roots = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

        /// <summary>
        /// The module folder a staged shell was copied from.
        ///
        /// A shell runs from a copy, so walking up from its own
        /// location reaches the staging folder rather than the module,
        /// and the native library and format file are not there. The
        /// loader is what knows both, and answers here.
        /// </summary>
        public static string ModuleRootOf(string shellLocation)
        {
            string dir = Path.GetDirectoryName(shellLocation) ?? shellLocation;
            if (Volatile.Read(ref s_roots).TryGetValue(dir, out string? root)) return root;
            // Loaded from the module itself, which is what Windows
            // PowerShell does before this loader has staged anything.
            string tfmDir = Path.GetDirectoryName(shellLocation) ?? throw new InvalidOperationException("shell assembly has no directory");
            return Path.GetDirectoryName(tfmDir) ?? throw new InvalidOperationException("shell assembly has no module root");
        }

        /// <summary>
        /// Copies the MAML help beside the staged shell.
        ///
        /// The engine looks for a cmdlet's help in a culture folder
        /// next to the assembly that declares it, so a shell running
        /// from a copy finds none unless its help travels with it.
        /// Each copy is written under a temporary name and renamed into
        /// place, so a help file that exists is always whole.
        /// </summary>
        private static void StageHelp(string shell, string into)
        {
            string tfmDir = Path.GetDirectoryName(shell) ?? shell;
            string name = Path.GetFileName(shell) + "-Help.xml";
            foreach (string culture in Directory.Exists(tfmDir) ? Directory.GetDirectories(tfmDir) : Array.Empty<string>())
            {
                string help = Path.Combine(culture, name);
                if (!File.Exists(help)) continue;
                string dest = Path.Combine(into, Path.GetFileName(culture));
                Directory.CreateDirectory(dest);
                string staged = Path.Combine(dest, name);
                if (File.Exists(staged)) continue;
                string partial = staged + "." + Guid.NewGuid().ToString("N") + ".partial";
                File.Copy(help, partial);
                try
                {
                    File.Move(partial, staged);
                }
                catch (IOException) when (File.Exists(staged))
                {
                    // Another thread staged it between the two calls.
                    File.Delete(partial);
                }
            }
        }

        /// <summary>
        /// The shell assembly in a folder.
        ///
        /// Its name carries a stamp of the source it was built from,
        /// so the name is not known before the folder is read. A build
        /// clears the folder before it writes, so one is what is there;
        /// where two are, the newest is the live one.
        /// </summary>
        private static string FindShell(string dir, string shellName)
        {
            string prefix = shellName + ".Shell.";
            string? newest = null;
            DateTime when = DateTime.MinValue;
            foreach (string path in Directory.Exists(dir) ? Directory.GetFiles(dir, prefix + "*.dll") : Array.Empty<string>())
            {
                // Windows matches a three-letter extension against a
                // file's short name too, so the pattern alone can
                // return one whose extension is longer.
                if (!path.EndsWith(".dll", StringComparison.OrdinalIgnoreCase)) continue;
                DateTime stamp = File.GetLastWriteTimeUtc(path);
                if (newest is null || stamp > when)
                {
                    newest = path;
                    when = stamp;
                }
            }
            if (newest is null) throw new FileNotFoundException(Path.Combine(dir, prefix + "*.dll"));
            return newest;
        }

        /// <summary>Removes a folder and everything under it.</summary>
        private static void Empty(string dir)
        {
            if (!Directory.Exists(dir)) return;
            try
            {
                Directory.Delete(dir, recursive: true);
            }
            catch (IOException)
            {
                // Left for the load that follows to write into.
            }
            catch (UnauthorizedAccessException)
            {
                // Read-only or another user's; same handling.
            }
        }

        /// <summary>Returns the shell assembly for Import-Module -Assembly.</summary>
        public static Assembly Load(string moduleRoot, string shellName, string tfmFolder)
        {
            string dir = Path.Combine(moduleRoot, tfmFolder);
            string runtime = Path.Combine(dir, "Pwrs.Runtime.dll");
            if (!File.Exists(runtime)) throw new FileNotFoundException(runtime);
            string shell = FindShell(dir, shellName);
            string stageRoot = StageRoot(moduleRoot);
#if NET
            Loaded loaded = ContextFor(dir, shell, stageRoot, tfmFolder);
            ModuleContext ctx = loaded.Context!;
            string from = GenerationFolder(stageRoot, loaded.Generation, tfmFolder);
            StageHelp(shell, from);
            ctx.LoadFromAssemblyPath(Stage(runtime, from));
            return ctx.LoadFromAssemblyPath(Stage(shell, from));
#else
            // Windows PowerShell has one load context, and needs no
            // second one: shells built from different source have
            // different assembly names, so their types are distinct
            // here too and the engine binds the newest.
            string from = GenerationFolder(stageRoot, 0, tfmFolder);
            StageHelp(shell, from);
            Assembly.LoadFrom(Stage(runtime, from));
            return Assembly.LoadFrom(Stage(shell, from));
#endif
        }

        private static string GenerationFolder(string stageRoot, int generation, string tfmFolder)
            => Path.Combine(stageRoot, "gen" + generation.ToString(System.Globalization.CultureInfo.InvariantCulture), tfmFolder);

#if NET
        /// <summary>
        /// The load of <paramref name="shell"/> from the module folder
        /// <paramref name="dir"/>, with its context made. A shell name is
        /// the identity of the source it was built from, so a new name is
        /// a new surface and gets a context of its own in the next
        /// generation's folder; the same name keeps the context, and with
        /// it the types the session holds. Of the threads loading a new
        /// name at once, the one that publishes its entry makes the
        /// context and the rest wait for it.
        /// </summary>
        private static Loaded ContextFor(string dir, string shell, string stageRoot, string tfmFolder)
        {
            while (true)
            {
                Dictionary<string, Loaded> seen = Volatile.Read(ref s_contexts);
                seen.TryGetValue(dir, out Loaded? entry);
                if (entry != null && string.Equals(entry.Shell, shell, StringComparison.OrdinalIgnoreCase))
                {
                    entry.Made.Wait();
                    if (entry.Context != null) return entry;
                    // Making it failed, and that thread put back the
                    // entry it replaced; this one tries in its turn.
                    continue;
                }
                var claim = new Loaded(shell, entry is null ? 0 : entry.Generation + 1);
                if (!TryPublish(ref s_contexts, seen, dir, claim)) continue;
                try
                {
                    claim.Context = new ModuleContext(GenerationFolder(stageRoot, claim.Generation, tfmFolder));
                }
                catch
                {
                    PutBack(dir, claim, entry);
                    throw;
                }
                finally
                {
                    claim.Made.Land();
                }
                return claim;
            }
        }

        /// <summary>
        /// Replaces <paramref name="claim"/> with the entry it replaced,
        /// or removes it when it replaced none, so the generation it took
        /// is taken again by the next load.
        /// </summary>
        private static void PutBack(string dir, Loaded claim, Loaded? replaced)
        {
            while (true)
            {
                Dictionary<string, Loaded> seen = Volatile.Read(ref s_contexts);
                if (!seen.TryGetValue(dir, out Loaded? now) || !ReferenceEquals(now, claim)) return;
                var next = new Dictionary<string, Loaded>(seen, StringComparer.OrdinalIgnoreCase);
                if (replaced is null) next.Remove(dir); else next[dir] = replaced;
                if (Interlocked.CompareExchange(ref s_contexts, next, seen) == seen) return;
            }
        }
#endif

        /// <summary>
        /// Where a module's staged loads live: under the process's own
        /// temporary folder, so two sessions never contend for one
        /// copy and nothing is written inside the module.
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
            string all = Path.Combine(Path.GetTempPath(), "pwrs-load");
            SweepOnce(all, pid);
            string root = Path.Combine(all, pid, key);
            // A process identifier is handed out again once its owner
            // is gone, so the root can hold what an unswept session
            // left. It is cleared on first use, while nothing in it is
            // this process's: the thread that publishes the root's
            // flight clears it, and every other thread staging into it
            // waits for the clear.
            while (true)
            {
                Dictionary<string, Flight> seen = Volatile.Read(ref s_fresh);
                if (seen.TryGetValue(root, out Flight? clearing))
                {
                    clearing.Wait();
                    break;
                }
                var mine = new Flight();
                if (!TryPublish(ref s_fresh, seen, root, mine)) continue;
                try
                {
                    Empty(root);
                }
                finally
                {
                    mine.Land();
                }
                break;
            }
            return root;
        }

        /// <summary>The staging roots this process has cleared or is clearing.</summary>
        private static Dictionary<string, Flight> s_fresh = new Dictionary<string, Flight>(StringComparer.OrdinalIgnoreCase);

        private static int _swept;

        /// <summary>
        /// Deletes the staging folders of sessions that have ended.
        /// A session holds its own copies mapped and so cannot delete
        /// them; each clears up after the ones already gone, which
        /// bounds a machine to the sessions currently running.
        /// </summary>
        private static void SweepOnce(string all, string ownPid)
        {
            if (Interlocked.Exchange(ref _swept, 1) != 0) return;
            if (!Directory.Exists(all)) return;
            foreach (string dir in Directory.GetDirectories(all))
            {
                string name = Path.GetFileName(dir);
                if (name == ownPid) continue;
                if (!int.TryParse(name, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out int pid)) continue;
                try
                {
                    // A pid the system has handed out again reads as
                    // live and is left alone, which errs toward
                    // keeping a folder rather than deleting one in
                    // use.
                    System.Diagnostics.Process.GetProcessById(pid).Dispose();
                    continue;
                }
                catch (ArgumentException)
                {
                    // No such process, so the folder is finished with.
                }
                catch (InvalidOperationException)
                {
                    continue;
                }
                try
                {
                    Directory.Delete(dir, recursive: true);
                }
                catch (IOException)
                {
                    // Something still holds a file in it; the next
                    // session tries again.
                }
                catch (UnauthorizedAccessException)
                {
                    // Another user's session on the same machine.
                }
            }
        }
    }
}

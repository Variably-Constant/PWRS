using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
#if NET
using System.Runtime.Loader;
#endif

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

        /// <summary>What a module root has loaded, and from where.</summary>
        private sealed class Loaded
        {
            internal ModuleContext Context = null!;
            internal string Shell = string.Empty;
            internal int Generation;
        }

        private static readonly Dictionary<string, Loaded> Contexts = new Dictionary<string, Loaded>(StringComparer.OrdinalIgnoreCase);
#endif

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
            lock (Roots)
            {
                // Two levels up from the assembly is the module, the
                // same relation the staged copy loses.
                string tfmDir = Path.GetDirectoryName(source) ?? source;
                Roots[into] = Path.GetDirectoryName(tfmDir) ?? tfmDir;
            }
            if (!File.Exists(staged))
            {
                string partial = staged + "." + Guid.NewGuid().ToString("N") + ".partial";
                File.Copy(source, partial);
                try
                {
                    File.Move(partial, staged);
                }
                catch (IOException)
                {
                    // Staged by another caller between the two calls;
                    // that copy is whole, so this one goes.
                    File.Delete(partial);
                }
            }
            return staged;
        }

        /// <summary>
        /// One load at a time per process: the staging folders and the
        /// tables above are shared by every runspace, and a copy still
        /// being written must not be what another runspace loads.
        /// </summary>
        private static readonly object LoadLock = new object();

        private static readonly Dictionary<string, string> Roots = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

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
            lock (Roots)
            {
                if (Roots.TryGetValue(dir, out string? root)) return root;
            }
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
                try
                {
                    File.Copy(help, staged);
                }
                catch (IOException)
                {
                    // Another thread staged it between the two calls.
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
            lock (LoadLock)
            {
                return LoadLocked(moduleRoot, shellName, tfmFolder);
            }
        }

        private static Assembly LoadLocked(string moduleRoot, string shellName, string tfmFolder)
        {
            string dir = Path.Combine(moduleRoot, tfmFolder);
            string runtime = Path.Combine(dir, "Pwrs.Runtime.dll");
            if (!File.Exists(runtime)) throw new FileNotFoundException(runtime);
            string shell = FindShell(dir, shellName);
#if NET
            ModuleContext ctx;
            string from;
            lock (Contexts)
            {
                // A shell name is the identity of the source it was
                // built from, so a new name is a new surface and gets
                // a context of its own; the same name keeps the
                // context, and with it the types the session holds.
                if (!Contexts.TryGetValue(dir, out Loaded? entry) || !string.Equals(entry.Shell, shell, StringComparison.OrdinalIgnoreCase))
                {
                    int generation = entry is null ? 0 : entry.Generation + 1;
                    from = Path.Combine(StageRoot(moduleRoot), "gen" + generation.ToString(System.Globalization.CultureInfo.InvariantCulture), tfmFolder);
                    ctx = new ModuleContext(from);
                    Contexts[dir] = new Loaded { Context = ctx, Shell = shell, Generation = generation };
                }
                else
                {
                    ctx = entry.Context;
                    from = Path.Combine(StageRoot(moduleRoot), "gen" + entry.Generation.ToString(System.Globalization.CultureInfo.InvariantCulture), tfmFolder);
                }
            }
            StageHelp(shell, from);
            ctx.LoadFromAssemblyPath(Stage(runtime, from));
            return ctx.LoadFromAssemblyPath(Stage(shell, from));
#else
            // Windows PowerShell has one load context, and needs no
            // second one: shells built from different source have
            // different assembly names, so their types are distinct
            // here too and the engine binds the newest.
            string from = Path.Combine(StageRoot(moduleRoot), "gen0", tfmFolder);
            StageHelp(shell, from);
            Assembly.LoadFrom(Stage(runtime, from));
            return Assembly.LoadFrom(Stage(shell, from));
#endif
        }

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
            lock (Fresh)
            {
                // A process identifier is handed out again once its
                // owner is gone, so the root can hold what an unswept
                // session left. It is cleared on first use, while
                // nothing in it is this process's.
                if (Fresh.Add(root)) Empty(root);
            }
            return root;
        }

        private static readonly HashSet<string> Fresh = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        private static int _swept;

        /// <summary>
        /// Deletes the staging folders of sessions that have ended.
        /// A session holds its own copies mapped and so cannot delete
        /// them; each clears up after the ones already gone, which
        /// bounds a machine to the sessions currently running.
        /// </summary>
        private static void SweepOnce(string all, string ownPid)
        {
            if (System.Threading.Interlocked.Exchange(ref _swept, 1) != 0) return;
            if (!Directory.Exists(all)) return;
            foreach (string dir in Directory.GetDirectories(all))
            {
                string name = Path.GetFileName(dir);
                if (name == ownPid) continue;
                if (!int.TryParse(name, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out int pid)) continue;
                try
                {
                    // A pid the system has handed out again reads as
                    // live and is left alone, which errs towards
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

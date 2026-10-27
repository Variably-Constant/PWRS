using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace Pwrs
{
    /// <summary>
    /// Says why Windows would not load a module's native library: the
    /// library, the Win32 error and its text, and for a missing module or
    /// a bad image what in the library's import graph explains it. The
    /// graph is read from the PE headers on disk. Each DLL it names is
    /// looked for as the loader looks for it: in the library's own folder
    /// when the load searched there first, then by a data-file load,
    /// which searches as this process's loads do, packaged or not, and
    /// runs no code.
    /// </summary>
    internal static unsafe class LoadFailure
    {
        internal const uint LoadWithAlteredSearchPath = 0x8;
        private const uint LoadLibraryAsDatafile = 0x2;
        private const uint LoadLibrarySearchSystem32 = 0x800;
        private const int FileNotFound = 2;
        private const int PathNotFound = 3;
        private const int ModNotFound = 126;
        internal const int BadExeFormat = 193;

#if !NET
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate int IsApiSetImplementedFn(byte* contract);
#endif

        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        internal static extern IntPtr LoadLibraryExW(string name, IntPtr file, uint flags);
        [DllImport("kernel32", SetLastError = true)]
        private static extern bool FreeLibrary(IntPtr module);
        [DllImport("kernel32", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern IntPtr GetProcAddress(IntPtr module, string name);
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr GetModuleHandleW(string name);
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern uint GetModuleFileNameW(IntPtr module, char* buffer, uint size);
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern uint GetDllDirectoryW(uint size, char* buffer);
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true, EntryPoint = "K32GetMappedFileNameW")]
        private static extern uint GetMappedFileNameW(IntPtr process, IntPtr address, char* buffer, uint size);
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern uint QueryDosDeviceW(string device, char* buffer, uint size);

        /// <summary>
        /// The message for <paramref name="loaded"/>, the staged copy of
        /// the library <paramref name="source"/>, failing to load with
        /// Win32 error <paramref name="error"/>. <paramref name="altered"/>
        /// says the load searched the library's own folder first for what
        /// it imports, as LoadLibraryExW with LOAD_WITH_ALTERED_SEARCH_PATH
        /// does, rather than the folder of the process's executable.
        /// </summary>
        internal static string Describe(string source, string loaded, int error, bool altered)
        {
            var text = new StringBuilder();
            text.Append(source).Append(" could not be loaded: Windows error ")
                .Append(error.ToString(CultureInfo.InvariantCulture))
                .Append(" (").Append(ErrorText(error, Path.GetFileName(source))).Append(").");
            if (error != ModNotFound && error != BadExeFormat) return text.ToString();
            try
            {
                Explain(loaded, error, altered, text);
            }
            catch (Exception e) when (e is IOException || e is UnauthorizedAccessException || e is InvalidDataException)
            {
                text.Append(" Its imports could not be read: ").Append(e.Message);
            }
            return text.ToString();
        }

        private static void Explain(string loaded, int error, bool altered, StringBuilder text)
        {
            if (!File.Exists(loaded))
            {
                text.Append(" Its staged copy ").Append(loaded).Append(" is not there.");
                return;
            }
            ushort process = ProcessMachine();
            Pe library = Pe.Read(loaded);
            if (!library.IsPe)
            {
                text.Append(" The file is not a Windows DLL.");
                return;
            }
            if (library.Machine != process)
            {
                text.Append(" The library is built for ").Append(MachineName(library.Machine))
                    .Append(", and this process runs ").Append(MachineName(process)).Append('.');
                return;
            }
            var walk = new Walk(altered ? Path.GetDirectoryName(loaded) : null, StandardFolders(loaded, altered), process);
            walk.Imports(library, new List<string>());
            foreach (string finding in walk.Findings) text.Append(' ').Append(finding);
            if (walk.Findings.Count == 0)
            {
                text.Append(error == ModNotFound
                    ? " Every DLL it imports, directly or through another DLL, is on this process's DLL search path."
                    : " It and every DLL it imports, directly or through another DLL, are built for " + MachineName(process) + ".");
            }
            if (walk.ApiSetsUnchecked)
            {
                text.Append(" This version of Windows does not report which API sets it implements, so the API sets among its imports were not checked.");
            }
        }

        /// <summary>
        /// The folders the standard search order lists for a DLL named
        /// without a path, past the API sets, the loaded-module list and
        /// the known DLLs: the library's own folder for an altered search
        /// and the executable's otherwise, the folder SetDllDirectory names,
        /// the system folder, the 16-bit system folder, the Windows folder,
        /// the current folder unless SetDllDirectory names one, and PATH.
        /// A packaged process searches fewer; these name the file of a
        /// name the loader found but could not map, and one it did not
        /// find in a folder it does not search.
        /// </summary>
        private static List<string> StandardFolders(string loaded, bool altered)
        {
            var folders = new List<string>();
            string? anchor = altered ? loaded : ModulePath(IntPtr.Zero);
            string? first = anchor == null ? null : Path.GetDirectoryName(anchor);
            if (!string.IsNullOrEmpty(first)) folders.Add(first!);
            string? dllDirectory = DllDirectory();
            if (dllDirectory != null) folders.Add(dllDirectory);
            string windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            folders.Add(Environment.SystemDirectory);
            folders.Add(Path.Combine(windows, "System"));
            folders.Add(windows);
            if (dllDirectory == null) folders.Add(Environment.CurrentDirectory);
            foreach (string entry in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(';'))
            {
                string folder = entry.Trim().Trim('"');
                if (folder.Length > 0 && folder.IndexOfAny(Path.GetInvalidPathChars()) < 0) folders.Add(folder);
            }
            return folders;
        }

        /// <summary>
        /// Walks a library's static imports and every import of those it
        /// finds, each DLL once, collecting a sentence for each one the
        /// loader cannot have: not found, an API set this Windows does not
        /// implement, not a PE file, or built for another machine.
        /// </summary>
        private sealed class Walk
        {
            private readonly string? _libraryFolder;
            private readonly List<string> _standardFolders;
            private readonly ushort _machine;
            private readonly HashSet<string> _seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            private bool _queried;
            private IntPtr _apiSetQuery;
            internal readonly List<string> Findings = new List<string>();
            internal bool ApiSetsUnchecked;

            internal Walk(string? libraryFolder, List<string> standardFolders, ushort machine)
            {
                _libraryFolder = libraryFolder;
                _standardFolders = standardFolders;
                _machine = machine;
            }

            /// <summary>
            /// Checks the imports of <paramref name="pe"/>, reached from the
            /// library through the DLLs in <paramref name="chain"/>.
            /// </summary>
            internal void Imports(Pe pe, List<string> chain)
            {
                foreach (string name in pe.Imports)
                {
                    if (!_seen.Add(name)) continue;
                    var path = new List<string>(chain) { name };
                    if (name.StartsWith("api-", StringComparison.OrdinalIgnoreCase) || name.StartsWith("ext-", StringComparison.OrdinalIgnoreCase))
                    {
                        bool? implemented = ApiSetImplemented(name);
                        if (implemented == null) ApiSetsUnchecked = true;
                        else if (implemented == false) Findings.Add(Sentence(path) + ", an API set this version of Windows does not implement.");
                        continue;
                    }
                    if (GetModuleHandleW(name) != IntPtr.Zero) continue;
                    int error = Locate(name, out string? found);
                    if (error == FileNotFound || error == PathNotFound || error == ModNotFound)
                    {
                        string? unsearched = FirstStandardCopy(name);
                        Findings.Add(Sentence(path) + ", which is not on this process's DLL search path."
                            + (unsearched == null ? "" : " " + unsearched + " exists, in a folder this process does not search for DLLs."));
                        continue;
                    }
                    if (error == BadExeFormat)
                    {
                        string? copy = FirstStandardCopy(name);
                        Findings.Add(Sentence(path) + (copy == null ? ", whose copy on this process's DLL search path" : ", whose copy at " + copy) + " is not a Windows DLL.");
                        continue;
                    }
                    if (error != 0)
                    {
                        Findings.Add(Sentence(path) + ", which this process could not look for: Windows error "
                            + error.ToString(CultureInfo.InvariantCulture) + " (" + ErrorText(error, name) + ").");
                        continue;
                    }
                    if (found == null)
                    {
                        Findings.Add(Sentence(path) + ", which this process finds in a file it cannot name, so the DLLs that one imports were not checked.");
                        continue;
                    }
                    Pe dependency;
                    try
                    {
                        dependency = Pe.Read(found);
                    }
                    catch (Exception e) when (e is IOException || e is UnauthorizedAccessException || e is InvalidDataException)
                    {
                        Findings.Add(Sentence(path) + ", whose copy at " + found + " could not be read: " + e.Message);
                        continue;
                    }
                    if (!dependency.IsPe)
                    {
                        Findings.Add(Sentence(path) + ", whose copy at " + found + " is not a Windows DLL.");
                    }
                    else if (dependency.Machine != _machine)
                    {
                        Findings.Add(Sentence(path) + ", whose copy at " + found + " is built for " + MachineName(dependency.Machine)
                            + ", and this process runs " + MachineName(_machine) + ".");
                    }
                    else
                    {
                        Imports(dependency, path);
                    }
                }
            }

            private static string Sentence(List<string> path) => "It imports " + string.Join(", which imports ", path);

            /// <summary>
            /// Looks for <paramref name="name"/> as the loader does past the
            /// loaded-module list: in the library's folder when the load
            /// searched it first, then by a data-file load. Returns 0 with
            /// the file found in <paramref name="found"/>, null when its
            /// path cannot be named, or the Win32 error of the search.
            /// </summary>
            private int Locate(string name, out string? found)
            {
                found = null;
                if (_libraryFolder != null)
                {
                    string beside = Path.Combine(_libraryFolder, name);
                    if (File.Exists(beside))
                    {
                        found = beside;
                        return 0;
                    }
                }
                IntPtr mapped = LoadLibraryExW(name, IntPtr.Zero, LoadLibraryAsDatafile);
                if (mapped == IntPtr.Zero) return Marshal.GetLastWin32Error();
                found = MappedPath(mapped);
                FreeLibrary(mapped);
                return 0;
            }

            /// <summary>The first file named <paramref name="name"/> in the standard search order's folders.</summary>
            private string? FirstStandardCopy(string name)
            {
                foreach (string folder in _standardFolders)
                {
                    string candidate = Path.Combine(folder, name);
                    if (File.Exists(candidate)) return candidate;
                }
                return null;
            }

            /// <summary>
            /// Whether this Windows implements the API set
            /// <paramref name="name"/>, or null when it has no
            /// IsApiSetImplemented to call.
            /// </summary>
            private bool? ApiSetImplemented(string name)
            {
                if (!_queried)
                {
                    _queried = true;
                    IntPtr host = LoadLibraryExW("api-ms-win-core-apiquery-l2-1-0.dll", IntPtr.Zero, LoadLibrarySearchSystem32);
                    _apiSetQuery = host == IntPtr.Zero ? IntPtr.Zero : GetProcAddress(host, "IsApiSetImplemented");
                }
                if (_apiSetQuery == IntPtr.Zero) return null;
                string contract = name.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) ? name.Substring(0, name.Length - 4) : name;
                byte[] ascii = Encoding.ASCII.GetBytes(contract + "\0");
                fixed (byte* c = ascii)
                {
#if NET
                    return ((delegate* unmanaged[Stdcall]<byte*, int>)_apiSetQuery)(c) != 0;
#else
                    return Marshal.GetDelegateForFunctionPointer<IsApiSetImplementedFn>(_apiSetQuery)(c) != 0;
#endif
                }
            }
        }

        private static string ErrorText(int error, string file)
            => new Win32Exception(error).Message.Replace("%1", file).TrimEnd(' ', '.', '\r', '\n');

        private static ushort ProcessMachine() => RuntimeInformation.ProcessArchitecture switch
        {
            Architecture.X86 => 0x014c,
            Architecture.X64 => 0x8664,
            Architecture.Arm => 0x01c4,
            Architecture.Arm64 => 0xaa64,
            _ => 0,
        };

        private static string MachineName(ushort machine) => machine switch
        {
            0x014c => "x86",
            0x8664 => "x64",
            0x01c4 => "ARM",
            0xaa64 => "ARM64",
            _ => "machine 0x" + machine.ToString("x4", CultureInfo.InvariantCulture),
        };

        /// <summary>The path a loaded module was loaded from; the executable's for zero.</summary>
        private static string? ModulePath(IntPtr module)
        {
            for (uint size = 260; size <= 32768; size <<= 1)
            {
                var buffer = new char[size];
                uint length;
                fixed (char* p = buffer) length = GetModuleFileNameW(module, p, size);
                if (length == 0) return null;
                if (length < size) return new string(buffer, 0, (int)length);
            }
            return null;
        }

        /// <summary>
        /// The path of the file a data-file load mapped, from the device
        /// path the mapping reports, or null when the file is on no drive
        /// or share.
        /// </summary>
        private static string? MappedPath(IntPtr mapped)
        {
            var buffer = new char[32768];
            uint length;
            fixed (char* p = buffer) length = GetMappedFileNameW(new IntPtr(-1), new IntPtr(mapped.ToInt64() & ~3L), p, (uint)buffer.Length);
            if (length == 0) return null;
            string device = new string(buffer, 0, (int)length);
            foreach (string drive in Environment.GetLogicalDrives())
            {
                string letter = drive.TrimEnd('\\');
                string? target = DeviceOf(letter);
                if (target != null && device.Length > target.Length && device[target.Length] == '\\'
                    && device.StartsWith(target, StringComparison.OrdinalIgnoreCase))
                {
                    return letter + device.Substring(target.Length);
                }
            }
            const string share = @"\Device\Mup\";
            return device.StartsWith(share, StringComparison.OrdinalIgnoreCase) ? @"\\" + device.Substring(share.Length) : null;
        }

        /// <summary>The device a drive letter such as C: names.</summary>
        private static string? DeviceOf(string letter)
        {
            var buffer = new char[1024];
            uint length;
            fixed (char* p = buffer) length = QueryDosDeviceW(letter, p, (uint)buffer.Length);
            if (length == 0) return null;
            int end = Array.IndexOf(buffer, '\0');
            return new string(buffer, 0, end < 0 ? (int)length : end);
        }

        /// <summary>The folder SetDllDirectory last named, or null for none.</summary>
        private static string? DllDirectory()
        {
            uint needed = GetDllDirectoryW(0, null);
            if (needed == 0) return null;
            var buffer = new char[needed + 1];
            uint length;
            fixed (char* p = buffer) length = GetDllDirectoryW((uint)buffer.Length, p);
            return length == 0 || length >= buffer.Length ? null : new string(buffer, 0, (int)length);
        }
    }
}

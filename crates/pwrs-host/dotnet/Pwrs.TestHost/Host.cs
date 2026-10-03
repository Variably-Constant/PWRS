using System;
using System.Collections.Generic;
using System.Management.Automation;
using System.Management.Automation.Runspaces;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace Pwrs.TestHost
{
    /// <summary>
    /// Entry points the Rust host calls after loading this assembly
    /// through hostfxr. One runspace lives for the process; each Run
    /// executes a script in it and returns every stream as JSON.
    /// </summary>
    public static unsafe class Host
    {
        private static Runspace? _runspace;
        private static readonly object RunLock = new object();

        private sealed class RunResult
        {
            public List<string> Output { get; } = new List<string>();
            public List<string> Errors { get; } = new List<string>();
            public List<string> Verbose { get; } = new List<string>();
            public List<string> Warning { get; } = new List<string>();
            public List<string> Information { get; } = new List<string>();
            public string? Terminating { get; set; }
        }

        private static Runspace Runspace()
        {
            if (_runspace == null)
            {
                var rs = RunspaceFactory.CreateRunspace(InitialSessionState.CreateDefault2());
                rs.Open();
                _runspace = rs;
            }
            return _runspace;
        }

        /// <summary>Runs a script; returns a GCHandle to the UTF-16 JSON result.</summary>
        [UnmanagedCallersOnly(EntryPoint = "pwrs_testhost_run")]
        public static IntPtr Run(ushort* script, nuint length)
        {
            var result = new RunResult();
            string text = new string((char*)script, 0, checked((int)length));
            lock (RunLock)
            {
                try
                {
                    using var ps = PowerShell.Create();
                    ps.Runspace = Runspace();
                    ps.AddScript(text);
                    var output = ps.Invoke();
                    foreach (var o in output) result.Output.Add(o == null ? "" : (LanguagePrimitives.ConvertTo<string>(o) ?? ""));
                    foreach (var e in ps.Streams.Error) result.Errors.Add(e.FullyQualifiedErrorId + ": " + e.Exception.Message);
                    foreach (var v in ps.Streams.Verbose) result.Verbose.Add(v.Message);
                    foreach (var w in ps.Streams.Warning) result.Warning.Add(w.Message);
                    foreach (var i in ps.Streams.Information) result.Information.Add(i.MessageData?.ToString() ?? "");
                }
                catch (Exception e)
                {
                    result.Terminating = e.GetType().FullName + ": " + e.Message;
                }
            }
            string json = JsonSerializer.Serialize(result);
            return GCHandle.ToIntPtr(GCHandle.Alloc(json));
        }

        /// <summary>Copies a result string out; returns the full length in UTF-16 units.</summary>
        [UnmanagedCallersOnly(EntryPoint = "pwrs_testhost_read")]
        public static nuint Read(IntPtr handle, ushort* buffer, nuint capacity)
        {
            string s = (string)GCHandle.FromIntPtr(handle).Target!;
            int n = (int)Math.Min((ulong)s.Length, (ulong)capacity);
            if (buffer != null && n > 0)
            {
                fixed (char* src = s) Buffer.MemoryCopy(src, buffer, (long)capacity * 2, (long)n * 2);
            }
            return (nuint)s.Length;
        }

        [UnmanagedCallersOnly(EntryPoint = "pwrs_testhost_free")]
        public static void Free(IntPtr handle)
        {
            if (handle != IntPtr.Zero) GCHandle.FromIntPtr(handle).Free();
        }
    }
}

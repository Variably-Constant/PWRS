using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Management.Automation.Runspaces;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

namespace Pwrs
{
    /// <summary>
    /// An event of a COM object made late-bound, as New-Object -ComObject
    /// makes one, which .NET reflection does not see: one of the methods of
    /// the default source interface its class's type information names,
    /// reached through <see cref="ComEventsHelper"/>.
    /// </summary>
    internal readonly struct ComEvent
    {
        /// <summary>The IID of the source interface the event is a method of.</summary>
        internal readonly Guid Interface;
        /// <summary>The event's DISPID in that interface.</summary>
        internal readonly int DispId;
        /// <summary>How many arguments the event passes.</summary>
        internal readonly int Arguments;
        /// <summary>The event's name as the type information spells it.</summary>
        internal readonly string Name;
        /// <summary>The object's class, as its type information names it.</summary>
        internal readonly string Owner;

        private ComEvent(Guid source, int dispId, int arguments, string name, string owner)
        {
            Interface = source;
            DispId = dispId;
            Arguments = arguments;
            Name = name;
            Owner = owner;
        }

        [ComImport, Guid("B196B283-BAB4-101A-B69C-00AA00341D07"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IProvideClassInfo
        {
            [PreserveSig]
            int GetClassInfo(out ITypeInfo info);
        }

        private const short VtEmpty = 0;
        private const short VtVoid = 24;
        private const short VtHresult = 25;

        /// <summary>
        /// Finds the event <paramref name="name"/>, compared without regard
        /// to case as COM compares names, of the COM object <paramref name="rcw"/>,
        /// or throws naming why it has none a Rust callback can take.
        /// </summary>
        internal static ComEvent Find(object rcw, string name)
        {
            if (!(rcw is IProvideClassInfo provider))
            {
                throw new PwrsException($"the COM object has no event {name} PWRS can reach: it does not implement IProvideClassInfo, so nothing names its events");
            }
            int hr = provider.GetClassInfo(out ITypeInfo coclass);
            if (hr != 0)
            {
                throw new PwrsException($"the COM object has no event {name} PWRS can reach: reading its class's type information failed with HRESULT 0x{hr:X8}");
            }
            coclass.GetDocumentation(-1, out string owner, out _, out _, out _);
            ITypeInfo? source = DefaultSource(coclass);
            if (source == null)
            {
                throw new PwrsException($"the COM object {owner} raises no events: its class names no default source interface");
            }
            source.GetTypeAttr(out IntPtr attrPtr);
            TYPEATTR attr = Marshal.PtrToStructure<TYPEATTR>(attrPtr);
            source.ReleaseTypeAttr(attrPtr);
            var events = new List<string>();
            for (int i = 0; i < attr.cFuncs; i++)
            {
                source.GetFuncDesc(i, out IntPtr descPtr);
                try
                {
                    FUNCDESC desc = Marshal.PtrToStructure<FUNCDESC>(descPtr);
                    if ((desc.wFuncFlags & (short)FUNCFLAGS.FUNCFLAG_FRESTRICTED) != 0) continue;
                    source.GetDocumentation(desc.memid, out string method, out _, out _, out _);
                    events.Add(method);
                    if (!string.Equals(method, name, StringComparison.OrdinalIgnoreCase)) continue;
                    short returns = desc.elemdescFunc.tdesc.vt;
                    if (returns != VtVoid && returns != VtHresult && returns != VtEmpty)
                    {
                        throw new PwrsException($"the event {method} of the COM object {owner} returns a value, and a Rust callback returns nothing");
                    }
                    int size = Marshal.SizeOf<ELEMDESC>();
                    for (int p = 0; p < desc.cParams; p++)
                    {
                        ELEMDESC param = Marshal.PtrToStructure<ELEMDESC>(desc.lprgelemdescParam + p * size);
                        if ((param.desc.paramdesc.wParamFlags & PARAMFLAG.PARAMFLAG_FOUT) != 0)
                        {
                            throw new PwrsException($"the event {method} of the COM object {owner} takes an out or in-out argument, which a Rust callback is not given");
                        }
                    }
                    return new ComEvent(attr.guid, desc.memid, desc.cParams, method, owner);
                }
                finally
                {
                    source.ReleaseFuncDesc(descPtr);
                }
            }
            throw new PwrsException($"the COM object {owner} has no event {name}; its events are {(events.Count == 0 ? "none" : string.Join(", ", events))}");
        }

        /// <summary>The interface <paramref name="coclass"/> marks as its default source, or null.</summary>
        private static ITypeInfo? DefaultSource(ITypeInfo coclass)
        {
            coclass.GetTypeAttr(out IntPtr attrPtr);
            TYPEATTR attr = Marshal.PtrToStructure<TYPEATTR>(attrPtr);
            coclass.ReleaseTypeAttr(attrPtr);
            const IMPLTYPEFLAGS wanted = IMPLTYPEFLAGS.IMPLTYPEFLAG_FDEFAULT | IMPLTYPEFLAGS.IMPLTYPEFLAG_FSOURCE;
            for (int i = 0; i < attr.cImplTypes; i++)
            {
                coclass.GetImplTypeFlags(i, out IMPLTYPEFLAGS flags);
                if ((flags & wanted) != wanted) continue;
                coclass.GetRefTypeOfImplType(i, out int href);
                coclass.GetRefTypeInfo(href, out ITypeInfo source);
                return source;
            }
            return null;
        }
    }

    /// <summary>
    /// The handlers one module's library has on COM objects' events, each
    /// with the runspace it was added in. pwsh 7 can fail fast as it exits
    /// while such a handler is still attached, so each is detached when the
    /// runspace it was added in closes, which pwsh does before it exits,
    /// and every one left when the module unloads, or at last when the
    /// process exits.
    /// </summary>
    internal sealed class ComHooks
    {
#if NET
        /// <summary>
        /// The handlers of the module this copy of the assembly belongs to:
        /// on .NET every module has a copy of its own.
        /// </summary>
        internal static readonly ComHooks OfThisCopy = new ComHooks();
#endif

        private readonly ConcurrentDictionary<Subscription, Runspace?> _live = new ConcurrentDictionary<Subscription, Runspace?>();

        /// <summary>The module's name, which its NativeModule sets, for what a detach at exit writes.</summary>
        internal string Module { get; set; } = "a PWRS module";

        internal ComHooks()
        {
            AppDomain.CurrentDomain.ProcessExit += (sender, e) =>
            {
                foreach (Exception error in DetachAll())
                {
                    Console.Error.WriteLine($"pwrs: detaching a COM event handler of {Module} as the process exits failed: {error.Message}");
                }
            };
        }

        /// <summary>Records a handler just attached, in the runspace of the thread that added it, if it has one.</summary>
        internal void Add(Subscription handler) => _live[handler] = Runspace.DefaultRunspace;

        /// <summary>Forgets a handler that has been detached.</summary>
        internal void Forget(Subscription handler) => _live.TryRemove(handler, out _);

        /// <summary>Detaches every handler added in <paramref name="runspace"/> and returns what failed.</summary>
        internal List<Exception> DetachMadeIn(Runspace runspace)
        {
            var errors = new List<Exception>();
            foreach (KeyValuePair<Subscription, Runspace?> pair in _live)
            {
                if (ReferenceEquals(pair.Value, runspace)) Detach(pair.Key, errors);
            }
            return errors;
        }

        /// <summary>Detaches every handler left and returns what failed.</summary>
        internal List<Exception> DetachAll()
        {
            var errors = new List<Exception>();
            foreach (Subscription handler in _live.Keys) Detach(handler, errors);
            return errors;
        }

        private static void Detach(Subscription handler, List<Exception> errors)
        {
            try
            {
                handler.Remove();
            }
            catch (Exception e)
            {
                errors.Add(e);
            }
        }
    }
}

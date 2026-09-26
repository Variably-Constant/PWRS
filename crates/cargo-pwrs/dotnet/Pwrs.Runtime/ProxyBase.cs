using System;
using System.Runtime.InteropServices;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// Base of every generated proxy class. The Rust value lives behind
    /// the pointer. Each property getter, each method and each borrow a
    /// cmdlet takes through PsProxy is one call into it, serialized per
    /// object because a method may mutate the value. An entry is shared
    /// (a property read, a method taking `&self`, a `with` borrow) or
    /// exclusive (a method taking `&mut self`, a `with_mut` borrow).
    /// Shared entries nest on the thread inside one, so a call may read
    /// the object it is running on; an exclusive entry holds the only
    /// reference to the value, so it is refused while anything is inside
    /// and refuses everything while it runs. Dispose or finalization
    /// frees the value exactly once; asked for during a call on the same
    /// thread, the free waits for the last entry to end.
    ///
    /// A class declared with native_bytes reports the bytes its value
    /// holds to the garbage collector with GC.AddMemoryPressure when the
    /// wrapper is made, asks again after each method call and each
    /// mutable borrow, and withdraws the figure when the value is freed,
    /// so a small wrapper over a large value is collected on the value's
    /// account and not only the wrapper's.
    /// </summary>
    public abstract class ProxyBase : IDisposable
    {
        private readonly NativeModule _module;
        private readonly uint _classId;
        private readonly int _generation;
        private readonly bool _reportsBytes;
        private readonly object _gate = new object();
        private IntPtr _instance;
        /// <summary>Shared entries the thread holding the gate is inside: property reads, `&self` methods and `with` borrows, which nest.</summary>
        private int _shared;
        /// <summary>An exclusive entry is running, a `&mut self` method or a `with_mut` borrow: the only reference to the value.</summary>
        private bool _exclusive;
        /// <summary>Dispose was asked for during a call; the call's end frees the value.</summary>
        private bool _releasePending;
        /// <summary>Bytes reported with GC.AddMemoryPressure and not yet withdrawn.</summary>
        private long _pressure;

        protected ProxyBase(NativeModule module, uint classId, IntPtr instance) : this(module, classId, instance, false) { }

        protected ProxyBase(NativeModule module, uint classId, IntPtr instance, bool reportsBytes)
        {
            _module = module;
            _classId = classId;
            _generation = module.Generation;
            _instance = instance;
            _reportsBytes = reportsBytes;
            if (reportsBytes) UpdatePressure(instance);
        }

        /// <summary>
        /// Refuses a value whose module has been reloaded since it was
        /// made. The pointer addresses the load that produced it, and
        /// that load's body is no longer the one behind the exports, so
        /// the check happens here rather than across the boundary: code
        /// inside the old image cannot be the thing that guards against
        /// it.
        /// </summary>
        private void RequireCurrentGeneration()
        {
            int now = _module.Generation;
            if (now != _generation)
            {
                throw new PwrsException(
                    $"this {GetType().Name} was made by load {_generation} of the module and load {now} is running; make it again");
            }
        }

        /// <summary>
        /// Takes the gate for one call into the value and answers its
        /// pointer, or throws when the value is freed, belongs to an
        /// earlier load, or is in use on this thread in a way the call
        /// cannot share: any entry while an exclusive one runs, and an
        /// exclusive entry while any is inside. Every success is paired
        /// with <see cref="Leave"/> on the same thread.
        /// </summary>
        private IntPtr Enter(bool exclusive)
        {
            Monitor.Enter(_gate);
            try
            {
                if (_exclusive || (exclusive && _shared > 0))
                {
                    throw new PwrsException($"this {GetType().Name} is in use by a call already running on this thread; it can be reached again once that call returns");
                }
                IntPtr instance = _instance;
                if (instance == IntPtr.Zero) throw new ObjectDisposedException(GetType().FullName);
                RequireCurrentGeneration();
                if (exclusive) _exclusive = true; else _shared++;
                return instance;
            }
            catch
            {
                Monitor.Exit(_gate);
                throw;
            }
        }

        /// <summary>
        /// Ends a call <see cref="Enter"/> began: asks for the value's
        /// bytes again when the call may have changed it, frees the value
        /// when Dispose was asked for meanwhile and this was the last
        /// entry, and releases the gate. An exclusive entry never shares
        /// the gate with a shared one, so the entry ending is the
        /// exclusive one while that flag is set and a shared one
        /// otherwise. Throws nothing.
        /// </summary>
        private void Leave(bool changed)
        {
            if (changed && _reportsBytes) UpdatePressure(_instance);
            if (_exclusive) _exclusive = false; else _shared--;
            if (_releasePending && !_exclusive && _shared == 0) Free();
            Monitor.Exit(_gate);
        }

        /// <summary>
        /// Moves the figure reported to the collector to what the value
        /// answers now. Runs with the gate held, or in the constructor
        /// before the object is shared, and never for a value of an
        /// earlier load, whose export has been replaced.
        /// </summary>
        private void UpdatePressure(IntPtr instance)
        {
            if (instance == IntPtr.Zero || _module.Generation != _generation) return;
            long now = _module.ProxyBytes(_classId, instance);
            long before = _pressure;
            if (now > before) GC.AddMemoryPressure(now - before);
            else if (now < before) GC.RemoveMemoryPressure(before - now);
            _pressure = now;
        }

        /// <summary>
        /// Reads field <paramref name="fieldId"/> from the Rust value.
        /// Named with the prefix, and called through base by the
        /// generated code, so a module method named Get never enters
        /// the candidate set in its place.
        /// </summary>
        protected unsafe object? PwrsGet(uint fieldId)
        {
            IntPtr instance = Enter(false);
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status;
            try
            {
                status = _module.ProxyGet(_classId, fieldId, instance, &result, &err);
            }
            finally
            {
                Leave(false);
            }
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"proxy field read failed with status {status}");
            }
            return Native.TakeTarget(result);
        }

        /// <summary>
        /// Runs method <paramref name="methodId"/> on the Rust value with
        /// the packed argument block <paramref name="args"/>; a Rust
        /// error becomes a PwrsException. <paramref name="exclusive"/> is
        /// true for a method taking `&mut self`, which holds the only
        /// reference while it runs, and false for one taking `&self`,
        /// whose entry is shared. Named with the prefix for the reason
        /// PwrsGet is.
        /// </summary>
        protected unsafe object? PwrsCall(uint methodId, void* args, bool exclusive)
        {
            IntPtr instance = Enter(exclusive);
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status;
            try
            {
                status = _module.ProxyCall(_classId, methodId, instance, (IntPtr)args, &result, &err);
            }
            finally
            {
                Leave(true);
            }
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"proxy method call failed with status {status}");
            }
            return Native.TakeTarget(result);
        }

        /// <summary>
        /// Lends the value to native code of the module whose factories
        /// are <paramref name="caller"/>, when this object is of its class
        /// <paramref name="classId"/>: exclusively for a `with_mut`
        /// borrow, as shared for a `with` borrow. Paired with
        /// <see cref="ExitBorrow"/>.
        /// </summary>
        internal IntPtr EnterBorrow(Factories caller, uint classId, bool exclusive)
        {
            if (!ReferenceEquals(_module.Factories, caller) || classId != _classId)
            {
                throw new PwrsException($"a {GetType().FullName} was passed where another class, or another module's class, was expected");
            }
            return Enter(exclusive);
        }

        /// <summary>
        /// Ends a borrow <see cref="EnterBorrow"/> began. A thread that
        /// does not hold the gate has no borrow to end.
        /// </summary>
        internal void ExitBorrow(bool changed)
        {
            if (!Monitor.IsEntered(_gate)) return;
            Leave(changed);
        }

        public bool IsDisposed => _instance == IntPtr.Zero;

        /// <summary>
        /// Frees the Rust value, or, when a call into it is running on
        /// this thread, leaves the free to the end of the last entry.
        /// Another thread waits for the gate, so no value is freed under
        /// a running call.
        /// </summary>
        private void ReleaseInstance()
        {
            lock (_gate)
            {
                if (_exclusive || _shared > 0)
                {
                    _releasePending = true;
                    return;
                }
                Free();
            }
        }

        /// <summary>
        /// Withdraws the reported bytes and frees the value, unless the
        /// module has been reloaded since it was made. The exports now
        /// address the new load, so handing it a pointer from the old one
        /// would free a value in a heap it does not own; the value is
        /// abandoned instead and goes with the image that still holds it.
        /// Runs with the gate held.
        /// </summary>
        private void Free()
        {
            _releasePending = false;
            IntPtr instance = _instance;
            if (instance == IntPtr.Zero) return;
            _instance = IntPtr.Zero;
            if (_pressure > 0)
            {
                GC.RemoveMemoryPressure(_pressure);
                _pressure = 0;
            }
            if (_module.Generation != _generation) return;
            _module.ProxyDrop(_classId, instance);
        }

        public void Dispose()
        {
            ReleaseInstance();
            GC.SuppressFinalize(this);
        }

        /// <summary>
        /// Frees the value of an object nothing disposed. A public
        /// constructor whose Rust half failed leaves an object no
        /// constructor ran on, whose gate is unset and which holds no
        /// value, so there is nothing to free; a finalizer that threw
        /// would end the process.
        /// </summary>
        ~ProxyBase()
        {
            if (_gate is null) return;
            ReleaseInstance();
        }
    }
}

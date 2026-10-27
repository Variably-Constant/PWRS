using System;

namespace Pwrs
{
    /// <summary>
    /// Base of every generated proxy class that names no base class of its
    /// own. It has the <see cref="ProxyCore"/> its Rust value is reached
    /// through, whose summary says how calls into the value run and when
    /// the value is freed, and gives the generated code that core's calls
    /// under names with a prefix of their own.
    ///
    /// A generated proxy class's public constructor chains to the one
    /// taking the value's pointer after Rust made the value. When Rust
    /// refuses, no constructor of the class or of this one runs, so the
    /// object has no core. Nothing else can reach such an object, and
    /// the core's own finalizer is what frees a value, so there is nothing
    /// for it to free.
    /// </summary>
    public abstract class ProxyBase : IDisposable, IProxy
    {
        private readonly ProxyCore _core;

        protected ProxyBase(NativeModule module, uint classId, IntPtr instance) : this(module, classId, instance, false) { }

        protected ProxyBase(NativeModule module, uint classId, IntPtr instance, bool reportsBytes)
        {
            _core = new ProxyCore(this, module, classId, instance, reportsBytes);
        }

        ProxyCore IProxy.PwrsCore => _core;

        /// <summary>
        /// Reads field <paramref name="fieldId"/> from the Rust value.
        /// Named with the prefix, and called through base by the
        /// generated code, so a module method named Get never enters
        /// the candidate set in its place.
        /// </summary>
        protected object? PwrsGet(uint fieldId) => _core.Get(fieldId);

        /// <summary>
        /// Runs method <paramref name="methodId"/> on the Rust value, as
        /// <see cref="ProxyCore.Call"/> does. Named with the prefix for
        /// the reason PwrsGet is.
        /// </summary>
        protected unsafe object? PwrsCall(uint methodId, void* args, bool exclusive) => _core.Call(methodId, args, exclusive);

        /// <summary>
        /// Runs a method that also takes another object of this class by
        /// reference, as <see cref="ProxyCore.CallWith"/> does.
        /// </summary>
        protected unsafe object? PwrsCallWith(uint methodId, void* args, bool exclusive, ProxyBase peer, IntPtr* peerSlot) =>
            _core.CallWith(methodId, args, exclusive, peer._core, peerSlot);

        public bool IsDisposed => _core is null || _core.IsDisposed;

        public void Dispose()
        {
            _core?.Release();
        }
    }
}

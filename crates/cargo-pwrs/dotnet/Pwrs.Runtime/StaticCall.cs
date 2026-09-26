using System;

namespace Pwrs
{
    /// <summary>
    /// Selects the constructor a copied class keeps for its generated
    /// factory. A constructor a module declares takes only the types its
    /// Rust signature lowers to, and none lowers to this, so the
    /// factory's constructor can never be mistaken for one of them.
    /// Without it, a class declaring a parameterless <c>new()</c> would
    /// have the factory call that constructor, which calls Rust, which
    /// calls the factory.
    /// </summary>
    public struct FromFields { }

    /// <summary>
    /// Runs a static method of a class, a constructor among them. There
    /// is no Rust value to run against, so nothing is gated or
    /// generation-checked and the current load answers. Reached from
    /// the generated code of both proxy and copied classes.
    /// </summary>
    public static class StaticCall
    {
        /// <summary>
        /// Runs static method <paramref name="methodId"/> of class
        /// <paramref name="classId"/> with its packed argument block. A
        /// proxy's constructor answers the pointer of the value it made,
        /// as a long; a copied class's answers the object its factory
        /// built.
        /// </summary>
        public static unsafe object? Invoke(NativeModule module, uint classId, uint methodId, void* args)
        {
            IntPtr result = IntPtr.Zero;
            IntPtr err = IntPtr.Zero;
            int status = module.ProxyCall(classId, methodId, IntPtr.Zero, (IntPtr)args, &result, &err);
            object? errObj = Native.TakeErr(err);
            if (status != Native.Ok)
            {
                throw new PwrsException(errObj as string ?? $"static call failed with status {status}");
            }
            return Native.TakeTarget(result);
        }
    }
}
